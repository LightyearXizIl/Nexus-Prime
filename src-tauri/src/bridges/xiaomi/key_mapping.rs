//! 对齐 Python `handle_direct_hid_report` / `VoiceShortcut` / `_perform_button_action`
//!
//! 遥控器按键 → 读取 xiaomi.json 的 button_bindings / voice_hotkey → SendInput 注入

use crate::bridges::xiaomi::connect;
use crate::bridges::xiaomi::key_log::{button_label, emit_key_phase, emit_message};
use crate::bridges::xiaomi::tv_gate;
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 与 Python `EXTRA_INFO = 0x584D4952` ('XMIR') 一致，供 LL hook 放行虚拟键
pub const EXTRA_INFO: usize = 0x584D_4952;

static VOICE_HELD: AtomicBool = AtomicBool::new(false);
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
static INPUT_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static FIRMWARE_VOICE_HELD: AtomicBool = AtomicBool::new(false);
static VOICE_HOOK_APP: Mutex<Option<AppHandle>> = Mutex::new(None);

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
        cancel_pending_gestures();
    }
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
        return VOICE_F5_DOWN_SUPPRESSED.swap(false, Ordering::AcqRel);
    }
    if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) {
        return true;
    }
    if !input_session_active() {
        return false;
    }
    if voice_native_suppress_active()
        || direct_signal_recent("voice", Duration::from_millis(300))
        || direct_signal_recent("mic", Duration::from_millis(300))
    {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        return true;
    }
    if wait_for_direct_signal("mic", Duration::from_millis(80)) {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        arm_voice_native_suppress();
        return true;
    }
    // Policy B：关联不上则放行（键盘 F5 可用）。ATVV 挂掉时由 toast 提示，不再会话级全吞。
    false
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

/// 语音键仅在配置扩展槽位时进入五档手势模式；否则保留旧的点击/按住模式。
pub fn voice_uses_extended_gestures(app: &AppHandle) -> bool {
    load_xiaomi_config(app)
        .map(|config| has_multi_click(&config, "mic") || has_long_press(&config, "mic"))
        .unwrap_or(false)
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
    let is_voice = button_id == "voice" || button_id == "mic";
    if is_voice {
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

    let multi_enabled = has_multi_click(&config, button_id);
    let long_enabled = has_long_press(&config, button_id);

    // 兼容旧的“点击/按住触发”模式；一旦语音配置了双击、三击、四击或长按，
    // 则由与其它按键相同的五档手势引擎独占处理。
    if is_voice && !multi_enabled && !long_enabled {
        handle_voice(app, pressed);
        return;
    }

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
    cancel_repeat(button_id);
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
                                } else {
                                    key_chord(&keys, false);
                                    state.held_keys = Some(keys);
                                    true
                                }
                            } else {
                                perform_action(action)
                            }
                        } else {
                            perform_action(action)
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
                        start_hold_repeat_loop(app, button_id, interval);
                    }
                }
            }
        })
        .ok();
}

fn start_hold_repeat_loop(app: AppHandle, button_id: String, interval: Duration) {
    let gen = {
        let mut map = repeats();
        let e = map.as_mut().unwrap().entry(button_id.clone()).or_insert(0);
        *e = e.wrapping_add(1);
        *e
    };
    std::thread::Builder::new()
        .name(format!("xiaomi-repeat-{button_id}"))
        .spawn(move || loop {
            std::thread::sleep(interval);
            {
                let map = repeats();
                if map.as_ref().and_then(|m| m.get(&button_id)).copied() != Some(gen) {
                    break;
                }
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
    // 点击 / 按住：快捷键都跟遥控按下/抬起走（短按≈点按，长按=按住）
    // 「点击模式」的短按点按由 input_session 在短于阈值抬起时改走 tap；此处处理按下/抬起和弦
    if pressed {
        if !VOICE_HELD.swap(true, Ordering::SeqCst) {
            key_chord(&vks, false);
            log::info!(
                "XIAOMI VOICE SHORTCUT DOWN mode={:?} vks={vks:?}",
                config.trigger_mode
            );
        }
    } else if VOICE_HELD.swap(false, Ordering::SeqCst) {
        key_chord(&vks, true);
        log::info!(
            "XIAOMI VOICE SHORTCUT UP mode={:?} vks={vks:?}",
            config.trigger_mode
        );
    }
}

/// 点击模式：短按判定为「点按一次」完整 tap（若尚未因长按而 DOWN）
pub fn voice_shortcut_tap(app: &AppHandle) {
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
    // 若已经按住 DOWN，先松开再 tap，避免粘键
    if VOICE_HELD.swap(false, Ordering::SeqCst) {
        key_chord(&vks, true);
    }
    let hold = if vks.iter().any(|vk| matches!(vk, 0x5B | 0x5C)) {
        120
    } else {
        70
    };
    tap_vks(&vks, hold);
    log::info!("XIAOMI VOICE SHORTCUT TAP (click) vks={vks:?} hold_ms={hold}");
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
    if !VOICE_HELD.swap(true, Ordering::SeqCst) {
        key_chord(&vks, false);
        log::info!("XIAOMI VOICE SHORTCUT DOWN (hold-after-click-threshold) vks={vks:?}");
    }
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
            other if (0x41..=0x5A).contains(&other) => {
                ((other as u8) as char).to_ascii_lowercase().to_string()
            }
            other if (0x30..=0x39).contains(&other) => {
                char::from(b'0' + (other - 0x30) as u8).to_string()
            }
            other if (0x70..=0x7B).contains(&other) => format!("f{}", other - 0x6F),
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
}

fn name_to_vk(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase().replace(' ', "");
    match n.as_str() {
        "backspace" => Some(0x08),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
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
    key_chord(vks, true);
    log::info!("XIAOMI MAPPING held keys release keys={vks:?} reason={reason}");
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
        || (vks.len() == 1 && matches!(vks[0], 0x20 | 0xAD | 0xAE | 0xAF));
    !bypass_virtual_hid
}

fn inject_chord_via_send_input(vks: &[u16], hold_ms: u64) {
    key_chord(vks, false);
    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
    key_chord(vks, true);
}

pub fn tap_vks(vks: &[u16], hold_ms: u64) {
    // 音量/静音与 Space：优先走 SendInput。
    // 某些全屏 Web 播放器会把虚拟 HID 的 Space 误判为播放器菜单触发，
    // SendInput 可保持标准的空格键语义。
    // 所有 Alt 组合都必须跳过 WinUHid，避免成功后提前返回并绕过下方分流：
    // Alt+Tab 走未武装拦截标记的系统 SendInput，其余 Alt 组合走窗口消息。
    let try_virtual_hid = should_try_virtual_hid(vks);
    if try_virtual_hid {
        if crate::bridges::xiaomi::hid_injector::tap_vks(vks, hold_ms) {
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }

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
            log::debug!(
                "XIAOMI MAPPING inject SendInput vks={vks:?} hold_ms={hold_ms} forced={}",
                !try_virtual_hid
            );
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

fn key_chord(vks: &[u16], key_up: bool) {
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
        if !inputs.is_empty() {
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, key_up);
    }
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
        assert!(!is_system_alt_tab_chord(&[0xA4, 0x20]));
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

        assert!(!should_try_virtual_hid(&[0xA4, 0x20]));
        assert!(!should_try_virtual_hid(&[0xA5, 0x53]));
        assert!(should_try_virtual_hid(&[0x11, 0x09]));
    }

    #[test]
    fn non_tab_alt_chords_keep_window_message_route() {
        assert_eq!(
            fallback_injection_route(&[0xA4, 0x20]),
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
}
