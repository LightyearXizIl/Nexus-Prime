//! 快捷键捕获
//!
//! - 吞键 + 识别：都走常驻 `special_keys` WH_KEYBOARD_LL
//!   （`try_swallow_capture_key` 内用 vk/wParam 驱动 CaptureEngine，再 return 1）
//! - 禁止靠 GetAsyncKeyState 识别：键被 LL 吞掉后异步状态常不更新 → 永远录不上
//!
//! 完成规则：全部参与按键抬起后，提交完整组合。
//!
//! 不能在主键按下时立即提交：WebView 与低级钩子的事件先后并不稳定，
//! 这会把 Ctrl+Shift+D 之类的三键组合截断为只有两个修饰键。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;

fn is_modifier(vk: u32) -> bool {
    matches!(
        vk,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LWIN
            | VK_RWIN
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

fn normalize_chord(keys: &[u32]) -> Vec<u32> {
    let set: HashSet<u32> = keys.iter().copied().collect();
    let mut out = Vec::new();

    let ctrl = if set.contains(&VK_LCONTROL) {
        Some(VK_LCONTROL)
    } else if set.contains(&VK_RCONTROL) {
        Some(VK_RCONTROL)
    } else if set.contains(&VK_CONTROL) {
        Some(VK_LCONTROL)
    } else {
        None
    };
    let shift = if set.contains(&VK_LSHIFT) {
        Some(VK_LSHIFT)
    } else if set.contains(&VK_RSHIFT) {
        Some(VK_RSHIFT)
    } else if set.contains(&VK_SHIFT) {
        Some(VK_LSHIFT)
    } else {
        None
    };
    let alt = if set.contains(&VK_LMENU) {
        Some(VK_LMENU)
    } else if set.contains(&VK_RMENU) {
        Some(VK_RMENU)
    } else if set.contains(&VK_MENU) {
        Some(VK_LMENU)
    } else {
        None
    };
    let win = if set.contains(&VK_LWIN) {
        Some(VK_LWIN)
    } else if set.contains(&VK_RWIN) {
        Some(VK_RWIN)
    } else {
        None
    };

    for m in [ctrl, shift, alt, win].into_iter().flatten() {
        out.push(m);
    }
    let mut mains: Vec<u32> = set
        .into_iter()
        .filter(|vk| !is_modifier(*vk))
        .collect();
    mains.sort();
    out.extend(mains);
    out
}

pub fn vk_to_label(vk: u32) -> String {
    match vk {
        VK_SHIFT | VK_LSHIFT => "左 Shift".into(),
        VK_RSHIFT => "右 Shift".into(),
        VK_CONTROL | VK_LCONTROL => "左 Ctrl".into(),
        VK_RCONTROL => "右 Ctrl".into(),
        VK_MENU | VK_LMENU => "左 Alt".into(),
        VK_RMENU => "右 Alt".into(),
        VK_LWIN => "左 Win".into(),
        VK_RWIN => "右 Win".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x13 => "Pause".into(),
        0x14 => "CapsLock".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x2C => "PrtSc".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x5D => "Menu".into(),
        0x90 => "NumLock".into(),
        0x91 => "ScrLk".into(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x6A => "Num*".into(),
        0x6B => "Num+".into(),
        0x6D => "Num-".into(),
        0x6E => "Num.".into(),
        0x6F => "Num/".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        0xAD => "Mute".into(),
        0xAE => "Vol-".into(),
        0xAF => "Vol+".into(),
        0xA6 => "Browser back".into(),
        0xA7 => "Browser forward".into(),
        0xA8 => "Browser refresh".into(),
        0xA9 => "Browser stop".into(),
        0xAA => "Browser search".into(),
        0xAB => "Browser favorites".into(),
        0xAC => "Browser home".into(),
        0xB0 => "Next track".into(),
        0xB1 => "Previous track".into(),
        0xB2 => "Media stop".into(),
        0xB3 => "Play/Pause".into(),
        0xB4 => "Mail".into(),
        0xB5 => "Media player".into(),
        0xB6 => "App 1".into(),
        0xB7 => "App 2".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 => format!("{}", vk - 0x30),
        0x41..=0x5A => ((vk as u8) as char).to_string(),
        _ => format!("VK_0x{vk:02X}"),
    }
}

struct CaptureEngine {
    prev: HashMap<u32, bool>,
    active_keys: HashSet<u32>,
    chord_history: HashSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CaptureStep {
    Progress(Vec<u32>),
    Captured(Vec<u32>),
}

impl CaptureEngine {
    fn new(initial_down: HashMap<u32, bool>) -> Self {
        let mut active_keys = HashSet::new();
        let mut chord_history = HashSet::new();
        for (&vk, &down) in &initial_down {
            if down {
                active_keys.insert(vk);
                chord_history.insert(vk);
            }
        }
        Self {
            prev: initial_down,
            active_keys,
            chord_history,
        }
    }

    fn progress_keys(&self) -> Vec<u32> {
        normalize_chord(&self.active_keys.iter().copied().collect::<Vec<_>>())
    }

    /// 钩子路径：按单个按键事件增量更新（勿依赖 GetAsyncKeyState）
    fn on_event(&mut self, vk: u32, is_down: bool) -> CaptureStep {
        let mut frame = self.prev.clone();
        frame.insert(vk, is_down);
        self.step(&frame)
    }

    fn step(&mut self, down: &HashMap<u32, bool>) -> CaptureStep {
        let mut saw_release = false;

        for (&vk, &is_down) in down {
            let was = self.prev.get(&vk).copied().unwrap_or(false);
            if is_down && !was {
                self.chord_history.insert(vk);
                self.active_keys.insert(vk);
            } else if !is_down && was {
                self.active_keys.remove(&vk);
                saw_release = true;
            }
            self.prev.insert(vk, is_down);
        }

        if saw_release && self.active_keys.is_empty() && !self.chord_history.is_empty() {
            let hist = normalize_chord(&self.chord_history.iter().copied().collect::<Vec<_>>());
            return CaptureStep::Captured(hist);
        }
        CaptureStep::Progress(self.progress_keys())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCapturedPayload {
    pub keys: Vec<u32>,
    pub labels: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCaptureProgress {
    pub labels: Vec<String>,
}

struct CaptureRuntime {
    stop: AtomicBool,
    capturing: AtomicBool,
    pending: Mutex<Option<ShortcutCapturedPayload>>,
    progress: Mutex<Vec<String>>,
    app: Mutex<Option<AppHandle>>,
}

impl CaptureRuntime {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(true),
            capturing: AtomicBool::new(false),
            pending: Mutex::new(None),
            progress: Mutex::new(Vec::new()),
            app: Mutex::new(None),
        }
    }

    fn publish_progress(&self, labels: Vec<String>) {
        *self.progress.lock().unwrap() = labels.clone();
        let app = self.app.lock().unwrap().clone();
        if let Some(app) = app {
            // 勿在 LL 回调线程同步 emit
            thread::spawn(move || {
                let _ = app.emit(
                    "shortcut-capture-progress",
                    ShortcutCaptureProgress { labels },
                );
            });
        }
    }

    fn publish_result(&self, keys: Vec<u32>) {
        let keys = normalize_chord(&keys);
        if keys.is_empty() {
            return;
        }
        let labels: Vec<String> = keys.iter().copied().map(vk_to_label).collect();
        log::info!("Shortcut captured: {}", labels.join("+"));
        let payload = ShortcutCapturedPayload {
            keys,
            labels: labels.clone(),
        };
        *self.pending.lock().unwrap() = Some(payload.clone());
        self.capturing.store(false, Ordering::SeqCst);
        // 对齐 Python：提交后继续吞键，直到 blocked_vks 全部 KEYUP
        mark_capture_submitted();
        let app = self.app.lock().unwrap().clone();
        if let Some(app) = app {
            thread::spawn(move || {
                let _ = app.emit("shortcut-captured", payload);
            });
        }
    }

    fn take_pending(&self) -> Option<ShortcutCapturedPayload> {
        self.pending.lock().unwrap().take()
    }
}

// ---------------------------------------------------------------------------
// 吞键 + 钩子内识别（由 special_keys 调用）
// ---------------------------------------------------------------------------

static SWALLOW_ACTIVE: AtomicBool = AtomicBool::new(false);
static CAPTURE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static SWALLOW_HIT_LOGGED: AtomicBool = AtomicBool::new(false);
static BLOCKED_VKS: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static HOOK_ENGINE: LazyLock<Mutex<Option<CaptureEngine>>> =
    LazyLock::new(|| Mutex::new(None));
static HOOK_RUNTIME: LazyLock<Mutex<Option<Arc<CaptureRuntime>>>> =
    LazyLock::new(|| Mutex::new(None));

fn reset_hook_session() {
    CAPTURE_SUBMITTED.store(false, Ordering::SeqCst);
    if let Ok(mut blocked) = BLOCKED_VKS.lock() {
        blocked.clear();
    }
    if let Ok(mut eng) = HOOK_ENGINE.lock() {
        *eng = None;
    }
    if let Ok(mut rt) = HOOK_RUNTIME.lock() {
        *rt = None;
    }
}

fn mark_capture_submitted() {
    CAPTURE_SUBMITTED.store(true, Ordering::SeqCst);
    maybe_finish_hook_after_drain();
}

fn maybe_finish_hook_after_drain() {
    if !CAPTURE_SUBMITTED.load(Ordering::SeqCst) {
        return;
    }
    let empty = BLOCKED_VKS
        .lock()
        .map(|g| g.is_empty())
        .unwrap_or(false);
    if empty {
        set_swallow_active(false);
        log::info!("Shortcut capture swallow drain complete");
    }
}

fn set_swallow_active(active: bool) {
    SWALLOW_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn is_swallow_active() -> bool {
    SWALLOW_ACTIVE.load(Ordering::SeqCst)
}

/// 由常驻 `special_keys` LL 钩子最前调用。true = 已吞掉，调用方必须 `return LRESULT(1)`。
/// 在回调内用 vk/wParam 识别和弦；**禁止** GetAsyncKeyState（吞键后状态不更新）。
pub fn try_swallow_capture_key(vk: u32, wparam: u32, is_injected: bool) -> bool {
    if is_injected || !SWALLOW_ACTIVE.load(Ordering::SeqCst) {
        return false;
    }

    const WM_KEYDOWN: u32 = 0x0100;
    const WM_KEYUP: u32 = 0x0101;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSKEYUP: u32 = 0x0105;

    let is_down = wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN;
    let is_up = wparam == WM_KEYUP || wparam == WM_SYSKEYUP;
    if !is_down && !is_up {
        return true;
    }

    // 必须记录每个物理键：丢事件会丢 Win → 只录到 Ctrl，并提前关吞键漏出 Win/语音
    track_blocked_vk(vk, is_down);
    if is_up {
        maybe_finish_hook_after_drain();
    }

    // 注意：钩子路径无宽限期。宽限期内吞键但不喂引擎，会重演「Ctrl+Win 只录到 Ctrl」：
    // 按下先于宽限期结束的键对引擎不可见，剩余键抬起时按纯修饰键提前提交。
    // 钩子是精确边沿：录入前已按住的键只会收到 KEYUP，引擎按 was=false 忽略，天然安全。
    if !CAPTURE_SUBMITTED.load(Ordering::SeqCst) {
        // 先算 step 再放锁，再 publish，避免持锁 emit / 嵌套锁丢事件
        let step = {
            let mut slot = match HOOK_ENGINE.lock() {
                Ok(s) => s,
                Err(_) => return true,
            };
            match slot.as_mut() {
                Some(eng) => Some(eng.on_event(vk, is_down)),
                None => None,
            }
        };
        if let Some(step) = step {
            let runtime = HOOK_RUNTIME.lock().ok().and_then(|g| g.clone());
            if let Some(runtime) = runtime {
                if runtime.capturing.load(Ordering::SeqCst) {
                    match step {
                        CaptureStep::Captured(keys) => {
                            runtime.publish_result(keys);
                        }
                        CaptureStep::Progress(mods) => {
                            if !mods.is_empty() {
                                let labels: Vec<String> =
                                    mods.iter().copied().map(vk_to_label).collect();
                                runtime.publish_progress(labels);
                            }
                        }
                    }
                }
            }
        }
    }

    if !SWALLOW_HIT_LOGGED.swap(true, Ordering::SeqCst) {
        log::info!("Shortcut capture swallow vk=0x{vk:02X} wp=0x{wparam:X}");
    }
    true
}

fn track_blocked_vk(vk: u32, down: bool) {
    if let Ok(mut g) = BLOCKED_VKS.lock() {
        if down {
            g.insert(vk);
        } else {
            g.remove(&vk);
        }
    }
}

pub struct ShortcutCaptureSession {
    runtime: Arc<CaptureRuntime>,
}

impl ShortcutCaptureSession {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(CaptureRuntime::new()),
        }
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.runtime.stop.store(true, Ordering::SeqCst);
        self.runtime.capturing.store(false, Ordering::SeqCst);

        set_swallow_active(false);
        reset_hook_session();
        self.runtime.progress.lock().unwrap().clear();
        Ok(())
    }

    pub fn start(&self, app: AppHandle) -> Result<(), String> {
        self.cancel()?;

        *self.runtime.app.lock().unwrap() = Some(app);
        *self.runtime.pending.lock().unwrap() = None;
        self.runtime.progress.lock().unwrap().clear();
        self.runtime.stop.store(false, Ordering::SeqCst);
        self.runtime.capturing.store(true, Ordering::SeqCst);
        reset_hook_session();

        #[cfg(target_os = "windows")]
        {
            crate::bridges::xiaomi::special_keys::ensure_hook_for_capture();
            let deadline = Instant::now() + Duration::from_millis(800);
            while !crate::bridges::xiaomi::special_keys::is_hook_armed()
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(10));
            }
            if !crate::bridges::xiaomi::special_keys::is_hook_armed() {
                return Err(
                    "键盘吞键钩子未启动：无法安全录入（系统热键会穿透）。请检查 special_keys。"
                        .into(),
                );
            }
        }

        *HOOK_ENGINE.lock().unwrap() = Some(CaptureEngine::new(HashMap::new()));
        *HOOK_RUNTIME.lock().unwrap() = Some(Arc::clone(&self.runtime));

        SWALLOW_HIT_LOGGED.store(false, Ordering::SeqCst);
        set_swallow_active(true);
        log::info!("Shortcut capture started (special_keys detect+swallow)");
        Ok(())
    }

    pub fn take_result(&self) -> Option<ShortcutCapturedPayload> {
        self.runtime.take_pending()
    }

    pub fn is_active(&self) -> bool {
        self.runtime.capturing.load(Ordering::SeqCst)
    }
}

impl Default for ShortcutCaptureSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ShortcutCaptureSession {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(downs: &[(u32, bool)]) -> HashMap<u32, bool> {
        downs.iter().copied().collect()
    }

    fn idle_engine() -> CaptureEngine {
        CaptureEngine::new(HashMap::new())
    }

    #[test]
    fn test_normalize_and_labels() {
        assert_eq!(
            normalize_chord(&[VK_CONTROL, VK_LCONTROL, 0x41]),
            vec![VK_LCONTROL, 0x41]
        );
        assert_eq!(vk_to_label(VK_LCONTROL), "左 Ctrl");
        assert_eq!(vk_to_label(VK_LWIN), "左 Win");
    }

    #[test]
    fn test_extended_key_labels() {
        assert_eq!(vk_to_label(0x13), "Pause");
        assert_eq!(vk_to_label(0x14), "CapsLock");
        assert_eq!(vk_to_label(0x2C), "PrtSc");
        assert_eq!(vk_to_label(0x2D), "Insert");
        assert_eq!(vk_to_label(0x5D), "Menu");
        assert_eq!(vk_to_label(0x90), "NumLock");
        assert_eq!(vk_to_label(0x91), "ScrLk");
        assert_eq!(vk_to_label(0x60), "Num0");
        assert_eq!(vk_to_label(0x69), "Num9");
        assert_eq!(vk_to_label(0x6A), "Num*");
        assert_eq!(vk_to_label(0x6B), "Num+");
        assert_eq!(vk_to_label(0x6D), "Num-");
        assert_eq!(vk_to_label(0x6E), "Num.");
        assert_eq!(vk_to_label(0x6F), "Num/");
        assert_eq!(vk_to_label(0x7C), "F13");
        assert_eq!(vk_to_label(0x87), "F24");
        assert_eq!(vk_to_label(0xBA), ";");
        assert_eq!(vk_to_label(0xDE), "'");
        // 本地既有标签不受影响
        assert_eq!(vk_to_label(0xAD), "Mute");
        assert_eq!(vk_to_label(0xA6), "Browser back");
        assert_eq!(vk_to_label(0x7B), "F12");
    }

    #[test]
    fn capture_single_key() {
        let mut eng = idle_engine();
        assert_eq!(eng.on_event(0x41, true), CaptureStep::Progress(vec![0x41]));
        assert_eq!(
            eng.on_event(0x41, false),
            CaptureStep::Captured(vec![0x41])
        );
    }

    #[test]
    fn capture_via_hook_on_event() {
        let mut eng = idle_engine();
        eng.on_event(VK_LMENU, true);
        eng.on_event(0x20, true);
        eng.on_event(0x20, false);
        assert_eq!(
            eng.on_event(VK_LMENU, false),
            CaptureStep::Captured(vec![VK_LMENU, 0x20])
        );
    }

    #[test]
    fn capture_ctrl_plus_a() {
        let mut eng = idle_engine();
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(VK_CONTROL, true);
        eng.on_event(0x41, true);
        eng.on_event(0x41, false);
        eng.on_event(VK_LCONTROL, false);
        assert_eq!(eng.on_event(VK_CONTROL, false), CaptureStep::Captured(vec![VK_LCONTROL, 0x41]));
    }

    #[test]
    fn capture_codex_ctrl_shift_d_is_not_committed_as_modifiers_only() {
        let mut eng = idle_engine();
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(VK_LSHIFT, true);
        eng.on_event(0x44, true);
        eng.on_event(0x44, false);
        eng.on_event(VK_LSHIFT, false);
        assert_eq!(
            eng.on_event(VK_LCONTROL, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LSHIFT, 0x44])
        );
    }

    #[test]
    fn capture_codex_ctrl_shift_d_accepts_modifier_order_variations() {
        let mut eng = idle_engine();
        eng.on_event(VK_LSHIFT, true);
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(0x44, true);
        eng.on_event(0x44, false);
        eng.on_event(VK_LCONTROL, false);
        assert_eq!(
            eng.on_event(VK_LSHIFT, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LSHIFT, 0x44])
        );
    }

    #[test]
    fn capture_four_keys_and_ignores_repeated_down_events() {
        let mut eng = idle_engine();
        for vk in [VK_LCONTROL, VK_LSHIFT, VK_LMENU, 0x44] {
            eng.on_event(vk, true);
        }
        // Typematic/repeated keydown must not create a duplicate or commit early.
        assert_eq!(eng.on_event(0x44, true), CaptureStep::Progress(vec![VK_LCONTROL, VK_LSHIFT, VK_LMENU, 0x44]));
        eng.on_event(0x44, false);
        eng.on_event(VK_LMENU, false);
        eng.on_event(VK_LSHIFT, false);
        assert_eq!(
            eng.on_event(VK_LCONTROL, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LSHIFT, VK_LMENU, 0x44])
        );
    }

    #[test]
    fn capture_ctrl_win_on_all_modifiers_released() {
        let mut eng = idle_engine();
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(VK_CONTROL, true);
        eng.on_event(VK_LWIN, true);
        // 只松左 Ctrl：通用 Ctrl 与 Win 仍按下，不应提交
        assert_eq!(
            eng.on_event(VK_LCONTROL, false),
            CaptureStep::Progress(vec![VK_LCONTROL, VK_LWIN])
        );
        assert_eq!(
            eng.on_event(VK_CONTROL, false),
            CaptureStep::Progress(vec![VK_LWIN])
        );
        assert_eq!(
            eng.on_event(VK_LWIN, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LWIN])
        );
    }

    #[test]
    fn capture_ctrl_win_does_not_commit_on_first_release() {
        let mut eng = idle_engine();
        eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true)]));
        eng.step(&frame(&[
            (VK_LCONTROL, true),
            (VK_CONTROL, true),
            (VK_LWIN, true),
        ]));
        assert_eq!(
            eng.step(&frame(&[
                (VK_LCONTROL, false),
                (VK_CONTROL, false),
                (VK_LWIN, true),
            ])),
            CaptureStep::Progress(vec![VK_LWIN])
        );
    }

    #[test]
    fn capture_single_ctrl_on_release() {
        let mut eng = idle_engine();
        eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true)]));
        assert_eq!(
            eng.step(&frame(&[(VK_LCONTROL, false), (VK_CONTROL, false)])),
            CaptureStep::Captured(vec![VK_LCONTROL])
        );
    }

    #[test]
    fn keyup_without_keydown_is_ignored() {
        // 录入开始前就按住 Win：钩子只会补到 KEYUP。若误当成修饰键抬起提交，
        // 会重演「Ctrl+Win 只录到 Ctrl / 只录到 Win」。
        let mut eng = idle_engine();
        assert_eq!(eng.on_event(VK_LWIN, false), CaptureStep::Progress(vec![]));
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(VK_LWIN, true);
        assert_eq!(
            eng.on_event(VK_LCONTROL, false),
            CaptureStep::Progress(vec![VK_LWIN])
        );
        assert_eq!(
            eng.on_event(VK_LWIN, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LWIN])
        );
    }

    #[test]
    fn capture_win_first_then_ctrl() {
        // 左 Win 先按、左 Ctrl 后按，同样提交完整组合
        let mut eng = idle_engine();
        eng.on_event(VK_LWIN, true);
        eng.on_event(VK_LCONTROL, true);
        eng.on_event(VK_LWIN, false);
        assert_eq!(
            eng.on_event(VK_LCONTROL, false),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LWIN])
        );
    }

    #[test]
    fn second_session_engine_is_independent() {
        let mut a = idle_engine();
        a.step(&frame(&[(VK_LCONTROL, true)]));
        assert_eq!(
            a.step(&frame(&[(VK_LCONTROL, false)])),
            CaptureStep::Captured(vec![VK_LCONTROL])
        );
        let mut b = idle_engine();
        b.on_event(0x41, true);
        assert_eq!(b.on_event(0x41, false), CaptureStep::Captured(vec![0x41]));
    }

    #[test]
    fn special_keys_must_consult_capture_swallow() {
        let src = include_str!("../xiaomi/special_keys.rs");
        assert!(
            src.contains("try_swallow_capture_key"),
            "special_keys LL proc must call try_swallow_capture_key so Alt/Win hotkeys are swallowed during capture"
        );
    }

    #[test]
    fn try_swallow_blocks_syskeydown_when_active() {
        set_swallow_active(false);
        assert!(!try_swallow_capture_key(0x20, 0x0104, false));
        // 无 engine/runtime 时仍应吞键
        set_swallow_active(true);
        assert!(try_swallow_capture_key(0x20, 0x0104, false));
        assert!(!try_swallow_capture_key(0x53, 0x0104, true));
        set_swallow_active(false);
    }
}
