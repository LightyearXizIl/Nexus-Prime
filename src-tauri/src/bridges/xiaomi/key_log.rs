//! 小米按键采集 — 对齐 Python 生产路径
//!
//! - HidOverGatt Frida Gadget tap → 返回键 0xF1、音量 0x80/0x81（Windows HID 独占时必需）
//! - ATVV Control → 语音键
//! - 低级键盘钩 → 抑制已由 Tap 映射的原生气，避免双触发
//!
//! 故意不做：hidapi 打开设备、默认 GATT HID 订阅（会抢占 Microsoft HID，导致
//! Windows 原生音量失效且 Tap 未就绪时三键全死）。

use crate::bridges::xiaomi::connect::XiaomiRuntime;
use crate::bridges::xiaomi::input_session::run_input_session;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// 一次 BLE 连接所创建的输入线程。重连前必须全部结束，不能遗留旧回调。
pub struct KeyLoggerSession {
    input: Option<std::thread::JoinHandle<()>>,
    vk_poll: Option<std::thread::JoinHandle<()>>,
    raw_mapping: Option<std::thread::JoinHandle<()>>,
}

impl KeyLoggerSession {
    #[cfg(not(target_os = "windows"))]
    fn empty() -> Self {
        Self {
            input: None,
            vk_poll: None,
            raw_mapping: None,
        }
    }

    pub fn stop_and_join(
        mut self,
        runtime: &XiaomiRuntime,
        session_id: u64,
        reason: &str,
    ) {
        runtime.end_session(session_id, reason);
        crate::bridges::xiaomi::key_mapping::reset_voice_input_state(reason);
        for handle in [self.input.take(), self.vk_poll.take(), self.raw_mapping.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.join();
        }
        log::info!("XIAOMI SESSION workers joined id={session_id} reason={reason}");
    }
}

/// HID Tap 附着等待决策：返回 true 表示可以附着。
///
/// - `atvv_ok`：ATVV 已订阅成功 → 立即附着（无 WUDFHost 竞争）
/// - `diagnosed_failed`：ATVV 首轮诊断已失败（进入降级模式）→ 等 `fail_grace` 宽限窗口
///   让后台重试，仍失败才附着（此时蓝牙栈已稳定，竞争风险低）
/// - `hard_limit`：硬上限兜底，防止 ATVV 长时间无结论时返回/音量键永久不可用
fn tap_attach_due(
    atvv_ok: bool,
    diagnosed_failed: bool,
    elapsed: Duration,
    fail_grace: Duration,
    hard_limit: Duration,
) -> bool {
    if atvv_ok {
        return true;
    }
    if elapsed >= hard_limit {
        return true;
    }
    diagnosed_failed && elapsed >= fail_grace
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XiaomiKeyEvent {
    pub button_id: String,
    pub label: String,
    /// "down" | "up"
    #[serde(default = "default_key_phase")]
    pub phase: String,
}

#[allow(dead_code)] // referenced by serde default = "default_key_phase"
fn default_key_phase() -> String {
    "down".into()
}

#[derive(Clone, Serialize)]
pub struct XiaomiKeyMessage {
    pub message: String,
}

/// 按键去抖门闩：同一 button_id 在窗口内只发一次 UI 事件
pub struct KeyEmitGate {
    last: Mutex<HashMap<String, Instant>>,
    window: Duration,
}

impl KeyEmitGate {
    pub fn new(window_ms: u64) -> Self {
        Self {
            last: Mutex::new(HashMap::new()),
            window: Duration::from_millis(window_ms),
        }
    }

    pub fn try_emit(&self, button_id: &str) -> bool {
        let now = Instant::now();
        let mut guard = self.last.lock();
        if let Some(prev) = guard.get(button_id) {
            if now.duration_since(*prev) < self.window {
                return false;
            }
        }
        guard.insert(button_id.to_string(), now);
        true
    }
}

pub fn emit_key(app: &AppHandle, button_id: &str, label: &str) {
    emit_key_phase(app, button_id, label, true);
}

pub fn emit_key_phase(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyEvent {
            button_id: button_id.to_string(),
            label: label.to_string(),
            phase: if pressed { "down".into() } else { "up".into() },
        },
    );
}

/// 对齐 Python：检测后立刻执行 button_bindings 映射
pub fn emit_key_and_map(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    emit_key_phase(app, button_id, label, pressed);
    crate::bridges::xiaomi::key_mapping::on_remote_button(app, button_id, pressed);
}

pub fn emit_message(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyMessage {
            message: message.to_string(),
        },
    );
}

pub fn button_label(id: &str) -> &'static str {
    match id {
        "power" => "电源",
        "volume_up" => "音量+",
        "volume_down" => "音量-",
        "up" | "dpad_up" => "上",
        "down" | "dpad_down" => "下",
        "left" | "dpad_left" => "左",
        "right" | "dpad_right" => "右",
        "ok" => "确定",
        "back" => "返回",
        "home" => "主页",
        "menu" => "菜单",
        "voice" | "mic" => "语音",
        "mute" | "volume_mute" => "静音",
        "tv" => "TV",
        _ => "未知",
    }
}

/// 连接成功后启动按键通道（对齐 Python atvv_live_bridge 启动顺序）
pub fn start_key_logger(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    session_id: u64,
    address_u64: u64,
    atvv_interface_id: String,
) -> KeyLoggerSession {
    #[cfg(target_os = "windows")]
    {
        use crate::bridges::xiaomi::connect::reset_atvv_subscribed;
        use crate::bridges::xiaomi::hid_report_tap::{ensure_started, stop_and_join};
        use crate::config::manager::ConfigManager;
        use tauri::Manager;

        let gate = Arc::new(KeyEmitGate::new(90));
        let (tap_enabled, hook_enabled) = app
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("xiaomi").ok())
            .map(|c| (c.hid_report_tap_enabled, c.special_key_hook_enabled))
            .unwrap_or((true, true));

        crate::bridges::xiaomi::special_keys::set_hook_enabled(hook_enabled);
        crate::bridges::xiaomi::key_mapping::bind_voice_hook_app(app.clone());
        if hook_enabled {
            crate::bridges::xiaomi::special_keys::start_special_key_hook();
        }

        // 先完成 ATVV 首次订阅，再附着 HID Tap，避免两者并发抢占 WUDFHost。
        reset_atvv_subscribed();

        let input = {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            let iface = atvv_interface_id.clone();
            std::thread::Builder::new()
                .name(format!("xiaomi-gatt-input-{session_id}"))
                .spawn(move || {
                    let result = run_input_session(
                        app2.clone(),
                        address_u64,
                        iface,
                        runtime2.clone(),
                        session_id,
                        gate2,
                    );
                    crate::bridges::xiaomi::key_mapping::reset_voice_input_state(
                        "input_session_end",
                    );
                    runtime2.end_session(session_id, "input_session_end");
                    if let Err(e) = result {
                        log::warn!("ATVV input session unavailable session={session_id}: {e}");
                        emit_message(&app2, &format!("ATVV 语音通道不可用: {e}"));
                    }
                })
                .ok()
        };

        // HID Tap 附着时机以 ATVV 订阅结果为准（避免并发抢占 WUDFHost）：
        // - ATVV 订阅成功 → 立即附着（无竞争）
        // - ATVV 首轮诊断失败 → 给后台重试 15 秒窗口；仍失败才附着（此时蓝牙栈已稳定，
        //   竞争风险低；返回/音量优先于语音的妥协，ATVV 后台仍继续重试）
        // - 30 秒硬上限兜底；会话结束即放弃
        let wait_start = Instant::now();
        let atvv_fail_grace = Duration::from_secs(15);
        while runtime.session_active(session_id)
            && !tap_attach_due(
                crate::bridges::xiaomi::connect::atvv_subscribed(),
                crate::bridges::xiaomi::connect::atvv_diagnosed_failed(),
                wait_start.elapsed(),
                atvv_fail_grace,
                Duration::from_secs(30),
            )
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        if runtime.session_active(session_id)
            && !crate::bridges::xiaomi::connect::atvv_subscribed()
            && wait_start.elapsed() >= Duration::from_secs(30)
        {
            log::warn!("XIAOMI HID TAP attach wait timeout (ATVV not ready)");
        }

        let atvv_ok = crate::bridges::xiaomi::connect::atvv_subscribed();
        let mut tap_started = false;
        if runtime.session_active(session_id) && tap_enabled {
            if atvv_ok {
                let app2 = app.clone();
                let gate2 = Arc::clone(&gate);
                tap_started = ensure_started(app2, gate2);
                if !tap_started {
                    emit_message(
                        &app,
                        "HID Tap 未启动：返回/音量键不可用（请确认 Frida Gadget 资源）",
                    );
                }
            } else {
                emit_message(
                    &app,
                    "ATVV 语音通道未就绪，暂不附着 HID Tap（语音优先）；返回/音量走系统原生键",
                );
            }
        } else if !tap_enabled {
            stop_and_join();
            emit_message(&app, "HID Tap 已按配置禁用");
        }

        let raw_mapping = crate::bridges::xiaomi::raw_mapping::maybe_start_raw_mapping(
            app.clone(),
            Arc::clone(&runtime),
            session_id,
            Arc::clone(&gate),
            tap_started,
        );

        let vk_poll = {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            std::thread::Builder::new()
                .name(format!("xiaomi-vk-poll-{session_id}"))
                .spawn(move || {
                    windows_vk_poll_logger(app2, runtime2, session_id, gate2);
                })
                .ok()
        };

        let mode_desc = if tap_started {
            "HID-Tap 返回/音量 + ATVV 语音/音频".to_string()
        } else if atvv_ok {
            "ATVV 语音/音频".to_string()
        } else {
            "Battery/ATVV 后台重试中（语音与返回/音量受限）".to_string()
        };
        emit_message(
            &app,
            &format!("按键监听已启动 session={session_id}（{mode_desc}）"),
        );
        return KeyLoggerSession {
            input,
            vk_poll,
            raw_mapping,
        };
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, runtime, session_id, address_u64, atvv_interface_id);
        KeyLoggerSession::empty()
    }
}

/// VK 轮询：仅作 UI/诊断兜底，不执行映射（避免与系统原生气 + HID 映射双触发）
#[cfg(target_os = "windows")]
fn windows_vk_poll_logger(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    session_id: u64,
    gate: Arc<KeyEmitGate>,
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let keys: &[(i32, &str)] = &[
        (0xAF, "volume_up"),
        (0xAE, "volume_down"),
        (0xAD, "volume_mute"),
        (0x26, "up"),
        (0x28, "down"),
        (0x25, "left"),
        (0x27, "right"),
        (0x0D, "ok"),
        (0x24, "home"),
    ];

    let mut prev: HashMap<i32, bool> = HashMap::new();
    while runtime.session_active(session_id) {
        for &(vk, id) in keys {
            let down = unsafe { GetAsyncKeyState(vk) as u16 } & 0x8000 != 0;
            let was = prev.get(&vk).copied().unwrap_or(false);
            // 某些蓝牙遥控器的方向键不会进入 LL hook，但会在这里被观察到。
            // Alt+Tab 长按会话下，使用同一状态机补发带标记的方向键。
            if down != was && matches!(vk, 0x25..=0x28)
                && crate::bridges::xiaomi::key_mapping::alt_tab_hold_active()
            {
                crate::bridges::xiaomi::key_mapping::relay_alt_tab_navigation(vk as u16, !down);
                log::info!("XIAOMI VK alt_tab relay key={id} vk=0x{vk:02X} up={}", !down);
            }
            if down && !was && gate.try_emit(id) {
                emit_key(&app, id, button_label(id));
                log::info!("XIAOMI VK observe key={id} vk=0x{vk:02X} (no map)");
            }
            prev.insert(vk, down);
        }
        // ponytail: 25ms 高频仅 Alt+Tab 长按期间需要；平时 150ms 省唤醒
        std::thread::sleep(Duration::from_millis(if crate::bridges::xiaomi::key_mapping::alt_tab_hold_active() {
            25
        } else {
            150
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRACE: Duration = Duration::from_secs(15);
    const HARD: Duration = Duration::from_secs(30);

    #[test]
    fn tap_attach_due_attaches_immediately_when_atvv_ok() {
        assert!(tap_attach_due(true, false, Duration::ZERO, GRACE, HARD));
        assert!(tap_attach_due(true, true, Duration::ZERO, GRACE, HARD));
    }

    #[test]
    fn tap_attach_due_waits_while_atvv_diagnosing() {
        // 诊断中（未失败也未成功）：即使超过 hard_limit 之前的任意时刻都不附着
        assert!(!tap_attach_due(false, false, Duration::from_secs(10), GRACE, HARD));
    }

    #[test]
    fn tap_attach_due_grace_window_after_diagnosed_failure() {
        // 诊断失败后 15 秒宽限窗口内不附着（给 ATVV 后台重试机会）
        assert!(!tap_attach_due(false, true, Duration::from_secs(14), GRACE, HARD));
        // 超过宽限窗口才附着（返回/音量优先于语音的妥协）
        assert!(tap_attach_due(false, true, Duration::from_secs(15), GRACE, HARD));
    }

    #[test]
    fn tap_attach_due_hard_limit_always_attaches() {
        assert!(tap_attach_due(false, false, HARD, GRACE, HARD));
        assert!(tap_attach_due(false, false, Duration::from_secs(31), GRACE, HARD));
    }
}
