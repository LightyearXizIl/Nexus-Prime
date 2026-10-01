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
    coordinator: Option<std::thread::JoinHandle<()>>,
}

impl KeyLoggerSession {
    #[cfg(not(target_os = "windows"))]
    fn empty() -> Self {
        Self {
            input: None,
            vk_poll: None,
            raw_mapping: None,
            coordinator: None,
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
        for handle in [self.coordinator.take(), self.input.take(), self.vk_poll.take(), self.raw_mapping.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.join();
        }
        log::info!("XIAOMI SESSION workers joined id={session_id} reason={reason}");
    }
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
                    runtime2.end_session(session_id, "input_session_end");
                    crate::bridges::xiaomi::key_mapping::set_input_session_active(false);
                    crate::bridges::xiaomi::voice_pcm::stop();
                    crate::bridges::xiaomi::connect::mark_atvv_subscribed(false);
                    if let Err(e) = result {
                        log::warn!("ATVV input session unavailable session={session_id}: {e}");
                        emit_message(&app2, &format!("ATVV 语音通道不可用: {e}"));
                    }
                })
                .ok()
        };

        // Ongoing coordination: late ATVV recovery must also attach the key bridge.
        let coordinator = {
            let app2 = app.clone();
            let runtime2 = runtime.clone();
            let gate2 = gate.clone();
            std::thread::Builder::new().name(format!("xiaomi-key-coordinator-{session_id}"))
                .spawn(move || {
                    let mut last_attempt = Instant::now() - Duration::from_secs(10);
                    let mut previous = "waiting";
                    while runtime2.session_active(session_id) {
                        let connected = runtime2.health.lock().connected == Some(true)
                            && crate::bridges::xiaomi::key_mapping::input_session_active();
                        let atvv = crate::bridges::xiaomi::connect::atvv_subscribed();
                        let state = if !connected { "waiting" }
                        else if !tap_enabled { if runtime2.health.lock().raw_ready { "fallback" } else { "starting" } }
                        else if crate::bridges::xiaomi::special_keys::hid_tap_ready() { "ready" }
                        else if atvv {
                            if crate::bridges::xiaomi::recovery::tap_attach_due(connected, atvv, tap_enabled, crate::bridges::xiaomi::hid_report_tap::is_running())
                                && last_attempt.elapsed() >= Duration::from_secs(5) {
                                last_attempt = Instant::now();
                                if !ensure_started(app2.clone(), gate2.clone()) {
                                    runtime2.health.lock().key_bridge = "failed";
                                }
                            }
                            if crate::bridges::xiaomi::hid_report_tap::is_running() { "starting" } else if runtime2.health.lock().raw_ready { "fallback" } else { "failed" }
                        } else { "waiting" };
                        runtime2.health.lock().key_bridge = state;
                        if state != previous {
                            if state == "ready" || previous == "ready" {
                                crate::bridges::xiaomi::key_mapping::cancel_pending_gestures();
                            }
                            log::info!("XIAOMI KEY BRIDGE session={session_id} state={state}");
                            crate::bridges::emit_device_status(&app2, crate::bridges::BridgeType::Xiaomi);
                            previous = state;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    stop_and_join();
                }).ok()
        };
        if input.is_none() || coordinator.is_none() {
            runtime.end_session(session_id, "input_worker_spawn_failed");
        }
        let raw_mapping = crate::bridges::xiaomi::raw_mapping::maybe_start_raw_mapping(
            app.clone(),
            Arc::clone(&runtime),
            session_id,
            Arc::clone(&gate),
            false,
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

        emit_message(&app, &format!("输入恢复管理器已启动 session={session_id}"));
        return KeyLoggerSession {
            input,
            vk_poll,
            raw_mapping,
            coordinator,
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
        // 方向键可能不进入 LL hook，且快速点按可短于 150ms。这里保持
        // 25ms 采样，避免 Alt+Tab 刚激活时漏掉首个方向键。
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_requests_and_manual_disconnect_cannot_revive_old_session() {
        let runtime = XiaomiRuntime::new();
        let first = runtime.begin_session();
        for _ in 0..100 { runtime.probe_requested.store(true, std::sync::atomic::Ordering::SeqCst); }
        assert!(runtime.probe_requested.swap(false, std::sync::atomic::Ordering::SeqCst));
        assert!(!runtime.probe_requested.swap(false, std::sync::atomic::Ordering::SeqCst));
        runtime.request_stop();
        assert!(!runtime.session_active(first));
        runtime.cancel_active_session("test");
        runtime.clear_stop();
        let second = runtime.begin_session();
        assert!(!runtime.end_session(first, "stale"));
        assert!(runtime.session_active(second));
    }
}
