//! 对齐 Python `handle_direct_hid_report` / `VoiceShortcut` / `_perform_button_action`
//!
//! 遥控器按键 → 读取 xiaomi.json 的 button_bindings / voice_hotkey → SendInput 注入

use crate::bridges::xiaomi::connect;
use crate::bridges::xiaomi::key_log::{button_label, emit_key_phase, emit_message};
use crate::bridges::xiaomi::tv_gate;
use crate::bridges::xiaomi::voice_chord_state::{VoiceChordState, VoiceInjectionRoute};
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction, VoiceInputProfile};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 与 Python `EXTRA_INFO = 0x584D4952` ('XMIR') 一致，供 LL hook 放行虚拟键
pub const EXTRA_INFO: usize = 0x584D_4952;

/// 实际已通过 SendInput 按下的语音组合键。必须保留原始键位，不能在抬起时
/// 重新读配置，否则配置变更/连接断开会导致 Ctrl、Win 等修饰键粘住。
static VOICE_HELD_KEYS: Mutex<VoiceChordState> = Mutex::new(VoiceChordState::empty());
/// 旧版微信的 Ctrl+Win 是两次独立点按：按下启动、松开结束。它不能复用持续持键
/// 状态，否则遥控器固件持续产生的 F5 会与 Ctrl+Win 组合，被微信再次识别。
static VOICE_WECHAT_TAP_SESSION: Mutex<Option<VoiceTapSession>> = Mutex::new(None);
static DIRECT_MARKS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static REPEAT_GEN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static CLICK_GEN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static CLICK_STATE: Mutex<Option<HashMap<String, ClickState>>> = Mutex::new(None);
static PRESS_STATE: Mutex<Option<HashMap<String, PressState>>> = Mutex::new(None);
static GESTURE_EPOCH: AtomicU64 = AtomicU64::new(1);
static ACTION_SEQ: AtomicU64 = AtomicU64::new(1);
/// Alt+Tab 长按会话期间，物理左右键需要由 LL 钩子转发为带标记的方向键。
static ALT_TAB_HOLD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 语音键在 Windows 上常被译成 F5；记事本 F5=插入日期。
/// 短窗 `direct_signal_recent` 盖不住 typematic，故额外 sticky 抑制直到 F5 抬起或截止。
static VOICE_NATIVE_SUPPRESS: AtomicBool = AtomicBool::new(false);
static VOICE_NATIVE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
/// 对齐 Python `voice_f5_down_suppressed`：一次语音按压周期内吞掉 F5 连发/typematic
static VOICE_F5_DOWN_SUPPRESSED: AtomicBool = AtomicBool::new(false);
/// 如果 F5 DOWN 已经放行给 Windows，配对的 UP 也必须放行；否则会造成实体键盘
/// F5 看似被“粘住”。这个标记只覆盖当前一组 F5 边沿。
static VOICE_F5_DOWN_REACHED_OS: AtomicBool = AtomicBool::new(false);
static VOICE_F5_SUPPRESSED_COUNT: AtomicU64 = AtomicU64::new(0);
static INPUT_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static FIRMWARE_VOICE_HELD: AtomicBool = AtomicBool::new(false);
static VOICE_HOOK_APP: Mutex<Option<AppHandle>> = Mutex::new(None);
/// LL keyboard hook 回调只负责入队；真正的事件、等待和注入均由这个串行 worker
/// 执行，避免阻塞系统键盘钩子并保持 F5 DOWN/UP 的顺序。
static FIRMWARE_VOICE_DISPATCH: OnceLock<mpsc::Sender<(AppHandle, bool)>> = OnceLock::new();
/// 当前非旧版微信语音会话持有的快捷键修饰键。固件 Ctrl/Win 的抬起不能中断它。
static VOICE_HELD_CHORD_MODIFIERS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
/// 应用自身释放虚拟 HID 和弦时的短放行窗口。
static VOICE_CHORD_RELEASE_PASS_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
static VOICE_CHORD_GUARD_LOGGED: AtomicBool = AtomicBool::new(false);
/// ATVV 语音开始后短暂拦截固件冒出的左 Ctrl/左 Win，避免污染目标快捷键。
static VOICE_FIRMWARE_MODIFIER_FILTER: Mutex<FirmwareVoiceModifierFilter> =
    Mutex::new(FirmwareVoiceModifierFilter::empty());
/// 语音 DOWN 时锁定的预设；释放时不重新读取配置。
static VOICE_HELD_PROFILE: Mutex<Option<VoiceInputProfile>> = Mutex::new(None);
static SEND_INPUT_HEALTH: Mutex<SendInputHealth> = Mutex::new(SendInputHealth {
    verified: false,
    last_error: None,
});

#[derive(Debug, Clone)]
struct SendInputHealth {
    verified: bool,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct VoiceTapSession {
    keys: Vec<u16>,
    route: VoiceInjectionRoute,
    profile: Option<VoiceInputProfile>,
}

/// 固件语音报文会泄漏的修饰键（HID 0x05：左 Ctrl + 左 Win）。
pub(crate) const VOICE_FIRMWARE_LEAK_MODIFIER_VKS: [u16; 2] = [0xA2, 0x5B];

#[derive(Debug, Default)]
struct FirmwareVoiceModifierFilter {
    capture_until: Option<Instant>,
    correlated_f5: bool,
    suppressed_modifiers: Vec<u16>,
}

impl FirmwareVoiceModifierFilter {
    const fn empty() -> Self {
        Self {
            capture_until: None,
            correlated_f5: false,
            suppressed_modifiers: Vec::new(),
        }
    }

    fn arm(&mut self, f5_already_suppressed: bool) {
        self.capture_until = Some(Instant::now() + Duration::from_millis(120));
        self.correlated_f5 = f5_already_suppressed;
        self.suppressed_modifiers.clear();
    }

    fn finish_capture(&mut self) {
        self.capture_until = None;
    }

    fn clear(&mut self) {
        self.capture_until = None;
        self.correlated_f5 = false;
        self.suppressed_modifiers.clear();
    }

    fn note_f5_suppressed(&mut self) {
        self.correlated_f5 = true;
        self.finish_capture();
    }

    fn capture_active(&self, now: Instant) -> bool {
        !self.correlated_f5 && self.capture_until.is_some_and(|until| now <= until)
    }

    fn should_swallow(&mut self, vk: u16, down: bool, release_pass_open: bool) -> bool {
        let now = Instant::now();
        if self.capture_until.is_some_and(|until| now > until) {
            self.finish_capture();
        }
        if down
            && self.capture_active(now)
            && VOICE_FIRMWARE_LEAK_MODIFIER_VKS.contains(&vk)
        {
            if !self.suppressed_modifiers.contains(&vk) {
                self.suppressed_modifiers.push(vk);
            }
            return true;
        }
        if !down && self.suppressed_modifiers.contains(&vk) {
            self.suppressed_modifiers.retain(|stored| *stored != vk);
            return !release_pass_open;
        }
        false
    }
}

/// Status of the two keyboard injection layers. A layer is marked verified only
/// after Windows accepted a real shortcut report/event; this deliberately does
/// not claim that a third-party IME has opened its dictation UI.
#[derive(Debug, Clone)]
pub struct InputInjectionHealth {
    pub virtual_hid_ready: bool,
    pub virtual_hid_verified: bool,
    pub send_input_verified: bool,
    pub last_error: Option<String>,
}

pub fn input_injection_health() -> InputInjectionHealth {
    let virtual_hid = crate::bridges::xiaomi::hid_injector::health();
    let send_input = SEND_INPUT_HEALTH.lock().clone();
    InputInjectionHealth {
        virtual_hid_ready: virtual_hid.ready,
        virtual_hid_verified: virtual_hid.report_verified,
        send_input_verified: send_input.verified,
        last_error: virtual_hid.last_error.or(send_input.last_error),
    }
}

fn record_send_input_result(ok: bool, detail: String) {
    let mut health = SEND_INPUT_HEALTH.lock();
    health.verified = ok;
    health.last_error = if ok { None } else { Some(detail) };
}

#[derive(Debug, Clone, Copy, Default)]
struct ClickState {
    count: u8,
    gen: u64,
}

#[derive(Debug, Clone, Default)]
struct PressState {
    gen: u64,
    active: bool,
    held_fired: bool,
    long_bound: bool,
    /// 纯 Alt+Tab 长按时保持的 Alt；Tab 只点按一次，避免系统任务切换器自动连跳。
    held_alt_tab_modifier: Option<u16>,
    /// 长按语音组合键保持到物理语音键抬起，避免 Ctrl/Shift 等修饰键粘住。
    held_keys: Option<Vec<u16>>,
}

/// 输入会话（含仅电量）运行中：供 F5 固件泄漏抑制
pub fn set_input_session_active(active: bool) {
    INPUT_SESSION_ACTIVE.store(active, Ordering::Release);
    if active {
        // 新连接会话：允许再发一次 ATVV 失败 F5 提示
        reset_atvv_f5_toast_throttle();
    } else {
        reset_voice_input_state("input_session_inactive");
        cancel_pending_gestures();
    }
}

/// 连接中断、重启或退出时统一清理语音输入状态。该函数可重复调用。
pub fn reset_voice_input_state(reason: &str) {
    force_release_voice_shortcut(reason);
    clear_voice_chord_guards();
    FIRMWARE_VOICE_HELD.store(false, Ordering::Release);
    disarm_voice_native_suppress();
    VOICE_F5_DOWN_SUPPRESSED.store(false, Ordering::Release);
    VOICE_F5_DOWN_REACHED_OS.store(false, Ordering::Release);
    VOICE_F5_SUPPRESSED_COUNT.store(0, Ordering::Release);
    crate::bridges::xiaomi::voice_pcm::end_session();
    crate::bridges::xiaomi::voice_meter::set_session(false);
}

/// 配置变化或连接结束时取消全部未结算手势，防止旧计时器继续触发。
pub fn cancel_pending_gestures() {
    GESTURE_EPOCH.fetch_add(1, Ordering::AcqRel);
    if let Some(states) = click_states().as_mut() {
        states.clear();
    }
    let (held_alt_tab_modifiers, held_keys) = {
        let mut states = press_states();
        match states.as_mut() {
            Some(states) => {
                let held = states.values_mut().filter_map(take_held_alt_tab_modifier).collect::<Vec<_>>();
                let keys = states.values_mut().filter_map(|state| state.held_keys.take()).collect::<Vec<_>>();
                states.clear();
                (held, keys)
            }
            None => (Vec::new(), Vec::new()),
        }
    };
    for alt_vk in held_alt_tab_modifiers {
        release_held_alt_tab(alt_vk, "gesture_cancel");
    }
    for keys in held_keys {
        release_held_keys(&keys, "gesture_cancel");
    }
    if let Some(gens) = click_gens().as_mut() {
        for gen in gens.values_mut() {
            *gen = gen.wrapping_add(1);
        }
    }
    if let Some(gens) = repeats().as_mut() {
        for gen in gens.values_mut() {
            *gen = gen.wrapping_add(1);
        }
    }
}

pub fn input_session_active() -> bool {
    INPUT_SESSION_ACTIVE.load(Ordering::Acquire)
}

/// 供 F5 固件回退路径发 UI 事件（ATVV 未订阅时语音键仍走 Windows F5）
pub fn bind_voice_hook_app(app: AppHandle) {
    *VOICE_HOOK_APP.lock() = Some(app);
}

/// ATVV 不可用时，由 special_keys 在吞掉固件 F5 后调用，补齐按键映射区的按下/抬起提示
pub fn on_firmware_voice_key(pressed: bool) {
    if connect::atvv_subscribed() {
        return;
    }
    let Some(app) = VOICE_HOOK_APP.lock().clone() else {
        return;
    };
    let sender = FIRMWARE_VOICE_DISPATCH.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<(AppHandle, bool)>();
        let _ = std::thread::Builder::new()
            .name("xiaomi-firmware-voice".into())
            .spawn(move || {
                while let Ok((app, pressed)) = receiver.recv() {
                    process_firmware_voice_key(app, pressed);
                }
            });
        sender
    });
    if sender.send((app, pressed)).is_err() {
        log::warn!("XIAOMI VOICE firmware dispatch worker unavailable");
    }
}

fn process_firmware_voice_key(app: AppHandle, pressed: bool) {
    if pressed {
        if FIRMWARE_VOICE_HELD.swap(true, Ordering::SeqCst) {
            return;
        }
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), true);
        on_remote_button(&app, "mic", true);
        log::debug!("XIAOMI VOICE UI down (firmware F5 fallback)");
    } else {
        if !FIRMWARE_VOICE_HELD.swap(false, Ordering::SeqCst) {
            return;
        }
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), false);
        on_remote_button(&app, "mic", false);
        log::debug!("XIAOMI VOICE UI up (firmware F5 fallback)");
    }
}

/// 与 special_keys F5 抑制策略对齐（测试/文档）
pub const VOICE_F5_SUPPRESS_DEADLINE_MS: u64 = 3_000;

pub fn arm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(true, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() =
        Some(Instant::now() + Duration::from_millis(VOICE_F5_SUPPRESS_DEADLINE_MS));
}

pub fn disarm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() = None;
}

pub fn voice_native_suppress_active() -> bool {
    if !VOICE_NATIVE_SUPPRESS.load(Ordering::Acquire) {
        return false;
    }
    let mut g = VOICE_NATIVE_DEADLINE.lock();
    match *g {
        Some(deadline) if Instant::now() <= deadline => true,
        _ => {
            VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
            *g = None;
            false
        }
    }
}

fn marks() -> parking_lot::MutexGuard<'static, Option<HashMap<String, Instant>>> {
    let mut g = DIRECT_MARKS.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

fn repeats() -> parking_lot::MutexGuard<'static, Option<HashMap<String, u64>>> {
    let mut g = REPEAT_GEN.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

fn click_gens() -> parking_lot::MutexGuard<'static, Option<HashMap<String, u64>>> {
    let mut g = CLICK_GEN.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

fn click_states() -> parking_lot::MutexGuard<'static, Option<HashMap<String, ClickState>>> {
    let mut g = CLICK_STATE.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

fn press_states() -> parking_lot::MutexGuard<'static, Option<HashMap<String, PressState>>> {
    let mut g = PRESS_STATE.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

/// HID DIRECT 刚触发某键：供 special hook 抑制 Windows 原键
pub fn mark_direct_signal(name: &str) {
    marks().as_mut().unwrap().insert(name.to_string(), Instant::now());
    // 别名同步标记，便于 LL hook 用 Python 键名匹配
    for alt in binding_aliases(name) {
        if *alt != name {
            marks()
                .as_mut()
                .unwrap()
                .insert((*alt).to_string(), Instant::now());
        }
    }
    // 语音键原生多为 F5：提前 sticky 抑制，避免 120ms 后 typematic 漏进记事本
    if name == "mic" || name == "voice" || binding_aliases(name).iter().any(|a| *a == "mic") {
        arm_voice_native_suppress();
    }
}

pub fn direct_signal_recent(name: &str, window: Duration) -> bool {
    let g = marks();
    let Some(m) = g.as_ref() else {
        return false;
    };
    if m.get(name).map(|t| t.elapsed() <= window).unwrap_or(false) {
        return true;
    }
    for alt in binding_aliases(name) {
        if m.get(*alt).map(|t| t.elapsed() <= window).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// 对齐 Python `_wait_for_direct_signal`：F5 可能比 ATVV 0x04 先到
fn wait_for_direct_signal(name: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if direct_signal_recent(name, Duration::from_millis(400)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    direct_signal_recent(name, Duration::from_millis(400))
}

/// 对齐 Python `_should_suppress_voice_f5`：关联固件 F5 与语音键，避免记事本刷日期时间
pub fn should_suppress_voice_f5(down: bool, up: bool) -> bool {
    if !down && !up {
        return false;
    }
    if up {
        // DOWN 没被我们吞掉时，绝不能仅因语音会话还在线而吞它的 UP。
        // 这也让普通实体键盘 F5 在 BLE 会话存活期间保持正常。
        if VOICE_F5_DOWN_REACHED_OS.swap(false, Ordering::AcqRel) {
            VOICE_F5_DOWN_SUPPRESSED.store(false, Ordering::Release);
            VOICE_F5_SUPPRESSED_COUNT.store(0, Ordering::Release);
            return false;
        }
        let suppressed = VOICE_F5_DOWN_SUPPRESSED.swap(false, Ordering::AcqRel);
        if suppressed {
            let count = VOICE_F5_SUPPRESSED_COUNT.swap(0, Ordering::AcqRel);
            log::info!("XIAOMI VOICE firmware F5 suppressed count={count}");
        }
        return suppressed;
    }
    let suppressed = if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) {
        true
    } else if !input_session_active() {
        false
    } else if voice_native_suppress_active()
        || direct_signal_recent("voice", Duration::from_millis(300))
        || direct_signal_recent("mic", Duration::from_millis(300))
    {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        true
    } else if wait_for_direct_signal("mic", Duration::from_millis(80)) {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        arm_voice_native_suppress();
        true
    } else {
        // Policy B：关联不上则放行（键盘 F5 可用）。ATVV 挂掉时由 toast 提示，不再会话级全吞。
        false
    };
    if suppressed {
        note_voice_firmware_f5_suppressed();
        let count = VOICE_F5_SUPPRESSED_COUNT.fetch_add(1, Ordering::AcqRel) + 1;
        if count == 1 {
            log::info!("XIAOMI VOICE firmware F5 suppression started");
        }
    }
    suppressed
}

/// special key hook 在决定放行 F5 DOWN 时调用。与 `should_suppress_voice_f5`
/// 配对，确保随后 F5 UP 不会被语音尾窗误吞。
pub fn note_passthrough_f5_down() {
    VOICE_F5_DOWN_REACHED_OS.store(true, Ordering::Release);
}

static ATVV_F5_TOAST_LAST: Mutex<Option<Instant>> = Mutex::new(None);
const ATVV_F5_TOAST_GAP: Duration = Duration::from_secs(60);

fn reset_atvv_f5_toast_throttle() {
    *ATVV_F5_TOAST_LAST.lock() = None;
}

/// N1：会话中且 ATVV 未订阅时，未关联的 F5（多为遥控语音键固件泄漏）→ 系统通知
pub fn on_uncorrelated_f5_down() {
    if !input_session_active() || connect::atvv_subscribed() {
        return;
    }
    {
        let mut last = ATVV_F5_TOAST_LAST.lock();
        if let Some(t) = *last {
            if t.elapsed() < ATVV_F5_TOAST_GAP {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    let Some(app) = VOICE_HOOK_APP.lock().clone() else {
        return;
    };
    use tauri_plugin_notification::NotificationExt;
    log::info!("XIAOMI VOICE F5 toast (atvv down; not suppressed)");
    if let Err(e) = app
        .notification()
        .builder()
        .title("遥控器 ATVV 未连接")
        .body(
            "语音键可能触发系统 F5（如记事本插入日期）。请打开本软件，在小米设置中点击「修复 ATVV 连接」。",
        )
        .show()
    {
        log::warn!("ATVV F5 notification failed: {e}");
    }
}

/// Python / 旧版 UI 键名互认
fn binding_aliases(id: &str) -> &'static [&'static str] {
    match id {
        "up" | "dpad_up" => &["up", "dpad_up"],
        "down" | "dpad_down" => &["down", "dpad_down"],
        "left" | "dpad_left" => &["left", "dpad_left"],
        "right" | "dpad_right" => &["right", "dpad_right"],
        "mic" | "voice" => &["mic", "voice"],
        "volume_mute" | "mute" => &["volume_mute", "mute"],
        _ => &[],
    }
}

pub fn canonical_button_id(id: &str) -> &'static str {
    match id {
        "up" | "dpad_up" => "up",
        "down" | "dpad_down" => "down",
        "left" | "dpad_left" => "left",
        "right" | "dpad_right" => "right",
        "mic" | "voice" => "mic",
        "volume_mute" | "mute" => "volume_mute",
        "power" => "power",
        "volume_up" => "volume_up",
        "volume_down" => "volume_down",
        "ok" => "ok",
        "back" => "back",
        "home" => "home",
        "menu" => "menu",
        "tv" => "tv",
        _ => "unknown",
    }
}

fn is_dedicated_voice_button(id: &str) -> bool {
    matches!(canonical_button_id(id), "mic")
}

fn lookup_action<'a>(config: &'a DeviceConfig, button_id: &str) -> Option<&'a KeyAction> {
    if let Some(a) = config.button_bindings.get(button_id) {
        return Some(a);
    }
    for alt in binding_aliases(button_id) {
        if let Some(a) = config.button_bindings.get(*alt) {
            return Some(a);
        }
    }
    None
}

fn lookup_multi_action<'a>(
    config: &'a DeviceConfig,
    button_id: &str,
    count: u8,
) -> Option<&'a KeyAction> {
    let canonical = canonical_button_id(button_id);
    config
        .multi_click_bindings
        .get(canonical)
        .and_then(|slots| slots.get(&count))
        .or_else(|| {
            for alt in binding_aliases(button_id) {
                if let Some(action) = config
                    .multi_click_bindings
                    .get(*alt)
                    .and_then(|slots| slots.get(&count))
                {
                    return Some(action);
                }
            }
            None
        })
}

fn lookup_long_action<'a>(
    config: &'a DeviceConfig,
    button_id: &str,
) -> Option<&'a KeyAction> {
    let canonical = canonical_button_id(button_id);
    config
        .long_press_bindings
        .get(canonical)
        .or_else(|| {
            for alt in binding_aliases(button_id) {
                if let Some(action) = config.long_press_bindings.get(*alt) {
                    return Some(action);
                }
            }
            None
        })
        .filter(|action| !matches!(action, KeyAction::None))
}

fn multi_slots_for<'a>(
    config: &'a DeviceConfig,
    button_id: &str,
) -> Option<&'a HashMap<u8, KeyAction>> {
    let canonical = canonical_button_id(button_id);
    config.multi_click_bindings.get(canonical).or_else(|| {
        for alt in binding_aliases(button_id) {
            if let Some(slots) = config.multi_click_bindings.get(*alt) {
                return Some(slots);
            }
        }
        None
    })
}

fn has_multi_click(config: &DeviceConfig, button_id: &str) -> bool {
    multi_slots_for(config, button_id)
        .map(|slots| slots.iter().any(|(n, a)| (2..=4).contains(n) && !matches!(a, KeyAction::None)))
        .unwrap_or(false)
}

fn has_long_press(config: &DeviceConfig, button_id: &str) -> bool {
    lookup_long_action(config, button_id).is_some()
}

fn has_single_binding(config: &DeviceConfig, button_id: &str) -> bool {
    lookup_action(config, button_id)
        .map(|action| !matches!(action, KeyAction::None))
        .unwrap_or(false)
}

fn should_arm_native_suppression(config: &DeviceConfig, button_id: &str) -> bool {
    has_single_binding(config, button_id)
        || has_multi_click(config, button_id)
        || has_long_press(config, button_id)
}

fn highest_multi_click(config: &DeviceConfig, button_id: &str) -> u8 {
    multi_slots_for(config, button_id)
        .and_then(|slots| {
            slots
                .iter()
                .filter_map(|(n, a)| {
                    if (2..=4).contains(n) && !matches!(a, KeyAction::None) {
                        Some(*n)
                    } else {
                        None
                    }
                })
                .max()
        })
        .unwrap_or(1)
}

fn multi_interval(config: &DeviceConfig) -> Duration {
    Duration::from_millis(config.multi_click_interval_ms.clamp(150, 800))
}

fn load_xiaomi_config(app: &AppHandle) -> Option<DeviceConfig> {
    let mgr = app.try_state::<ConfigManager>()?;
    mgr.get_device_config("xiaomi").ok()
}

/// 按下遥控器物理键后的统一处理
pub fn on_remote_button(app: &AppHandle, button_id: &str, pressed: bool) {
    if is_dedicated_voice_button(button_id) {
        mark_direct_signal("voice");
        mark_direct_signal("mic");
    }

    let button_id = canonical_button_id(button_id);
    if button_id == "unknown" {
        return;
    }

    if button_id == "tv" && pressed && !tv_gate::is_ready() {
        log::info!("XIAOMI MAPPING tv blocked_by_gate");
        return;
    }

    let Some(config) = load_xiaomi_config(app) else {
        log::warn!("XIAOMI MAPPING no config manager");
        return;
    };

    // Voice is always a dedicated input-method shortcut, never a gesture key.
    if is_dedicated_voice_button(button_id) {
        handle_voice(app, pressed);
        return;
    }

    let multi_enabled = has_multi_click(&config, button_id);
    let long_enabled = has_long_press(&config, button_id);

    // 必须在等待连击/长按之前标记。菜单键原生 VK_APPS 会立即抵达 Windows；
    // 若等到抬起或连击结算，抖音等应用会先收到原生菜单键。
    if pressed && should_arm_native_suppression(&config, button_id) {
        mark_direct_signal(button_id);
    }

    if multi_enabled || long_enabled {
        handle_gesture_button(
            app,
            &config,
            button_id,
            pressed,
            multi_enabled,
            long_enabled,
        );
        return;
    }

    if !pressed {
        mark_direct_signal(button_id);
        cancel_repeat(button_id);
        return;
    }

    let triggered = perform_button_action(&config, button_id);
    log::debug!("XIAOMI MAPPING key={button_id} mapped={triggered} pressed=true");

    if triggered {
        emit_message(app, &format!("单击 → {}", action_label_for_button(&config, button_id, 1)));
        match button_id {
            "back" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(280),
                Duration::from_millis(40),
            ),
            "volume_up" | "volume_down" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(400),
                Duration::from_millis(120),
            ),
            "up" | "down" | "left" | "right" | "dpad_up" | "dpad_down" | "dpad_left"
            | "dpad_right" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(280),
                Duration::from_millis(40),
            ),
            _ => {}
        }
    }
}

fn handle_gesture_button(
    app: &AppHandle,
    config: &DeviceConfig,
    button_id: &str,
    pressed: bool,
    multi_enabled: bool,
    long_enabled: bool,
) {
    if pressed {
        let (gen, epoch) = {
            let mut states = press_states();
            let state = states
                .as_mut()
                .unwrap()
                .entry(button_id.to_string())
                .or_default();
            state.gen = state.gen.wrapping_add(1);
            state.active = true;
            state.held_fired = false;
            state.long_bound = long_enabled;
            state.held_alt_tab_modifier = None;
            state.held_keys = None;
            (state.gen, GESTURE_EPOCH.load(Ordering::Acquire))
        };
        let delay = hold_delay(button_id);
        start_hold_detector(app.clone(), button_id.to_string(), gen, epoch, delay);
        return;
    }

    mark_direct_signal(button_id);
    let (was_hold, held_alt_tab_modifier, held_keys) = {
        let mut states = press_states();
        let Some(state) = states.as_mut().unwrap().get_mut(button_id) else {
            return;
        };
        if !state.active {
            return;
        }
        state.gen = state.gen.wrapping_add(1);
        state.active = false;
        (state.held_fired, take_held_alt_tab_modifier(state), state.held_keys.take())
    };
    cancel_repeat(button_id);
    if let Some(alt_vk) = held_alt_tab_modifier {
        release_held_alt_tab(alt_vk, "remote_up");
    }
    if let Some(keys) = held_keys {
        release_held_keys(&keys, "remote_up");
    }
    if was_hold {
        return;
    }

    if multi_enabled {
        register_short_click(app.clone(), button_id.to_string(), config);
    } else if perform_button_action(config, button_id) {
        emit_message(
            app,
            &format!("单击 → {}", action_label_for_button(config, button_id, 1)),
        );
    }
}

fn hold_delay(button_id: &str) -> Duration {
    match button_id {
        "volume_up" | "volume_down" => Duration::from_millis(400),
        _ => Duration::from_millis(280),
    }
}

fn repeat_interval(button_id: &str) -> Option<Duration> {
    match button_id {
        "back" | "up" | "down" | "left" | "right" => Some(Duration::from_millis(40)),
        "volume_up" | "volume_down" => Some(Duration::from_millis(120)),
        _ => None,
    }
}

fn start_hold_detector(
    app: AppHandle,
    button_id: String,
    gen: u64,
    epoch: u64,
    delay: Duration,
) {
    std::thread::Builder::new()
        .name(format!("xiaomi-hold-{button_id}"))
        .spawn(move || {
            std::thread::sleep(delay);
            if GESTURE_EPOCH.load(Ordering::Acquire) != epoch {
                return;
            }
            let long_bound = {
                let mut states = press_states();
                let state = states
                    .as_mut()
                    .unwrap()
                    .entry(button_id.clone())
                    .or_default();
                if state.gen != gen || !state.active {
                    return;
                }
                state.held_fired = true;
                state.long_bound
            };
            cancel_click_sequence(&button_id);
            if button_id == "tv" && !tv_gate::is_ready() {
                return;
            }
            if let Some(config) = load_xiaomi_config(&app) {
                if GESTURE_EPOCH.load(Ordering::Acquire) != epoch {
                    return;
                }
                if long_bound {
                    if let Some(action) = lookup_long_action(&config, &button_id) {
                        let triggered = if let Some(alt_vk) = plain_alt_tab_modifier(action) {
                            // Keep state registration and injected DOWN in the same critical
                            // section. A physical UP waits for this block, so it cannot release
                            // Alt before the hold has actually been injected.
                            let mut states = press_states();
                            let state = states
                                .as_mut()
                                .unwrap()
                                .entry(button_id.clone())
                                .or_default();
                            if state.gen != gen || !state.active {
                                false
                            } else {
                                begin_held_alt_tab(alt_vk);
                                state.held_alt_tab_modifier = Some(alt_vk);
                                true
                            }
                        } else if button_id == "mic" {
                            // 语音长按的键盘动作必须持续到物理抬起，不能像普通
                            // 映射那样立即 tap，否则 Ctrl+Shift+D 不会保持录音状态。
                            if let Some(keys) = action_virtual_keys(action) {
                                let mut states = press_states();
                                let state = states
                                    .as_mut()
                                    .unwrap()
                                    .entry(button_id.clone())
                                    .or_default();
                                if state.gen != gen || !state.active {
                                    false
                                } else if key_chord(&keys, false) {
                                    state.held_keys = Some(keys);
                                    true
                                } else {
                                    let _ = key_chord(&keys, true);
                                    log::warn!(
                                        "XIAOMI MAPPING voice long-hold DOWN failed keys={keys:?}"
                                    );
                                    false
                                }
                            } else {
                                perform_action(action)
                            }
                        } else {
                            // MouseMove 长按：启动专用循环，按住期间持续移动
                            if let KeyAction::MouseMove { dx, dy, step, accelerate } = action {
                                if let Some(repeat_gen) = reserve_repeat_for_active_press(&button_id, gen) {
                                    start_mouse_move_loop(
                                        button_id.clone(),
                                        *dx,
                                        *dy,
                                        *step,
                                        *accelerate,
                                        repeat_gen,
                                    );
                                    true
                                } else {
                                    false
                                }
                            } else {
                                perform_action(action)
                            }
                        };
                        if triggered {
                            mark_direct_signal(&button_id);
                            emit_message(&app, &format!("长按 → {}", action_label(action)));
                        }
                    }
                } else if perform_button_action(&config, &button_id) {
                    mark_direct_signal(&button_id);
                    emit_message(
                        &app,
                        &format!("长按 → {}", action_label_for_button(&config, &button_id, 1)),
                    );
                    if let Some(interval) = repeat_interval(&button_id) {
                        if let Some(repeat_gen) = reserve_repeat_for_active_press(&button_id, gen) {
                            start_hold_repeat_loop(app, button_id, interval, repeat_gen);
                        }
                    }
                }
            }
        })
        .ok();
}

fn start_hold_repeat_loop(app: AppHandle, button_id: String, interval: Duration, gen: u64) {
    std::thread::Builder::new()
        .name(format!("xiaomi-repeat-{button_id}"))
        .spawn(move || loop {
            std::thread::sleep(interval);
            if !repeat_is_active(&button_id, gen) {
                break;
            }
            if let Some(config) = load_xiaomi_config(&app) {
                let _ = perform_button_action(&config, &button_id);
            }
        })
        .ok();
}

fn register_short_click(app: AppHandle, button_id: String, config: &DeviceConfig) {
    let max_clicks = highest_multi_click(config, &button_id).max(2);
    let interval = multi_interval(config);
    let epoch = GESTURE_EPOCH.load(Ordering::Acquire);
    let (count, gen) = {
        let mut states = click_states();
        let mut gens = click_gens();
        let state = states
            .as_mut()
            .unwrap()
            .entry(button_id.clone())
            .or_default();
        state.count = state.count.saturating_add(1).min(4);
        let gen = gens
            .as_mut()
            .unwrap()
            .entry(button_id.clone())
            .or_insert(0);
        *gen = gen.wrapping_add(1);
        state.gen = *gen;
        (state.count, state.gen)
    };

    if count >= 4 || count >= max_clicks {
        settle_click_sequence(app, button_id, gen, epoch);
        return;
    }

    emit_message(&app, &format!("等待下一次点击（{} / {}）", count, max_clicks));
    std::thread::Builder::new()
        .name(format!("xiaomi-click-{button_id}"))
        .spawn(move || {
            std::thread::sleep(interval);
            settle_click_sequence(app, button_id, gen, epoch);
        })
        .ok();
}

fn settle_click_sequence(app: AppHandle, button_id: String, gen: u64, epoch: u64) {
    if GESTURE_EPOCH.load(Ordering::Acquire) != epoch {
        return;
    }
    let count = {
        let mut states = click_states();
        let mut gens = click_gens();
        let current_gen = gens
            .as_mut()
            .unwrap()
            .entry(button_id.clone())
            .or_insert(0);
        if *current_gen != gen {
            return;
        }
        *current_gen = current_gen.wrapping_add(1);
        states
            .as_mut()
            .unwrap()
            .remove(&button_id)
            .map(|s| s.count)
            .unwrap_or(0)
    };
    if count == 0 {
        return;
    }
    let Some(config) = load_xiaomi_config(&app) else {
        return;
    };
    if GESTURE_EPOCH.load(Ordering::Acquire) != epoch {
        return;
    }
    let action = if count == 1 {
        lookup_action(&config, &button_id)
    } else {
        lookup_multi_action(&config, &button_id, count)
    };
    let Some(action) = action else {
        emit_message(&app, &format!("{}未绑定", click_count_label(count)));
        return;
    };
    if perform_action(action) {
        mark_direct_signal(&button_id);
        emit_message(
            &app,
            &format!(
                "{} → {}",
                click_count_label(count),
                action_label(action)
            ),
        );
        log::debug!("XIAOMI MAPPING key={button_id} clicks={count} mapped=true");
    } else {
        emit_message(&app, &format!("{}未绑定", click_count_label(count)));
    }
}

fn cancel_click_sequence(button_id: &str) {
    {
        let mut states = click_states();
        states.as_mut().unwrap().remove(button_id);
    }
    {
        let mut gens = click_gens();
        let gen = gens
            .as_mut()
            .unwrap()
            .entry(button_id.to_string())
            .or_insert(0);
        *gen = gen.wrapping_add(1);
    }
}

fn cancel_repeat(button_id: &str) {
    let mut map = repeats();
    let gen = map
        .as_mut()
        .unwrap()
        .entry(button_id.to_string())
        .or_insert(0);
    *gen = gen.wrapping_add(1);
}

/// Reserve a repeat generation while the matching physical press is still active.
/// The release path changes `active` before invalidating this generation, which
/// prevents a delayed hold detector from creating a loop after key-up.
fn reserve_repeat_for_active_press(button_id: &str, press_gen: u64) -> Option<u64> {
    let states = press_states();
    let state = states.as_ref()?.get(button_id)?;
    if state.gen != press_gen || !state.active {
        return None;
    }
    let mut map = repeats();
    let repeat_gen = map
        .as_mut()
        .unwrap()
        .entry(button_id.to_string())
        .or_insert(0);
    *repeat_gen = repeat_gen.wrapping_add(1);
    Some(*repeat_gen)
}

fn repeat_is_active(button_id: &str, generation: u64) -> bool {
    let map = repeats();
    map.as_ref().and_then(|m| m.get(button_id)).copied() == Some(generation)
}

fn start_hold_repeat(app: AppHandle, button_id: String, delay: Duration, interval: Duration) {
    let gen = {
        let mut map = repeats();
        let e = map.as_mut().unwrap().entry(button_id.clone()).or_insert(0);
        *e = e.wrapping_add(1);
        *e
    };
    std::thread::Builder::new()
        .name(format!("xiaomi-repeat-{button_id}"))
        .spawn(move || {
            std::thread::sleep(delay);
            loop {
                {
                    let map = repeats();
                    if map.as_ref().and_then(|m| m.get(&button_id)).copied() != Some(gen) {
                        break;
                    }
                }
                if button_id == "tv" && !tv_gate::is_ready() {
                    break;
                }
                if let Some(config) = load_xiaomi_config(&app) {
                    let _ = perform_button_action(&config, &button_id);
                }
                std::thread::sleep(interval);
            }
        })
        .ok();
}

fn perform_button_action(config: &DeviceConfig, button_id: &str) -> bool {
    let Some(action) = lookup_action(config, button_id) else {
        return false;
    };
    perform_action(action)
}

fn perform_action(action: &KeyAction) -> bool {
    match action {
        KeyAction::None => false,
        KeyAction::SingleKey(vk) => {
            tap_vks(&[*vk], 20);
            true
        }
        KeyAction::ComboKey(vks) if !vks.is_empty() => {
            tap_vks(vks, 70);
            true
        }
        KeyAction::ComboKey(_) => false,
        KeyAction::TextInput(text) => {
            tap_unicode_text(text);
            true
        }
        KeyAction::LaunchApp(path) => {
            let _ = std::process::Command::new(path).spawn();
            true
        }
        KeyAction::MouseClick => mouse_left_click(),
        KeyAction::MouseMove { dx, dy, step, .. } => {
            mouse_move_relative(*dx * *step as i32, *dy * *step as i32)
        }
    }
}

fn action_label_for_button(config: &DeviceConfig, button_id: &str, count: u8) -> String {
    let action = if count == 1 {
        lookup_action(config, button_id)
    } else {
        lookup_multi_action(config, button_id, count)
    };
    action.map(action_label).unwrap_or_else(|| "未绑定".into())
}

fn action_label(action: &KeyAction) -> String {
    match action {
        KeyAction::None => "未绑定".into(),
        KeyAction::SingleKey(vk) => crate::bridges::xiaomi::config::vk_code_to_name(*vk),
        KeyAction::ComboKey(vks) if !vks.is_empty() => vks
            .iter()
            .map(|vk| crate::bridges::xiaomi::config::vk_code_to_name(*vk))
            .collect::<Vec<_>>()
            .join(" + "),
        KeyAction::ComboKey(_) => "未绑定".into(),
        KeyAction::TextInput(text) => format!("文字: {text}"),
        KeyAction::LaunchApp(path) => format!("启动: {path}"),
        KeyAction::MouseClick => "鼠标左键".into(),
        KeyAction::MouseMove { dx, dy, step, .. } => {
            let dir = match (*dx, *dy) {
                (0, -1) => "鼠标↑",
                (0, 1) => "鼠标↓",
                (-1, 0) => "鼠标←",
                (1, 0) => "鼠标→",
                _ => "鼠标移动",
            };
            format!("{dir} {step}px")
        }
    }
}

fn click_count_label(count: u8) -> &'static str {
    match count {
        1 => "单击",
        2 => "双击",
        3 => "三击",
        4 => "四连击",
        _ => "连击",
    }
}

fn handle_voice(app: &AppHandle, pressed: bool) {
    // 抬起必须优先释放已记录的原始组合键；此时配置可能已关闭、丢失或变更。
    if !pressed {
        force_release_voice_shortcut("remote_up");
        return;
    }
    let Some(config) = load_xiaomi_config(app) else {
        return;
    };
    if !config.voice_shortcut_enabled {
        log::info!("XIAOMI VOICE shortcut disabled");
        return;
    }
    let vks = resolve_voice_hotkey(&config);
    if vks.is_empty() {
        log::warn!("XIAOMI VOICE shortcut empty");
        return;
    }
    let profile = config.voice_input_profile;
    arm_voice_firmware_modifier_filter();
    wait_for_clean_voice_modifier_window();
    if is_wechat_start_voice_shortcut(&vks, profile) {
        if let Some(route) = start_wechat_start_voice_session(&vks, profile) {
            log::info!(
                "XIAOMI VOICE WeChat start-voice tap route={} vks={vks:?}",
                voice_injection_route_label(route),
            );
        }
        return;
    }
    // 点击 / 按住：快捷键都跟遥控按下/抬起走（短按≈点按，长按=按住）。
    // 先截掉固件自带 Ctrl/Win，目标和弦才会落在干净的修饰键状态上。
    if let Some(route) = press_voice_shortcut(&vks, profile, "remote_down") {
        log::info!(
            "XIAOMI VOICE SHORTCUT DOWN mode={:?} route={} vks={vks:?}",
            config.trigger_mode,
            voice_injection_route_label(route),
        );
    }
}

/// 点击模式：短按判定为「点按一次」完整 tap（若尚未因长按而 DOWN）
pub fn voice_shortcut_tap(app: &AppHandle) -> &'static str {
    // 若已经按住 DOWN，先松开再读取当前配置，避免配置读取失败时粘键。
    force_release_voice_shortcut("click_tap");
    let Some(config) = load_xiaomi_config(app) else {
        return "none";
    };
    if !config.voice_shortcut_enabled {
        return "none";
    }
    let vks = resolve_voice_hotkey(&config);
    if vks.is_empty() {
        return "none";
    }
    let hold = if vks.iter().any(|vk| matches!(vk, 0x5B | 0x5C)) {
        120
    } else {
        70
    };
    wait_for_leak_modifiers_released();
    let route = press_voice_shortcut(&vks, config.voice_input_profile, "click_tap");
    let route = match route {
        Some(route) => {
            std::thread::sleep(Duration::from_millis(hold));
            let _ = force_release_voice_shortcut("click_tap");
            route
        }
        None => VoiceInjectionRoute::SendInputFallback,
    };
    log::info!(
        "XIAOMI VOICE SHORTCUT TAP (click) route={} vks={vks:?} hold_ms={hold}",
        voice_injection_route_label(route),
    );
    voice_injection_route_label(route)
}

/// Click mode still needs a complete key-down/key-up pair, but the IME is now
/// activated on AUDIO_START.  Win chords get a slightly longer minimum hold.
pub fn voice_shortcut_min_hold_ms(app: &AppHandle) -> u64 {
    let Some(config) = load_xiaomi_config(app) else {
        return 70;
    };
    if resolve_voice_hotkey(&config)
        .iter()
        .any(|vk| matches!(vk, 0x5B | 0x5C))
    {
        120
    } else {
        70
    }
}

/// 点击模式：确认已进入长按后补发 DOWN（若尚未 DOWN）
pub fn voice_shortcut_ensure_down(app: &AppHandle) {
    let Some(config) = load_xiaomi_config(app) else {
        return;
    };
    if !config.voice_shortcut_enabled {
        return;
    }
    let vks = resolve_voice_hotkey(&config);
    if vks.is_empty() {
        return;
    }
    wait_for_leak_modifiers_released();
    if let Some(route) = press_voice_shortcut(&vks, config.voice_input_profile, "hold_threshold") {
        log::info!(
            "XIAOMI VOICE SHORTCUT DOWN (hold-after-click-threshold) route={} vks={vks:?}",
            voice_injection_route_label(route),
        );
    }
}

fn press_voice_shortcut(
    vks: &[u16],
    profile: Option<VoiceInputProfile>,
    reason: &str,
) -> Option<VoiceInjectionRoute> {
    let mut state = VOICE_HELD_KEYS.lock();
    if state.is_held() {
        log::debug!("XIAOMI VOICE SHORTCUT DOWN ignored already_held reason={reason}");
        return None;
    }
    let pressed = state.press_with(
        vks,
        inject_voice_shortcut_down,
        compensate_voice_shortcut_down,
    );
    if pressed {
        *VOICE_HELD_PROFILE.lock() = profile;
        set_voice_held_chord_modifiers(vks);
    } else {
        clear_voice_chord_guards();
        log::warn!("XIAOMI VOICE SHORTCUT DOWN failed reason={reason} vks={vks:?}");
    }
    state.held_route()
}

/// 释放实际按下的组合键；对同一会话重复调用不产生额外键盘事件。
pub fn force_release_voice_shortcut(reason: &str) -> bool {
    // 停止点按与清空会话必须经同一守卫完成：parking_lot 不可重入，
    // 持锁期间再次 lock() 同一把锁会自锁死调用线程。
    let mut session_slot = VOICE_WECHAT_TAP_SESSION.lock();
    if let Some(session) = session_slot.clone() {
        let released = tap_voice_shortcut_on_route(&session.keys, session.route, 120, false);
        if released {
            *session_slot = None;
            clear_voice_chord_guards();
            log::info!(
                "XIAOMI VOICE WeChat start-voice stop tap reason={reason} profile={:?} route={} vks={:?}",
                session.profile,
                voice_injection_route_label(session.route),
                session.keys,
            );
        } else {
            log::error!(
                "XIAOMI VOICE WeChat start-voice stop tap failed reason={reason} profile={:?} route={} vks={:?}",
                session.profile,
                voice_injection_route_label(session.route),
                session.keys,
            );
        }
        return released;
    }
    drop(session_slot);
    let profile = *VOICE_HELD_PROFILE.lock();
    open_voice_chord_release_pass_window();
    let mut state = VOICE_HELD_KEYS.lock();
    let Some((keys, route, mut released)) =
        state.release_with(|keys, route| release_voice_shortcut(keys, route, profile))
    else {
        return false;
    };
    if !released && route == VoiceInjectionRoute::VirtualHid {
        // The virtual device may have disappeared after accepting DOWN.  Drop
        // that handle, then use KEYUP only as an emergency for keys owned by
        // this voice session.  The state is cleared only after verification.
        crate::bridges::xiaomi::hid_injector::reset_and_retry();
        released = key_chord(&keys, true) && wait_for_owned_modifiers_released(&keys);
        if released {
            state.clear_after_verified_release();
            log::warn!(
                "XIAOMI VOICE SHORTCUT UP recovered via emergency SendInput reason={reason} vks={keys:?}"
            );
        }
    }
    if released {
        clear_voice_chord_guards();
        log::info!(
            "XIAOMI VOICE SHORTCUT UP reason={reason} route={} vks={keys:?}",
            voice_injection_route_label(route),
        );
    } else {
        log::error!("XIAOMI VOICE SHORTCUT UP failed reason={reason} vks={keys:?}");
    }
    released
}

fn voice_injection_route_label(route: VoiceInjectionRoute) -> &'static str {
    match route {
        VoiceInjectionRoute::VirtualHid => "virtual_hid",
        VoiceInjectionRoute::SendInputFallback => "send_input_fallback",
    }
}

/// Diagnostic-only view of the route fixed for the active voice press.
pub fn current_voice_route_label() -> &'static str {
    VOICE_WECHAT_TAP_SESSION
        .lock()
        .as_ref()
        .map(|session| voice_injection_route_label(session.route))
        .or_else(|| VOICE_HELD_KEYS
        .lock()
        .held_route()
        .map(voice_injection_route_label))
        .unwrap_or("none")
}

/// Voice shortcuts are the only path allowed to bypass the ordinary Alt/Space/media
/// exclusions. IME global shortcuts such as Doubao's right Alt need hardware origin.
fn should_try_virtual_hid_for_voice(vks: &[u16]) -> bool {
    !vks.is_empty()
}

fn inject_voice_shortcut_down(vks: &[u16]) -> Option<VoiceInjectionRoute> {
    if should_try_virtual_hid_for_voice(vks)
        && crate::bridges::xiaomi::hid_injector::press_ready(vks).is_ok()
    {
        return Some(VoiceInjectionRoute::VirtualHid);
    }

    // press_ready compensates a failed driver report on its original handle
    // before disposing it.  Never call release() here: that would synchronously
    // recreate WinUHid in the same voice-key callback.
    if key_chord(vks, false) {
        log::warn!("XIAOMI VOICE shortcut primary virtual_hid failed; route=send_input_fallback vks={vks:?}");
        Some(VoiceInjectionRoute::SendInputFallback)
    } else {
        None
    }
}

fn is_wechat_start_voice_shortcut(vks: &[u16], profile: Option<VoiceInputProfile>) -> bool {
    // 微信“启动语音输入”是 Ctrl+Win 点击型快捷键：遥控器按下和抬起
    // 分别发送一次短脉冲，不在固件重复流期间持续持有组合键。
    // 已弃用的 WechatHold 会在配置边界迁移，不能进入此运行时分支。
    matches!(profile, Some(VoiceInputProfile::Wechat))
        && matches!(vks, [0xA2, 0x5B] | [0x5B, 0xA2])
}

fn arm_voice_firmware_modifier_filter() {
    let f5_already_suppressed = VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire);
    VOICE_FIRMWARE_MODIFIER_FILTER
        .lock()
        .arm(f5_already_suppressed);
    log::debug!(
        "XIAOMI VOICE firmware modifier prefilter armed f5_already_suppressed={f5_already_suppressed}"
    );
}

fn note_voice_firmware_f5_suppressed() {
    VOICE_FIRMWARE_MODIFIER_FILTER
        .lock()
        .note_f5_suppressed();
}

fn wait_for_clean_voice_modifier_window() {
    // F5 可能比 ATVV MIC_OPEN 早到；最多等待一个短窗口，绝不阻塞语音链路。
    for _ in 0..8 {
        if VOICE_FIRMWARE_MODIFIER_FILTER.lock().correlated_f5
            || VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    VOICE_FIRMWARE_MODIFIER_FILTER.lock().finish_capture();
    wait_for_leak_modifiers_released();
}

fn wait_for_leak_modifiers_released() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        for _ in 0..30 {
            let dirty = VOICE_FIRMWARE_LEAK_MODIFIER_VKS
                .iter()
                .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0);
            if !dirty {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        log::warn!("XIAOMI VOICE clean modifier window timed out after 150ms; inject anyway");
    }
}

pub fn should_swallow_voice_firmware_modifier(vk: u16, down: bool) -> bool {
    let release_pass_open = voice_chord_release_pass_open();
    let swallow = VOICE_FIRMWARE_MODIFIER_FILTER
        .lock()
        .should_swallow(vk, down, release_pass_open);
    if swallow {
        log::debug!(
            "XIAOMI VOICE firmware modifier prefilter swallowed vk=0x{vk:02X} down={down}"
        );
    }
    swallow
}

fn set_voice_held_chord_modifiers(vks: &[u16]) {
    *VOICE_HELD_CHORD_MODIFIERS.lock() = vks
        .iter()
        .copied()
        .filter(|vk| is_modifier_key(*vk))
        .collect();
    VOICE_CHORD_GUARD_LOGGED.store(false, Ordering::Release);
}

fn open_voice_chord_release_pass_window() {
    *VOICE_CHORD_RELEASE_PASS_UNTIL.lock() =
        Some(Instant::now() + Duration::from_millis(300));
}

fn voice_chord_release_pass_open() -> bool {
    let deadline = VOICE_CHORD_RELEASE_PASS_UNTIL.lock();
    matches!(*deadline, Some(until) if Instant::now() <= until)
}

fn clear_voice_chord_guards() {
    VOICE_HELD_CHORD_MODIFIERS.lock().clear();
    *VOICE_CHORD_RELEASE_PASS_UNTIL.lock() = None;
    VOICE_FIRMWARE_MODIFIER_FILTER.lock().clear();
    *VOICE_HELD_PROFILE.lock() = None;
    VOICE_CHORD_GUARD_LOGGED.store(false, Ordering::Release);
}

pub fn should_swallow_voice_chord_modifier_up(vk: u16) -> bool {
    let guarded = VOICE_HELD_CHORD_MODIFIERS.lock().clone();
    let swallow = voice_chord_modifier_up_guarded(&guarded, vk, voice_chord_release_pass_open());
    if swallow && !VOICE_CHORD_GUARD_LOGGED.swap(true, Ordering::AcqRel) {
        log::info!("XIAOMI VOICE chord modifier keyup guard active vk=0x{vk:02X}");
    }
    swallow
}

pub(crate) fn voice_chord_modifier_up_guarded(
    guarded: &[u16],
    event_vk: u16,
    release_pass_open: bool,
) -> bool {
    if release_pass_open || guarded.is_empty() {
        return false;
    }
    guarded
        .iter()
        .any(|&guard_vk| voice_guard_vk_covers(guard_vk, event_vk))
}

fn voice_guard_vk_covers(guard_vk: u16, event_vk: u16) -> bool {
    if guard_vk == event_vk {
        return true;
    }
    let family = |vk: u16| match vk {
        0x10 | 0xA0 | 0xA1 => 0x10u16,
        0x11 | 0xA2 | 0xA3 => 0x11,
        0x12 | 0xA4 | 0xA5 => 0x12,
        other => other,
    };
    matches!(guard_vk, 0x10 | 0x11 | 0x12) && family(guard_vk) == family(event_vk)
}

fn wait_for_first_voice_f5_suppression() {
    for _ in 0..8 {
        if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start_wechat_start_voice_session(
    vks: &[u16],
    profile: Option<VoiceInputProfile>,
) -> Option<VoiceInjectionRoute> {
    if VOICE_WECHAT_TAP_SESSION.lock().is_some() {
        log::debug!("XIAOMI VOICE WeChat start-voice ignored already_active");
        return None;
    }
    // Let the correlated physical F5 reach our suppression hook first.  The
    // shortcut is then a short edge-triggered pulse, never a chord held
    // alongside the remote firmware's repeat stream.
    wait_for_first_voice_f5_suppression();
    let route = inject_voice_shortcut_down(vks)?;
    if !tap_voice_shortcut_on_route(vks, route, 120, true) {
        compensate_voice_shortcut_down(vks);
        return None;
    }
    *VOICE_WECHAT_TAP_SESSION.lock() = Some(VoiceTapSession {
        keys: vks.to_vec(),
        route,
        profile,
    });
    Some(route)
}

fn tap_voice_shortcut_on_route(
    vks: &[u16],
    route: VoiceInjectionRoute,
    hold_ms: u64,
    key_is_already_down: bool,
) -> bool {
    // The caller has already sent the initial DOWN for a start tap.  For a
    // stop tap we need to emit that DOWN here; callers pass a route stored at
    // start so both halves of the voice session use the same injection layer.
    if !key_is_already_down {
        let pressed = match route {
            VoiceInjectionRoute::VirtualHid => crate::bridges::xiaomi::hid_injector::press_ready(vks).is_ok(),
            VoiceInjectionRoute::SendInputFallback => key_chord(vks, false),
        };
        if !pressed {
            return false;
        }
    }
    std::thread::sleep(Duration::from_millis(hold_ms.clamp(20, 1000)));
    let mut released = false;
    for _ in 0..3 {
        released = match route {
            VoiceInjectionRoute::VirtualHid => crate::bridges::xiaomi::hid_injector::release_ready(vks).is_ok(),
            VoiceInjectionRoute::SendInputFallback => key_chord(vks, true),
        };
        if released {
            break;
        }
    }
    if !released && route == VoiceInjectionRoute::VirtualHid {
        crate::bridges::xiaomi::hid_injector::reset_and_retry();
        released = key_chord(vks, true);
    }
    released && wait_for_owned_modifiers_released(vks)
}

fn compensate_voice_shortcut_down(vks: &[u16]) {
    let _ = crate::bridges::xiaomi::hid_injector::release_ready(vks);
    let _ = key_chord(vks, true);
}

fn release_voice_shortcut(
    vks: &[u16],
    route: VoiceInjectionRoute,
    profile: Option<VoiceInputProfile>,
) -> bool {
    // Pure Win/Alt shortcuts must be turned into a real chord
    // before the modifiers lift, otherwise Windows can open Start/the system
    // menu. WeChat start-voice Ctrl+Win is handled above as two short taps and
    // never reaches this F24-based held-chord release path.
    let released = execute_voice_release_steps(&voice_release_steps(vks, route, profile), route);
    if matches!(profile, Some(VoiceInputProfile::Qianwen)) {
        log::info!(
            "XIAOMI VOICE Qianwen direct release route={} vks={vks:?}",
            voice_injection_route_label(route),
        );
    }
    released && wait_for_owned_modifiers_released(vks)
}

fn is_modifier_key(vk: u16) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

fn voice_needs_shell_neutralizer(vks: &[u16]) -> bool {
    !vks.is_empty()
        && vks.iter().all(|vk| is_modifier_key(*vk))
        && vks.iter().any(|vk| matches!(vk, 0x12 | 0xA4 | 0xA5 | 0x5B | 0x5C))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VoiceReleaseStep {
    Press(Vec<u16>),
    Release(Vec<u16>),
}

#[cfg(test)]
fn wechat_start_voice_tap_steps(vks: &[u16]) -> Vec<VoiceReleaseStep> {
    vec![
        VoiceReleaseStep::Press(vks.to_vec()),
        VoiceReleaseStep::Release(vks.to_vec()),
    ]
}

fn neutralized_voice_release_steps(vks: &[u16], route: VoiceInjectionRoute) -> Vec<VoiceReleaseStep> {
    const VK_F24: u16 = 0x87;
    match route {
        VoiceInjectionRoute::VirtualHid => {
            let mut chord = vks.to_vec();
            chord.push(VK_F24);
            vec![
                VoiceReleaseStep::Press(chord),
                VoiceReleaseStep::Press(vec![VK_F24]),
                VoiceReleaseStep::Release(vec![VK_F24]),
            ]
        }
        VoiceInjectionRoute::SendInputFallback => vec![
            VoiceReleaseStep::Press(vec![VK_F24]),
            VoiceReleaseStep::Release(vks.to_vec()),
            VoiceReleaseStep::Release(vec![VK_F24]),
        ],
    }
}

fn voice_release_steps(
    vks: &[u16],
    route: VoiceInjectionRoute,
    profile: Option<VoiceInputProfile>,
) -> Vec<VoiceReleaseStep> {
    if matches!(profile, Some(VoiceInputProfile::Qianwen)) {
        return vec![VoiceReleaseStep::Release(vks.to_vec())];
    }
    if voice_needs_shell_neutralizer(vks) {
        neutralized_voice_release_steps(vks, route)
    } else {
        vec![VoiceReleaseStep::Release(vks.to_vec())]
    }
}

fn execute_voice_release_steps(steps: &[VoiceReleaseStep], route: VoiceInjectionRoute) -> bool {
    let released = steps.iter().cloned().all(|step| match (route, step) {
        (VoiceInjectionRoute::VirtualHid, VoiceReleaseStep::Press(keys)) => {
            crate::bridges::xiaomi::hid_injector::press_ready(&keys).is_ok()
        }
        (VoiceInjectionRoute::VirtualHid, VoiceReleaseStep::Release(keys)) => {
            crate::bridges::xiaomi::hid_injector::release_ready(&keys).is_ok()
        }
        (VoiceInjectionRoute::SendInputFallback, VoiceReleaseStep::Press(keys)) => {
            key_chord(&keys, false)
        }
        (VoiceInjectionRoute::SendInputFallback, VoiceReleaseStep::Release(keys)) => {
            key_chord(&keys, true)
        }
    });
    log::info!(
        "XIAOMI VOICE release route={} result={released}",
        voice_injection_route_label(route),
    );
    released
}

fn wait_for_owned_modifiers_released(vks: &[u16]) -> bool {
    for attempt in 0..=4 {
        if owned_modifiers_released(vks, false) {
            if attempt > 0 {
                log::info!(
                    "XIAOMI VOICE modifier release confirmed after {}ms",
                    attempt * 10,
                );
            }
            return true;
        }
        if attempt < 4 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    owned_modifiers_released(vks, true)
}

fn owned_modifiers_released(vks: &[u16], log_stuck_key: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        // GetAsyncKeyState reports the high bit while a key is down.  Only
        // check modifiers this application injected; normal letters can be
        // physically held by the user and must not block cleanup.
        for &vk in vks.iter().filter(|vk| is_modifier_key(**vk)) {
            if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
                if log_stuck_key {
                    log::warn!("XIAOMI VOICE modifier still down after release confirmation vk=0x{vk:02X}");
                }
                return false;
            }
        }
    }
    true
}

/// ATVV opcode 路径调用（对齐 VoiceShortcut.press/release/tap）
pub fn voice_from_atvv(app: &AppHandle, opcode: u8) {
    match opcode {
        0x04 => on_remote_button(app, "mic", true),
        0x00 => on_remote_button(app, "mic", false),
        _ => {}
    }
}

fn resolve_voice_hotkey(config: &DeviceConfig) -> Vec<u16> {
    // 对齐 Python voice_hotkey_from_configs：界面上的 mic 按键映射优先于 voice_hotkey 字段
    if let Some(action) = config.button_bindings.get("mic") {
        if let Some(vks) = action_to_vks(action) {
            return vks;
        }
    }
    if let Some(action) = config.button_bindings.get("voice") {
        if let Some(vks) = action_to_vks(action) {
            return vks;
        }
    }
    if let Some(keys) = &config.voice_hotkey {
        let mut out = Vec::new();
        for k in keys {
            if let Some(vk) = name_to_vk(k) {
                out.push(vk);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    vec![0xA5] // 默认右 Alt
}

fn action_to_vks(action: &KeyAction) -> Option<Vec<u16>> {
    match action {
        KeyAction::SingleKey(vk) => Some(vec![*vk]),
        KeyAction::ComboKey(vks) if !vks.is_empty() => Some(vks.clone()),
        _ => None,
    }
}

fn vks_to_hotkey_names(vks: &[u16]) -> Vec<String> {
    vks.iter()
        .map(|&vk| match vk {
            0xA2 => "leftctrl".into(),
            0xA3 => "rightctrl".into(),
            0x11 => "ctrl".into(),
            0xA0 => "leftshift".into(),
            0xA1 => "rightshift".into(),
            0x10 => "shift".into(),
            0xA4 => "leftalt".into(),
            0xA5 => "rightalt".into(),
            0x12 => "alt".into(),
            0x5B => "leftwin".into(),
            0x5C => "rightwin".into(),
            0x20 => "space".into(),
            0x0D => "enter".into(),
            0x08 => "backspace".into(),
            0x1B => "esc".into(),
            0x13 => "pause".into(),
            0x14 => "capslock".into(),
            0x2C => "printscreen".into(),
            0x5D => "menu".into(),
            0x90 => "numlock".into(),
            0x91 => "scrolllock".into(),
            0x6A => "numpadmult".into(),
            0x6B => "numpadadd".into(),
            0x6D => "numpadsubtract".into(),
            0x6E => "numpaddecimal".into(),
            0x6F => "numpaddivide".into(),
            0xBA => "semicolon".into(),
            0xBB => "equal".into(),
            0xBC => "comma".into(),
            0xBD => "minus".into(),
            0xBE => "period".into(),
            0xBF => "slash".into(),
            0xC0 => "grave".into(),
            0xDB => "bracketleft".into(),
            0xDC => "backslash".into(),
            0xDD => "bracketright".into(),
            0xDE => "apostrophe".into(),
            other if (0x60..=0x69).contains(&other) => format!("numpad{}", other - 0x60),
            other if (0x41..=0x5A).contains(&other) => {
                ((other as u8) as char).to_ascii_lowercase().to_string()
            }
            other if (0x30..=0x39).contains(&other) => {
                char::from(b'0' + (other - 0x30) as u8).to_string()
            }
            other if (0x70..=0x87).contains(&other) => format!("f{}", other - 0x6F),
            other => format!("vk_{other:02x}"),
        })
        .collect()
}

/// 保存前：mic 映射同步到 voice_hotkey / voice 别名（对齐 Python 保存逻辑）
pub fn sync_voice_from_mic_binding(config: &mut DeviceConfig) {
    let mic = config
        .button_bindings
        .get("mic")
        .cloned()
        .or_else(|| config.button_bindings.get("voice").cloned());
    let Some(action) = mic else {
        return;
    };
    let Some(vks) = action_to_vks(&action) else {
        return;
    };
    config.voice_hotkey = Some(vks_to_hotkey_names(&vks));
    config.button_bindings.insert("mic".into(), action.clone());
    config.button_bindings.insert("voice".into(), action);
    if config
        .voice_input_profile
        .is_some_and(|profile| !profile.matches_voice_binding(&config.button_bindings["mic"], &config.trigger_mode))
    {
        log::info!("XIAOMI CONFIG cleared stale voice_input_profile after manual voice shortcut change");
        config.voice_input_profile = None;
    }
}

fn name_to_vk(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase().replace(' ', "");
    if let Some(hex) = n.strip_prefix("vk_") {
        return u16::from_str_radix(hex, 16).ok();
    }
    if let Some(digits) = n.strip_prefix("numpad") {
        if let Ok(d) = digits.parse::<u16>() {
            if d <= 9 {
                return Some(0x60 + d);
            }
        }
        return match n.as_str() {
            "numpadmult" => Some(0x6A),
            "numpadadd" => Some(0x6B),
            "numpadsubtract" => Some(0x6D),
            "numpaddecimal" => Some(0x6E),
            "numpaddivide" => Some(0x6F),
            _ => None,
        };
    }
    if let Some(number) = n.strip_prefix('f') {
        if let Ok(number) = number.parse::<u16>() {
            if (1..=24).contains(&number) {
                return Some(0x6F + number);
            }
        }
    }
    match n.as_str() {
        "backspace" => Some(0x08),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "pause" => Some(0x13),
        "capslock" => Some(0x14),
        "shift" => Some(0x10),
        "ctrl" | "control" => Some(0x11),
        "alt" => Some(0x12),
        "esc" | "escape" => Some(0x1B),
        "space" => Some(0x20),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "home" => Some(0x24),
        "printscreen" | "prtsc" => Some(0x2C),
        "menu" | "apps" => Some(0x5D),
        "numlock" => Some(0x90),
        "scrolllock" | "scrlk" => Some(0x91),
        "f10" => Some(0x79),
        "d" => Some(0x44),
        "win" | "leftwin" | "lwin" => Some(0x5B),
        "rightwin" | "rwin" => Some(0x5C),
        "leftshift" => Some(0xA0),
        "rightshift" => Some(0xA1),
        "leftctrl" => Some(0xA2),
        "rightctrl" => Some(0xA3),
        "leftalt" => Some(0xA4),
        "rightalt" | "ralt" | "rmenu" => Some(0xA5),
        "volume_mute" | "volumemute" => Some(0xAD),
        "volume_down" | "volumedown" => Some(0xAE),
        "volume_up" | "volumeup" => Some(0xAF),
        "semicolon" => Some(0xBA),
        "equal" => Some(0xBB),
        "comma" => Some(0xBC),
        "minus" => Some(0xBD),
        "period" => Some(0xBE),
        "slash" => Some(0xBF),
        "grave" => Some(0xC0),
        "bracketleft" => Some(0xDB),
        "backslash" => Some(0xDC),
        "bracketright" => Some(0xDD),
        "apostrophe" => Some(0xDE),
        other if other.len() == 1 => {
            let c = other.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                Some(c as u16)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2C | 0x2D | 0x2E | 0x5B
            | 0x5C | 0x5D | 0xA3 | 0xA5 | 0xA6..=0xB7
    )
}

fn is_alt_modifier(vk: u16) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5) // VK_MENU, VK_LMENU, VK_RMENU
}

fn has_alt_modifier(vks: &[u16]) -> bool {
    vks.iter().any(|&vk| is_alt_modifier(vk))
}

fn is_system_alt_tab_chord(vks: &[u16]) -> bool {
    has_alt_modifier(vks) && vks.contains(&0x09) // VK_TAB
}

/// 仅识别没有额外修饰键或普通键的纯 Alt+Tab。
/// 这种组合在长按槽位中需要保留 Alt，其他包含 Tab 的系统和弦仍按普通 tap 处理。
fn plain_alt_tab_modifier(action: &KeyAction) -> Option<u16> {
    let KeyAction::ComboKey(vks) = action else {
        return None;
    };
    if vks.len() != 2 || !vks.contains(&0x09) {
        return None;
    }
    vks.iter().copied().find(|&vk| is_alt_modifier(vk))
}

fn take_held_alt_tab_modifier(state: &mut PressState) -> Option<u16> {
    state.held_alt_tab_modifier.take()
}

/// 打开 Windows 任务切换器：Alt 保持按下，Tab 仅点按一次。
/// 不武装 ALT_CHORD_ACTIVE，确保这是系统可识别的真实 Alt+Tab。
fn begin_held_alt_tab(alt_vk: u16) {
    key_chord(&[alt_vk], false);
    key_chord(&[0x09], false);
    key_chord(&[0x09], true);
    ALT_TAB_HOLD_ACTIVE.store(true, Ordering::Release);
    let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
    log::info!("XIAOMI MAPPING Alt+Tab hold down alt_vk=0x{alt_vk:02X}");
}

fn release_held_alt_tab(alt_vk: u16, reason: &str) {
    key_chord(&[alt_vk], true);
    ALT_TAB_HOLD_ACTIVE.store(false, Ordering::Release);
    log::info!("XIAOMI MAPPING Alt+Tab hold release alt_vk=0x{alt_vk:02X} reason={reason}");
}

/// LL 钩子或 VK 轮询兜底调用：Alt+Tab 会话打开时转发四个方向键。
pub fn relay_alt_tab_navigation(vk: u16, key_up: bool) {
    if ALT_TAB_HOLD_ACTIVE.load(Ordering::Acquire) && matches!(vk, 0x25..=0x28) {
        key_chord(&[vk], key_up);
    }
}

pub fn alt_tab_hold_active() -> bool {
    ALT_TAB_HOLD_ACTIVE.load(Ordering::Acquire)
}

fn action_virtual_keys(action: &KeyAction) -> Option<Vec<u16>> {
    match action {
        KeyAction::SingleKey(vk) => Some(vec![*vk]),
        KeyAction::ComboKey(vks) if !vks.is_empty() => Some(vks.clone()),
        _ => None,
    }
}

fn release_held_keys(vks: &[u16], reason: &str) {
    let mut released = false;
    for _ in 0..3 {
        if key_chord(vks, true) {
            released = true;
            break;
        }
    }
    if released {
        log::info!("XIAOMI MAPPING held keys release keys={vks:?} reason={reason}");
    } else {
        log::error!("XIAOMI MAPPING held keys release failed keys={vks:?} reason={reason}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackInjectionRoute {
    SendInput,
    SystemAltTabSendInput,
    AltWindowMessage,
}

/// Select the injection route after WinUHid is unavailable or declines the chord.
///
/// Alt+Tab must remain a real system chord so Windows can open and navigate the
/// task switcher. Other Alt chords intentionally use window messages to avoid
/// activating the system menu or registered global hotkeys.
fn fallback_injection_route(vks: &[u16]) -> FallbackInjectionRoute {
    if is_system_alt_tab_chord(vks) {
        FallbackInjectionRoute::SystemAltTabSendInput
    } else if has_alt_modifier(vks) {
        FallbackInjectionRoute::AltWindowMessage
    } else {
        FallbackInjectionRoute::SendInput
    }
}

fn should_try_virtual_hid(vks: &[u16]) -> bool {
    let bypass_virtual_hid = has_alt_modifier(vks)
        // Enter 与四向键的物理原键会被 special_keys 在映射窗口内抑制。
        // 这五个键必须走带 EXTRA_INFO 的 SendInput，才能让钩子放行本应用注入。
        || (vks.len() == 1
            && matches!(vks[0], 0x0D | 0x20 | 0x25..=0x28 | 0xAD | 0xAE | 0xAF));
    !bypass_virtual_hid
}

fn inject_chord_via_send_input(vks: &[u16], hold_ms: u64) -> bool {
    let down = key_chord(vks, false);
    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
    let mut up = false;
    for _ in 0..3 {
        if key_chord(vks, true) {
            up = true;
            break;
        }
    }
    if !up {
        log::error!("XIAOMI MAPPING SendInput chord release exhausted retries vks={vks:?}");
    }
    down && up
}

pub fn tap_vks(vks: &[u16], hold_ms: u64) {
    // 音量/静音与 Space：优先走 SendInput。
    // 某些全屏 Web 播放器会把虚拟 HID 的 Space 误判为播放器菜单触发，
    // SendInput 可保持标准的空格键语义。
    // 所有 Alt 组合都必须跳过 WinUHid，避免成功后提前返回并绕过下方分流：
    // Alt+Tab 走未武装拦截标记的系统 SendInput，其余 Alt 组合走窗口消息。
    let try_virtual_hid = should_try_virtual_hid(vks);
    if try_virtual_hid {
        if crate::bridges::xiaomi::hid_injector::press(vks).is_ok() {
            std::thread::sleep(Duration::from_millis(hold_ms.clamp(20, 1000)));
            let mut released = false;
            for _ in 0..3 {
                if crate::bridges::xiaomi::hid_injector::release_ready(vks).is_ok() {
                    released = true;
                    break;
                }
            }
            if !released {
                // Do not send a second DOWN through SendInput.  First destroy
                // the stale virtual device and issue KEYUP only for the chord
                // this call owns.
                crate::bridges::xiaomi::hid_injector::reset_and_retry();
                let _ = key_chord(vks, true);
                log::error!("XIAOMI MAPPING virtual chord release recovery vks={vks:?}");
            }
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }

    tap_vks_fallback(vks, hold_ms);
}

fn tap_vks_fallback(vks: &[u16], hold_ms: u64) {
    match fallback_injection_route(vks) {
        FallbackInjectionRoute::SystemAltTabSendInput => {
            // Do not arm ALT_CHORD_ACTIVE here. Alt+Tab must reach Windows as a
            // genuine system chord rather than being swallowed by our LL hook.
            inject_chord_via_send_input(vks, hold_ms);
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
            log::debug!(
                "XIAOMI MAPPING inject system Alt+Tab via SendInput vks={vks:?} hold_ms={hold_ms}"
            );
        }
        FallbackInjectionRoute::AltWindowMessage => {
            // Alt 组合键（如 Alt+Space, Alt+S）：使用 SendMessage(WM_KEYDOWN) 注入，
            // 避免 SendInput 触发 WM_SYSKEYDOWN → 系统菜单/全局热键
            inject_alt_chord_via_message(vks, hold_ms);
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
        }
        FallbackInjectionRoute::SendInput => {
            inject_chord_via_send_input(vks, hold_ms);
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
            log::debug!("XIAOMI MAPPING inject SendInput vks={vks:?} hold_ms={hold_ms}");
        }
    }
}

/// 通过 SendMessage(WM_KEYDOWN/WM_KEYUP) 注入 Alt 组合键。
///
/// 与 SendInput 不同，SendMessage 投递的是 WM_KEYDOWN（非 WM_SYSKEYDOWN），
/// Windows 不会将其解释为系统键，因此 Alt+Space 不会弹出系统菜单、
/// Alt+S 不会触发全局热键。
#[cfg(target_os = "windows")]
fn inject_alt_chord_via_message(vks: &[u16], hold_ms: u64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, SendMessageTimeoutW, SMTO_NORMAL, WM_KEYDOWN, WM_KEYUP,
    };
    use windows::Win32::Foundation::HWND;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND(std::ptr::null_mut()) {
        // 无前台窗口，回退 SendInput。这里不能武装 ALT_CHORD_ACTIVE，
        // 否则低级键盘钩子会吞掉我们自己带 EXTRA_INFO 的回退事件。
        log::warn!("XIAOMI MAPPING alt_chord: no foreground window, fallback SendInput");
        inject_chord_via_send_input(vks, hold_ms);
        return;
    }

    // 武装特殊键钩子：若回调仍触发则抑制（双保险）
    crate::bridges::xiaomi::special_keys::arm_alt_chord();

    // 按下：正序发送 WM_KEYDOWN
    for &vk in vks {
        let lparam = make_key_lparam(vk, false);
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_KEYDOWN,
                windows::Win32::Foundation::WPARAM(vk as usize),
                windows::Win32::Foundation::LPARAM(lparam as isize),
                SMTO_NORMAL,
                500,
                None,
            );
        }
    }

    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));

    // 释放：逆序发送 WM_KEYUP
    for &vk in vks.iter().rev() {
        let lparam = make_key_lparam(vk, true);
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_KEYUP,
                windows::Win32::Foundation::WPARAM(vk as usize),
                windows::Win32::Foundation::LPARAM(lparam as isize),
                SMTO_NORMAL,
                500,
                None,
            );
        }
    }

    crate::bridges::xiaomi::special_keys::disarm_alt_chord();
    log::debug!(
        "XIAOMI MAPPING inject alt_chord via SendMessage vks={vks:?} hold_ms={hold_ms}"
    );
}

/// 构造 WM_KEYDOWN/WM_KEYUP 的 lParam
#[cfg(target_os = "windows")]
fn make_key_lparam(vk: u16, key_up: bool) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};

    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u32;
    let mut lparam: u32 = (scan & 0xFF) << 16;

    // bit 24: extended key flag
    if is_extended(vk) {
        lparam |= 1 << 24;
    }

    if key_up {
        // bit 30: previous key state (was down)
        // bit 31: transition state (being released)
        lparam |= (1 << 30) | (1 << 31);
    }

    // repeat count = 1 (bits 0-15 保持 1)
    lparam |= 1;

    lparam
}

#[cfg(not(target_os = "windows"))]
fn inject_alt_chord_via_message(vks: &[u16], hold_ms: u64) {
    // 非 Windows 回退
    key_chord(vks, false);
    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
    key_chord(vks, true);
}

fn tap_unicode_text(text: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE, VIRTUAL_KEY,
        };
        for ch in text.encode_utf16() {
            let inputs = [
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: ch,
                            dwFlags: KEYEVENTF_UNICODE,
                            time: 0,
                            dwExtraInfo: EXTRA_INFO,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: ch,
                            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: EXTRA_INFO,
                        },
                    },
                },
            ];
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
    }
}

fn key_chord(vks: &[u16], key_up: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
            KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
        };

        let iter: Box<dyn Iterator<Item = &u16>> = if key_up {
            Box::new(vks.iter().rev())
        } else {
            Box::new(vks.iter())
        };

        let mut inputs: Vec<INPUT> = Vec::with_capacity(vks.len());
        for &vk in iter {
            let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            let mut flags = if is_extended(vk) {
                KEYEVENTF_EXTENDEDKEY
            } else {
                Default::default()
            };
            if key_up {
                flags |= KEYEVENTF_KEYUP;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: EXTRA_INFO,
                    },
                },
            });
        }
        if inputs.is_empty() {
            return true;
        }
        // Release modifiers one by one.  A partial SendInput batch used to
        // leave LWin/RWin behind while reporting only a generic failure.  With
        // individual KEYUP records every owned key receives its own retry.
        if key_up {
            let mut released = true;
            for (index, input) in inputs.iter().enumerate() {
                let sent = unsafe {
                    SendInput(std::slice::from_ref(input), std::mem::size_of::<INPUT>() as i32)
                } as usize;
                if sent != 1 {
                    released = false;
                    let vk = vks[vks.len() - 1 - index];
                    log::warn!(
                        "XIAOMI MAPPING SendInput KEYUP failed vk=0x{vk:02X} vks={vks:?}"
                    );
                }
            }
            if released {
                record_send_input_result(true, String::new());
            } else {
                record_send_input_result(false, format!("SendInput KEYUP incomplete vks={vks:?}"));
            }
            return released;
        }

        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
        if sent != inputs.len() {
            let detail = format!(
                "SendInput incomplete sent={sent} expected={} key_up={key_up} vks={vks:?}",
                inputs.len()
            );
            record_send_input_result(false, detail.clone());
            log::warn!(
                "XIAOMI MAPPING {detail}"
            );
            return false;
        }
        record_send_input_result(true, String::new());
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, key_up);
        record_send_input_result(true, String::new());
        true
    }
}

fn send_input_complete(sent: u32, expected: usize) -> bool {
    sent == expected as u32
}

fn mouse_move_speed(step: u32, frame: u32, accelerate: bool) -> u32 {
    if accelerate {
        (step + frame / 10).min(step.saturating_mul(4))
    } else {
        step
    }
}

/// 模拟鼠标左键点击（在当前鼠标位置按下并抬起）。
fn mouse_left_click() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_LEFTDOWN,
            MOUSEEVENTF_LEFTUP,
        };

        let down = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTDOWN,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTUP,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        };
        let inputs = [down, up];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if send_input_complete(sent, inputs.len()) {
            log::debug!("XIAOMI MAPPING mouse left click");
            true
        } else {
            log::warn!(
                "XIAOMI MAPPING mouse left click SendInput incomplete sent={sent} expected={} error={}",
                inputs.len(),
                std::io::Error::last_os_error()
            );
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        log::warn!("XIAOMI MAPPING mouse click not supported on this platform");
        false
    }
}

/// 模拟鼠标相对移动（dx/dy 为像素偏移）。
fn mouse_move_relative(dx: i32, dy: i32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
        };
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        };
        let inputs = [input];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if send_input_complete(sent, inputs.len()) {
            log::debug!("XIAOMI MAPPING mouse move dx={dx} dy={dy}");
            true
        } else {
            log::warn!(
                "XIAOMI MAPPING mouse move SendInput incomplete dx={dx} dy={dy} sent={sent} expected={} error={}",
                inputs.len(),
                std::io::Error::last_os_error()
            );
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (dx, dy);
        log::warn!("XIAOMI MAPPING mouse move not supported on this platform");
        false
    }
}

/// 长按鼠标移动循环：generation 必须已在有效的物理按压期间预留。
fn start_mouse_move_loop(
    button_id: String,
    dx: i32,
    dy: i32,
    step: u32,
    accelerate: bool,
    generation: u64,
) {
    std::thread::Builder::new()
        .name(format!("xiaomi-mouse-move-{button_id}"))
        .spawn(move || {
            let mut frame: u32 = 0;
            loop {
                if !repeat_is_active(&button_id, generation) {
                    break;
                }
                // 加速：从 step 开始，每 10 帧增加 1px，上限 step*4。
                let speed = mouse_move_speed(step, frame, accelerate);
                if !mouse_move_relative(dx * speed as i32, dy * speed as i32) {
                    break;
                }
                frame += 1;
                std::thread::sleep(Duration::from_millis(16)); // ~60fps
            }
        })
        .ok();
}

#[cfg(test)]
mod gesture_tests {
    use super::*;

    #[test]
    fn long_press_uses_existing_button_thresholds() {
        assert_eq!(hold_delay("volume_up"), Duration::from_millis(400));
        assert_eq!(hold_delay("volume_down"), Duration::from_millis(400));
        assert_eq!(hold_delay("menu"), Duration::from_millis(280));
        assert_eq!(hold_delay("up"), Duration::from_millis(280));
    }

    #[test]
    fn mouse_send_input_reports_partial_delivery_as_failure() {
        assert!(send_input_complete(2, 2));
        assert!(!send_input_complete(1, 2));
        assert!(!send_input_complete(0, 1));
    }

    #[test]
    fn mouse_move_acceleration_is_bounded() {
        assert_eq!(mouse_move_speed(20, 0, true), 20);
        assert_eq!(mouse_move_speed(20, 10, true), 21);
        assert_eq!(mouse_move_speed(20, 1_000, true), 80);
        assert_eq!(mouse_move_speed(20, 1_000, false), 20);
    }

    #[test]
    fn repeat_token_cannot_be_armed_after_the_press_was_released() {
        let button_id = "mouse-repeat-race-test";
        let press_gen = 73;
        {
            let mut states = press_states();
            states.as_mut().unwrap().insert(
                button_id.into(),
                PressState {
                    gen: press_gen,
                    active: true,
                    ..Default::default()
                },
            );
        }

        let repeat_gen = reserve_repeat_for_active_press(button_id, press_gen).unwrap();
        assert!(repeat_is_active(button_id, repeat_gen));

        {
            let mut states = press_states();
            let state = states.as_mut().unwrap().get_mut(button_id).unwrap();
            state.gen = state.gen.wrapping_add(1);
            state.active = false;
        }
        cancel_repeat(button_id);

        assert!(!repeat_is_active(button_id, repeat_gen));
        assert_eq!(reserve_repeat_for_active_press(button_id, press_gen), None);
    }

    #[test]
    fn long_press_lookup_accepts_historical_aliases() {
        let mut config = DeviceConfig::new();
        config
            .long_press_bindings
            .insert("dpad_up".into(), KeyAction::SingleKey(0x26));

        assert!(has_long_press(&config, "up"));
        assert_eq!(
            lookup_long_action(&config, "dpad_up"),
            Some(&KeyAction::SingleKey(0x26))
        );
    }

    #[test]
    fn mapped_menu_arms_native_suppression_before_gesture_settlement() {
        let mut config = DeviceConfig::new();
        config
            .button_bindings
            .insert("menu".into(), KeyAction::SingleKey(0x20));
        config
            .multi_click_bindings
            .entry("menu".into())
            .or_default()
            .insert(2, KeyAction::SingleKey(0x41));

        assert!(should_arm_native_suppression(&config, "menu"));

        config.button_bindings.clear();
        config.multi_click_bindings.clear();
        assert!(!should_arm_native_suppression(&config, "menu"));
    }

    #[test]
    fn independent_long_press_also_uses_deferred_gesture_path() {
        let mut config = DeviceConfig::new();
        config
            .long_press_bindings
            .insert("ok".into(), KeyAction::SingleKey(0x0D));

        assert!(!has_multi_click(&config, "ok"));
        assert!(has_long_press(&config, "ok"));
        assert!(should_arm_native_suppression(&config, "ok"));
    }

    #[test]
    fn plain_alt_tab_long_press_accepts_only_one_alt_and_one_tab() {
        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0x12, 0x09])),
            Some(0x12)
        );
        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0x09, 0xA4])),
            Some(0xA4)
        );
        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0xA5, 0x09])),
            Some(0xA5)
        );

        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0x11, 0xA4, 0x09])),
            None
        );
        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0xA4, 0x10, 0x09])),
            None
        );
        assert_eq!(
            plain_alt_tab_modifier(&KeyAction::ComboKey(vec![0xA4, 0x20])),
            None
        );
        assert_eq!(plain_alt_tab_modifier(&KeyAction::SingleKey(0x09)), None);
    }

    #[test]
    fn held_alt_tab_modifier_is_taken_only_once() {
        let mut state = PressState {
            held_alt_tab_modifier: Some(0xA4),
            ..Default::default()
        };

        assert_eq!(take_held_alt_tab_modifier(&mut state), Some(0xA4));
        assert_eq!(take_held_alt_tab_modifier(&mut state), None);
    }

    #[test]
    fn classifies_all_alt_tab_variants_as_system_chords() {
        assert!(is_system_alt_tab_chord(&[0x12, 0x09]));
        assert!(is_system_alt_tab_chord(&[0xA4, 0x09]));
        assert!(is_system_alt_tab_chord(&[0xA5, 0x09]));
        assert!(is_system_alt_tab_chord(&[0xA4, 0x10, 0x09]));
        assert!(is_system_alt_tab_chord(&[0x11, 0xA5, 0x09]));
        assert!(is_system_alt_tab_chord(&[0x09, 0xA4]));

        assert!(!is_system_alt_tab_chord(&[0x09]));
        assert!(!is_system_alt_tab_chord(&[0x11, 0x09]));
        assert!(!is_system_alt_tab_chord(&[0xA5, 0x20]));
    }

    #[test]
    fn alt_tab_uses_unarmed_send_input_route() {
        assert_eq!(
            fallback_injection_route(&[0x12, 0x09]),
            FallbackInjectionRoute::SystemAltTabSendInput
        );
        assert_eq!(
            fallback_injection_route(&[0xA4, 0x10, 0x09]),
            FallbackInjectionRoute::SystemAltTabSendInput
        );
        assert_eq!(
            fallback_injection_route(&[0x11, 0xA5, 0x09]),
            FallbackInjectionRoute::SystemAltTabSendInput
        );
    }

    #[test]
    fn alt_chords_never_attempt_virtual_hid_first() {
        assert!(!should_try_virtual_hid(&[0x12, 0x09]));
        assert!(!should_try_virtual_hid(&[0xA4, 0x10, 0x09]));
        assert!(!should_try_virtual_hid(&[0x11, 0xA5, 0x09]));

        assert!(!should_try_virtual_hid(&[0xA5, 0x20]));
        assert!(!should_try_virtual_hid(&[0xA5, 0x53]));
        assert!(should_try_virtual_hid(&[0x11, 0x09]));
    }

    #[test]
    fn enter_and_directions_use_marked_send_input_instead_of_virtual_hid() {
        for vk in [0x0D, 0x25, 0x26, 0x27, 0x28] {
            assert!(!should_try_virtual_hid(&[vk]), "vk=0x{vk:02X}");
            assert_eq!(fallback_injection_route(&[vk]), FallbackInjectionRoute::SendInput);
        }
        assert!(should_try_virtual_hid(&[0x41]));
    }

    #[test]
    fn voice_shortcuts_try_virtual_hid_for_both_ime_presets() {
        // Doubao/Qianwen: right Alt; WeChat: legacy Ctrl+Win and current Ctrl+Shift+D.
        assert!(should_try_virtual_hid_for_voice(&[0xA5]));
        assert!(should_try_virtual_hid_for_voice(&[0xA2, 0x5B]));
        assert!(should_try_virtual_hid_for_voice(&[0xA2, 0xA0, 0x44]));
        assert!(!should_try_virtual_hid_for_voice(&[]));
    }

    #[test]
    fn voice_only_win_or_alt_chords_are_neutralized_before_release() {
        assert!(voice_needs_shell_neutralizer(&[0xA5]));
        assert!(voice_needs_shell_neutralizer(&[0xA2, 0x5B]));
        assert!(voice_needs_shell_neutralizer(&[0x5C]));
        assert!(!voice_needs_shell_neutralizer(&[0xA2, 0xA0, 0x44]));
        assert!(!voice_needs_shell_neutralizer(&[0x5B, 0x44]));
    }

    #[test]
    fn wechat_start_voice_ctrl_win_uses_the_separate_tap_session_policy() {
        assert!(is_wechat_start_voice_shortcut(
            &[0xA2, 0x5B],
            Some(VoiceInputProfile::Wechat)
        ));
        assert!(is_wechat_start_voice_shortcut(
            &[0x5B, 0xA2],
            Some(VoiceInputProfile::Wechat)
        ));
        assert!(!is_wechat_start_voice_shortcut(
            &[0xA2, 0x5B],
            Some(VoiceInputProfile::WechatHold)
        ));
        assert!(!is_wechat_start_voice_shortcut(
            &[0xA2, 0xA0, 0x44],
            Some(VoiceInputProfile::Wechat)
        ));
        assert!(!is_wechat_start_voice_shortcut(
            &[0xA5],
            Some(VoiceInputProfile::Wechat)
        ));
        assert_eq!(
            wechat_start_voice_tap_steps(&[0xA2, 0x5B]),
            vec![
                VoiceReleaseStep::Press(vec![0xA2, 0x5B]),
                VoiceReleaseStep::Release(vec![0xA2, 0x5B]),
            ]
        );
    }

    #[test]
    fn force_release_keeps_wechat_start_voice_session_when_stop_tap_fails() {
        // 测试进程中 WinUHid 设备从未预热，VirtualHid 路由的停止点按会立即
        // 失败且不产生真实按键；失败时会话必须保留以便稍后重试，且函数必须
        // 正常返回（锁守卫不得遗留持有可能导致后续 lock() 挂起）。
        *VOICE_WECHAT_TAP_SESSION.lock() = Some(VoiceTapSession {
            keys: vec![0xA2, 0x5B],
            route: VoiceInjectionRoute::VirtualHid,
            profile: Some(VoiceInputProfile::Wechat),
        });
        let released = force_release_voice_shortcut("test");
        assert!(!released);
        assert!(VOICE_WECHAT_TAP_SESSION.lock().is_some());
        *VOICE_WECHAT_TAP_SESSION.lock() = None;
    }

    #[test]
    fn virtual_hid_voice_release_keeps_f24_held_while_modifiers_lift() {
        let ctrl_win = vec![0xA2, 0x5B];
        assert_eq!(
            neutralized_voice_release_steps(&ctrl_win, VoiceInjectionRoute::VirtualHid),
            vec![
                VoiceReleaseStep::Press(vec![0xA2, 0x5B, 0x87]),
                VoiceReleaseStep::Press(vec![0x87]),
                VoiceReleaseStep::Release(vec![0x87]),
            ]
        );
    }

    #[test]
    fn send_input_voice_release_lifts_chord_between_f24_down_and_up() {
        let ctrl_win = vec![0xA2, 0x5B];
        assert_eq!(
            neutralized_voice_release_steps(&ctrl_win, VoiceInjectionRoute::SendInputFallback),
            vec![
                VoiceReleaseStep::Press(vec![0x87]),
                VoiceReleaseStep::Release(vec![0xA2, 0x5B]),
                VoiceReleaseStep::Release(vec![0x87]),
            ]
        );
    }

    #[test]
    fn right_alt_uses_the_same_neutralized_release_shape() {
        assert_eq!(
            neutralized_voice_release_steps(&[0xA5], VoiceInjectionRoute::VirtualHid),
            vec![
                VoiceReleaseStep::Press(vec![0xA5, 0x87]),
                VoiceReleaseStep::Press(vec![0x87]),
                VoiceReleaseStep::Release(vec![0x87]),
            ]
        );
        assert_eq!(
            neutralized_voice_release_steps(&[0xA5], VoiceInjectionRoute::SendInputFallback),
            vec![
                VoiceReleaseStep::Press(vec![0x87]),
                VoiceReleaseStep::Release(vec![0xA5]),
                VoiceReleaseStep::Release(vec![0x87]),
            ]
        );
    }

    #[test]
    fn qianwen_profile_releases_right_alt_directly_without_f24() {
        assert_eq!(
            voice_release_steps(
                &[0xA5],
                VoiceInjectionRoute::VirtualHid,
                Some(VoiceInputProfile::Qianwen),
            ),
            vec![VoiceReleaseStep::Release(vec![0xA5])]
        );
        assert_eq!(
            voice_release_steps(
                &[0xA5],
                VoiceInjectionRoute::VirtualHid,
                Some(VoiceInputProfile::DoubaoHold),
            ),
            neutralized_voice_release_steps(&[0xA5], VoiceInjectionRoute::VirtualHid)
        );
    }

    #[test]
    fn firmware_modifier_prefilter_is_short_lived_and_release_aware() {
        let mut filter = FirmwareVoiceModifierFilter::empty();
        filter.arm(false);
        assert!(filter.should_swallow(0xA2, true, false));
        assert!(!filter.should_swallow(0xA0, true, false));
        filter.note_f5_suppressed();
        assert!(!filter.should_swallow(0x5B, true, false));
        assert!(filter.should_swallow(0xA2, false, false));

        filter.arm(false);
        assert!(filter.should_swallow(0x5B, true, false));
        assert!(!filter.should_swallow(0x5B, false, true));
        filter.clear();
        assert!(!filter.should_swallow(0xA2, false, false));
    }

    #[test]
    fn chord_guard_only_blocks_matching_modifier_keyups_outside_release_window() {
        assert!(voice_chord_modifier_up_guarded(&[0xA2, 0xA0], 0xA2, false));
        assert!(!voice_chord_modifier_up_guarded(&[0xA2, 0xA0], 0xA2, true));
        assert!(!voice_chord_modifier_up_guarded(&[0xA2, 0xA0], 0x5B, false));
        assert!(!voice_chord_modifier_up_guarded(&[], 0xA2, false));
    }

    #[test]
    fn ordinary_wechat_chord_skips_shell_neutralization() {
        assert!(!voice_needs_shell_neutralizer(&[0xA2, 0xA0, 0x44]));
    }

    #[test]
    fn non_tab_alt_chords_keep_window_message_route() {
        assert_eq!(
            fallback_injection_route(&[0xA5, 0x20]),
            FallbackInjectionRoute::AltWindowMessage
        );
        assert_eq!(
            fallback_injection_route(&[0xA5, 0x53]),
            FallbackInjectionRoute::AltWindowMessage
        );
        assert_eq!(
            fallback_injection_route(&[0x12, 0x73]),
            FallbackInjectionRoute::AltWindowMessage
        );
        assert_eq!(
            fallback_injection_route(&[0x11, 0x09]),
            FallbackInjectionRoute::SendInput
        );
    }

    #[test]
    fn serialized_voice_hotkey_names_round_trip() {
        let vks = vec![0xA2, 0x08, 0x74, 0x25];
        let names = vks_to_hotkey_names(&vks);
        assert_eq!(names, vec!["leftctrl", "backspace", "f5", "vk_25"]);
        assert_eq!(
            names.iter().filter_map(|name| name_to_vk(name)).collect::<Vec<_>>(),
            vks
        );
    }

    #[test]
    fn extended_keys_round_trip() {
        let vks = vec![0x60, 0x69, 0x6A, 0x6B, 0x6D, 0x6E, 0x6F, 0x13, 0x14, 0x2C, 0x5D, 0x90, 0x91, 0x7C, 0x87, 0xBA, 0xDE];
        let names = vks_to_hotkey_names(&vks);
        assert_eq!(
            names,
            vec![
                "numpad0", "numpad9", "numpadmult", "numpadadd", "numpadsubtract",
                "numpaddecimal", "numpaddivide", "pause", "capslock", "printscreen",
                "menu", "numlock", "scrolllock", "f13", "f24", "semicolon", "apostrophe",
            ]
        );
        assert_eq!(
            names.iter().filter_map(|name| name_to_vk(name)).collect::<Vec<_>>(),
            vks
        );
    }

    #[test]
    fn voice_aliases_are_always_dedicated_shortcut_buttons() {
        assert!(is_dedicated_voice_button("mic"));
        assert!(is_dedicated_voice_button("voice"));
        assert!(!is_dedicated_voice_button("menu"));
    }
}
