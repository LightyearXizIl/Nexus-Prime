//! 小米输入会话 — 对齐 Python `XiaomiGattHidSession` + ATVV Control
//!
//! - 返回键：HID usage `0xF1`（Windows kbdhid 丢弃）→ GATT HID Report
//! - 音量±：HID usage `0x80`/`0x81`（GATT）或由上层 VK 并行兜底
//! - 语音键：ATVV Control opcode `0x08`/`0x04`/`0x00`

use crate::bridges::xiaomi::ble_bridge::XiaomiButton;
use crate::bridges::xiaomi::connect::{mark_atvv_subscribed, reset_atvv_subscribed, XiaomiRuntime};
use crate::bridges::xiaomi::key_log::{
    button_label, emit_key_and_map, emit_key_phase, emit_message, KeyEmitGate,
};
use crate::bridges::xiaomi::key_mapping;
use crate::config::manager::{ConfigManager, TriggerMode};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const HID_SERVICE: u128 = 0x00001812_0000_1000_8000_00805f9b34fb;
const HID_REPORT: u128 = 0x00002a4d_0000_1000_8000_00805f9b34fb;
const HID_REPORT_REFERENCE: u128 = 0x00002908_0000_1000_8000_00805f9b34fb;
const HID_CONTROL_POINT: u128 = 0x00002a4c_0000_1000_8000_00805f9b34fb;
const HID_PROTOCOL_MODE: u128 = 0x00002a4e_0000_1000_8000_00805f9b34fb;

const ATVV_SERVICE: u128 = 0xab5e0001_5a21_4f05_bc7d_af01f617b664;
const ATVV_TX: u128 = 0xab5e0002_5a21_4f05_bc7d_af01f617b664;
const ATVV_AUDIO: u128 = 0xab5e0003_5a21_4f05_bc7d_af01f617b664;
const ATVV_CONTROL: u128 = 0xab5e0004_5a21_4f05_bc7d_af01f617b664;

/// 标准 BLE Battery Service / Battery Level
const BATTERY_SERVICE: u128 = 0x0000180f_0000_1000_8000_00805f9b34fb;
const BATTERY_LEVEL: u128 = 0x00002a19_0000_1000_8000_00805f9b34fb;
/// BAS 1.1：可选的电量与充电状态汇总特征。
const BATTERY_LEVEL_STATUS: u128 = 0x00002bed_0000_1000_8000_00805f9b34fb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryChargingState {
    Unknown,
    Charging,
    DischargingActive,
    DischargingInactive,
}

impl BatteryChargingState {
    fn is_charging(self) -> Option<bool> {
        match self {
            Self::Charging => Some(true),
            Self::DischargingActive | Self::DischargingInactive => Some(false),
            Self::Unknown => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Charging => "charging",
            Self::DischargingActive => "discharging_active",
            Self::DischargingInactive => "discharging_inactive",
        }
    }
}

const GET_CAPS_V10: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];

fn disconnect_confirmed(initial: bool, after_500ms: bool, after_750ms: bool) -> bool {
    initial && after_500ms && after_750ms
}

/// 解析 RC003 HID 报告（对齐 Python `handle_direct_hid_report` / `decode_rc003_ioctl_output`）
pub fn parse_hid_usages(payload: &[u8]) -> HashSet<u16> {
    let mut usages = HashSet::new();
    let data: &[u8] = if payload.len() == 9 && payload.starts_with(&[0x01, 0x00, 0x00]) {
        // HidOverGatt IOCTL：3 字节前缀 + 6 字节 usages
        &payload[3..]
    } else if payload.len() == 7 && payload[0] == 1 {
        // 带 report id=1 前缀
        &payload[1..]
    } else if payload.len() >= 6 && payload.len() % 2 == 0 {
        payload
    } else if payload.len() > 6 && (payload.len() - 1) % 2 == 0 && payload[0] <= 0x0F {
        // 其它小 report id 前缀
        &payload[1..]
    } else {
        payload
    };

    if data.is_empty() || data.len() % 2 != 0 {
        return usages;
    }
    for chunk in data.chunks_exact(2) {
        let usage = u16::from_le_bytes([chunk[0], chunk[1]]);
        if usage != 0 {
            usages.insert(usage);
        }
    }
    usages
}

/// 启动 GATT HID + ATVV（阻塞直到 stop）。任一通道成功即可。
pub fn run_input_session(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<XiaomiRuntime>,
    session_id: u64,
    gate: Arc<KeyEmitGate>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_run_input_session(app, address_u64, atvv_interface_id, runtime, session_id, gate)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, address_u64, atvv_interface_id, runtime, session_id, gate);
        Err("仅支持 Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn windows_run_input_session(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<XiaomiRuntime>,
    session_id: u64,
    gate: Arc<KeyEmitGate>,
) -> Result<(), String> {
    use windows::core::{GUID, HSTRING};
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattCommunicationStatus, GattDeviceService, GattSession,
    };
    use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice};
    use windows::Foundation::TypedEventHandler;
    use crate::bridges::xiaomi::tv_gate;
    use crate::bridges::xiaomi::voice_pcm;
    use crate::config::manager::ConfigManager;
    use tauri::Manager;

    log::info!("XIAOMI INPUT SESSION start id={session_id}");

    tv_gate::mark_connecting();
    reset_atvv_subscribed();

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let cfg = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok());
    let gain_db = cfg.as_ref().map(|c| c.gain_db).unwrap_or(10.0);
    let tv_delay = cfg
        .as_ref()
        .map(|c| c.tv_action_ready_delay)
        .unwrap_or(2.0);

    // The discovery phase has already opened the ATVV service by interface
    // ID.  Reuse that ID for the input-session device instead of creating a
    // fresh address-only object, which can sit disconnected until Windows
    // times out its separate GATT connection attempt.
    let device = if atvv_interface_id.is_empty() {
        BluetoothLEDevice::FromBluetoothAddressAsync(address_u64)
            .map_err(|e| format!("input session address open: {e}"))?
            .get()
            .map_err(|e| format!("input session address get: {e}"))?
    } else {
        let id = HSTRING::from(atvv_interface_id.as_str());
        BluetoothLEDevice::FromIdAsync(&id)
            .map_err(|e| format!("input session FromId open: {e}"))?
            .get()
            .map_err(|e| format!("input session FromId get: {e}"))?
    };

    // Keep a Windows GATT session alive for the entire input session.  This
    // asks the Bluetooth stack to establish the link when the remote wakes,
    // rather than relying on a one-off discovery request with a short timeout.
    let gatt_session = device
        .BluetoothDeviceId()
        .ok()
        .and_then(|id| GattSession::FromDeviceIdAsync(&id).ok())
        .and_then(|op| op.get().ok());
    if let Some(session) = gatt_session.as_ref() {
        match session.CanMaintainConnection() {
            Ok(true) => match session.SetMaintainConnection(true) {
                Ok(()) => log::info!("XIAOMI GATT maintain-connection enabled"),
                Err(error) => log::warn!("XIAOMI GATT maintain-connection failed: {error}"),
            },
            Ok(false) => log::info!("XIAOMI GATT maintain-connection unsupported"),
            Err(error) => log::warn!("XIAOMI GATT maintain-connection unavailable: {error}"),
        }
    } else {
        log::warn!("XIAOMI GATT session unavailable; using operation-triggered connection");
    }

    let mut tokens: Vec<(
        GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )> = Vec::new();
    // The ATVV service interface was successfully opened during discovery.
    // Subscribe through it before asking Windows to enumerate every GATT
    // service again.  Some adapters report the broad enumeration as
    // Unreachable even while this exact ATVV interface is usable.
    let mut atvv_ok = false;
    let mut last_atvv_fail: Option<AtvvFailReason> = None;
    if !atvv_interface_id.is_empty() {
        match subscribe_atvv_from_interface(
            &app,
            &atvv_interface_id,
            &gate,
            &mut tokens,
            gain_db,
        ) {
            Ok(true) => {
                atvv_ok = true;
                emit_message(&app, "ATVV 语音键/音频已订阅（FromId）");
            }
            Ok(false) => {
                last_atvv_fail = Some(AtvvFailReason::chars_incomplete());
            }
            Err(error) => {
                last_atvv_fail = Some(AtvvFailReason::from_error(&error));
                log::warn!("ATVV pre-subscribe FromId failed: {error}");
            }
        }
    }

    // FromBluetoothAddressAsync only creates a WinRT object; it does not
    // necessarily establish a BLE link.  Do not treat its initial
    // ConnectionStatus as authoritative before uncached GATT discovery.
    let services_result = device
        .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let services_status = services_result.Status().ok();
    let services = if services_status == Some(GattCommunicationStatus::Success) {
        Some(services_result.Services().map_err(|e| e.to_string())?)
    } else if atvv_ok || gatt_session.is_some() {
        log::warn!(
            "GATT full discovery unavailable: {}; keeping the GATT session alive and retrying ATVV",
            describe_gatt_comm_status(services_status)
        );
        None
    } else {
        return Err(format!(
            "GATT 服务发现失败: {}",
            describe_gatt_comm_status(services_status)
        ));
    };

    // GATT discovery above is the session's connection proof.  After that,
    // debounce a disconnect event so Windows' transient status transition
    // cannot tear down a freshly subscribed ATVV session.
    let runtime_conn = Arc::clone(&runtime);
    let conn_token = device
        .ConnectionStatusChanged(&TypedEventHandler::new(
            move |sender: &Option<BluetoothLEDevice>, _args| {
                let Some(dev) = sender else {
                    return Ok(());
                };
                let initially_disconnected = dev.ConnectionStatus().ok()
                    == Some(BluetoothConnectionStatus::Disconnected);
                if !initially_disconnected {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(500));
                let after_500ms = dev.ConnectionStatus().ok()
                    == Some(BluetoothConnectionStatus::Disconnected);
                if !after_500ms {
                    log::info!("Xiaomi disconnect transition recovered id={session_id}");
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(250));
                if disconnect_confirmed(
                    initially_disconnected,
                    after_500ms,
                    dev.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Disconnected),
                ) {
                    log::warn!("Xiaomi remote disconnected confirmed id={session_id}");
                    crate::bridges::xiaomi::key_mapping::reset_voice_input_state(
                        "remote_disconnected",
                    );
                    runtime_conn.end_session(session_id, "remote_disconnected");
                }
                Ok(())
            },
        ))
        .map_err(|e| format!("ConnectionStatusChanged: {e}"))?;

    let hid_guid = GUID::from_u128(HID_SERVICE);
    let atvv_guid = GUID::from_u128(ATVV_SERVICE);
    let battery_guid = GUID::from_u128(BATTERY_SERVICE);
    let report_guid = GUID::from_u128(HID_REPORT);
    let report_ref_guid = GUID::from_u128(HID_REPORT_REFERENCE);
    let protocol_guid = GUID::from_u128(HID_PROTOCOL_MODE);
    let control_point_guid = GUID::from_u128(HID_CONTROL_POINT);

    let mut hid_service: Option<GattDeviceService> = None;
    let mut atvv_service: Option<GattDeviceService> = None;
    let mut battery_service: Option<GattDeviceService> = None;
    if let Some(services) = services {
        let count = services.Size().map_err(|e| e.to_string())?;
        for i in 0..count {
            let svc = services.GetAt(i).map_err(|e| e.to_string())?;
            let uuid = svc.Uuid().map_err(|e| e.to_string())?;
            if uuid == hid_guid {
                hid_service = Some(svc);
            } else if uuid == atvv_guid {
                atvv_service = Some(svc);
            } else if uuid == battery_guid {
                battery_service = Some(svc);
            }
        }
    }

    let active_usages: Arc<Mutex<HashSet<u16>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut hid_ok = false;

    // 默认跳过 GATT HID：Windows Microsoft HID 独占时 Open/CCCD 会抢占设备，
    // 导致原生音量失效且又收不到报告。生产路径用 HID Tap（对齐 Python 注释）。
    // 仅当显式设置 REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID=1 时尝试（Windows HID 关闭时的 fallback）。
    let force_gatt_hid = std::env::var("REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if force_gatt_hid {
        if let Some(hid) = hid_service.as_ref() {
            try_subscribe_gatt_hid(
                &app,
                hid,
                &gate,
                &active_usages,
                &mut tokens,
                &mut hid_ok,
                protocol_guid,
                control_point_guid,
                report_guid,
                report_ref_guid,
            );
        } else {
            log::warn!("FORCE_GATT_HID set but HID service not found");
        }
    } else {
        log::info!(
            "Skip GATT HID open (use HID Tap for back/volume); set \
             REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID=1 only if Windows HID is disabled"
        );
        emit_message(
            &app,
            "跳过 GATT HID（避免抢占 Windows 音量；返回/音量走 HID Tap）",
        );
    }

    // ---- ATVV Control：语音键（对齐 v1.3.3：FromId 优先，再地址路径）----
    for attempt in 0..8 {
        if !runtime.session_active(session_id) {
            break;
        }
        if atvv_ok {
            break;
        }
        if attempt > 0 {
            log::info!("ATVV subscribe retry attempt={attempt}");
            std::thread::sleep(Duration::from_millis(500));
        }

        // 1) 发现阶段 AQS 接口 FromId（连接时已 Open 过，成功率最高）
        if !atvv_interface_id.is_empty() {
            match subscribe_atvv_from_interface(
                &app,
                &atvv_interface_id,
                &gate,
                &mut tokens,
                gain_db,
            ) {
                Ok(true) => {
                    atvv_ok = true;
                    emit_message(&app, "ATVV 语音键/音频已订阅（FromId）");
                }
                Ok(false) => {
                    let reason = AtvvFailReason::chars_incomplete();
                    log_atvv_fail("FromId", &reason, attempt);
                    last_atvv_fail = Some(reason);
                }
                Err(e) => {
                    let reason = AtvvFailReason::from_error(&e);
                    log_atvv_fail("FromId", &reason, attempt);
                    if attempt == 0 {
                        emit_message(
                            &app,
                            &format!("ATVV FromId 失败，回退地址打开: {}", reason.label),
                        );
                    }
                    last_atvv_fail = Some(reason);
                }
            }
        }

        // 2) 回退：设备枚举到的 ATVV 服务（地址路径）
        if !atvv_ok {
            if let Some(atvv) = atvv_service.as_ref() {
                match subscribe_atvv_periodic_retry(
                    &app,
                    atvv,
                    &gate,
                    &mut tokens,
                    gain_db,
                    &runtime,
                    session_id,
                ) {
                    Ok(true) => {
                        atvv_ok = true;
                        emit_message(&app, "ATVV 语音键/音频已订阅");
                    }
                    Ok(false) => {
                        let reason = AtvvFailReason::chars_incomplete();
                        log_atvv_fail("address-path", &reason, attempt);
                        last_atvv_fail = Some(reason);
                    }
                    Err(e) => {
                        let reason = AtvvFailReason::from_error(&e);
                        log_atvv_fail("address-path", &reason, attempt);
                        last_atvv_fail = Some(reason);
                    }
                }
            } else if atvv_interface_id.is_empty() {
                let reason = AtvvFailReason::service_missing();
                log_atvv_fail("address-path", &reason, attempt);
                last_atvv_fail = Some(reason);
            }
        }
    }

    if atvv_ok {
        mark_atvv_subscribed(true);
        log::info!("ATVV subscribe ok after diagnostics");
    }

    // ---- Battery Level（0x180F / 0x2A19）----
    // 与 ATVV 解耦：语音通道失败时仍应能显示电量
    let mut battery_ch: Option<GattCharacteristic> = None;
    let mut battery_status_ch: Option<GattCharacteristic> = None;
    let mut last_battery: Option<u8> = None;
    let mut last_battery_charging: Option<BatteryChargingState> = None;
    if let Some(state) = app.try_state::<crate::bridges::BridgeState>() {
        // A new session must not inherit a charge state reported by an older connection.
        state.update_battery_charging(crate::bridges::BridgeType::Xiaomi, None);
    }
    if let Some(batt) = battery_service.as_ref() {
        match setup_battery_monitor(&app, batt, &mut tokens) {
            Ok((level_ch, status_ch)) => {
                if let Some(level) = read_battery_level(&level_ch) {
                    publish_battery(&app, level, &mut last_battery, true);
                }
                if let Some(status_ch) = status_ch.as_ref() {
                    if let Some(charging) = read_battery_charging_status(status_ch) {
                        publish_battery_charging(
                            &app,
                            charging,
                            &mut last_battery_charging,
                            true,
                        );
                    }
                }
                battery_ch = Some(level_ch);
                battery_status_ch = status_ch;
            }
            Err(e) => {
                log::warn!("XIAOMI BATTERY setup failed: {e}");
                emit_message(&app, &format!("电量读取失败: {e}"));
            }
        }
    } else {
        log::info!("XIAOMI BATTERY service 0x180F not found on device");
    }

    if !atvv_ok {
        // 通知 key_logger：ATVV 首轮诊断失败，HID Tap 附着需等待后台重试窗口，
        // 避免在语音通道未就绪时注入 DLL 抢占 WUDFHost。
        crate::bridges::xiaomi::connect::mark_atvv_diagnosed_failed();
        if battery_ch.is_none() && gatt_session.is_none() {
            tv_gate::reset();
            return Err(
                "无法订阅 ATVV 通知（语音键依赖 ATVV；返回/音量依赖 HID Tap）".into(),
            );
        }
        log::warn!("ATVV subscribe failed; keeping session alive for retry");
        let reason = last_atvv_fail.unwrap_or_else(AtvvFailReason::unknown);
        log::warn!(
            "ATVV FAIL code={} recoverable={} hint={}",
            reason.code,
            reason.recoverable,
            reason.hint
        );
        emit_message(
            &app,
            &format!(
                "ATVV 不可用：{}（{}；将保持连接并自动重试；{}）",
                reason.label,
                reason.code,
                if reason.recoverable {
                    "将后台重试，或请重连"
                } else {
                    "请重连遥控器"
                }
            ),
        );
        crate::bridges::xiaomi::conflict_guard::notify_atvv_failed(&format!(
            "{} ({})",
            reason.label, reason.code
        ));
    }

    let mode = match (hid_ok, atvv_ok) {
        (true, true) => "GATT HID+ATVV",
        (true, false) => "GATT HID",
        (false, true) => "ATVV（语音+音频）",
        _ if battery_ch.is_some() => "Battery",
        _ => "GATT",
    };
    emit_message(&app, &format!("输入会话已启动 ({mode})"));
    log::info!(
        "Input session running mode={mode} atvv={atvv_ok} battery={} battery_status={} subscriptions={}",
        battery_ch.is_some(),
        battery_status_ch.is_some(),
        tokens.len()
    );
    if atvv_ok {
        tv_gate::mark_ready(Duration::from_secs_f32(tv_delay.max(0.0)));
        crate::bridges::xiaomi::hid_injector::warmup_async();
        // Never wait for the router on the BLE session thread.  The first
        // complete frames are held by voice_pcm until this warmup succeeds.
        voice_pcm::warmup_async();
    } else if battery_ch.is_some() {
        tv_gate::mark_ready(Duration::from_secs_f32(tv_delay.max(0.0)));
    }

    crate::bridges::xiaomi::key_mapping::set_input_session_active(true);

    let mut since_batt = Instant::now();
    let mut since_battery_status = Instant::now();
    let mut since_pcm_warm = Instant::now();
    let mut since_atvv_retry = Instant::now();
    while runtime.session_active(session_id) {
        std::thread::sleep(Duration::from_millis(200));
        if !atvv_ok && since_atvv_retry.elapsed() >= Duration::from_secs(3) {
            since_atvv_retry = Instant::now();
            let retry = if !atvv_interface_id.is_empty() {
                subscribe_atvv_from_interface(&app, &atvv_interface_id, &gate, &mut tokens, gain_db)
            } else if let Some(atvv) = atvv_service.as_ref() {
                subscribe_atvv_service(&app, atvv, &gate, &mut tokens, gain_db)
            } else {
                Ok(false)
            };
            match retry {
                Ok(true) => {
                    atvv_ok = true;
                    mark_atvv_subscribed(true);
                    emit_message(&app, "ATVV 语音键/音频已订阅（后台重试成功）");
                    log::info!("ATVV subscribe recovered on periodic retry");
                    tv_gate::mark_ready(Duration::from_secs_f32(tv_delay.max(0.0)));
                    crate::bridges::xiaomi::hid_injector::warmup_async();
                    voice_pcm::warmup_async();
                }
                Ok(false) => log::debug!(
                    "ATVV periodic retry: {}",
                    AtvvFailReason::chars_incomplete().code
                ),
                Err(e) => {
                    let reason = AtvvFailReason::from_error(&e);
                    log::debug!(
                        "ATVV periodic retry still failing code={} raw={e}",
                        reason.code
                    );
                }
            }
        }
        // 会话中保持 PCM 通路预热（路由重启后自动恢复）
        if atvv_ok
            && !voice_pcm::is_ready()
            && since_pcm_warm.elapsed() >= Duration::from_secs(2)
        {
            since_pcm_warm = Instant::now();
            voice_pcm::warmup_async();
        }
        if let Some(ch) = battery_ch.as_ref() {
            // 首次已读；之后每 45s 轮询，并在启动后 3s 再读一次（提高 UI 首次可见性）
            let due = since_batt.elapsed() >= Duration::from_secs(45)
                || (last_battery.is_none() && since_batt.elapsed() >= Duration::from_secs(3));
            if due {
                since_batt = Instant::now();
                if let Some(level) = read_battery_level(ch) {
                    publish_battery(&app, level, &mut last_battery, false);
                }
            }
        }
        if let Some(ch) = battery_status_ch.as_ref() {
            let due = since_battery_status.elapsed() >= Duration::from_secs(45)
                || (last_battery_charging.is_none()
                    && since_battery_status.elapsed() >= Duration::from_secs(3));
            if due {
                since_battery_status = Instant::now();
                if let Some(charging) = read_battery_charging_status(ch) {
                    publish_battery_charging(
                        &app,
                        charging,
                        &mut last_battery_charging,
                        false,
                    );
                }
            }
        }
    }

    voice_pcm::stop();
    crate::bridges::xiaomi::key_mapping::reset_voice_input_state("input_session_cleanup");
    crate::bridges::xiaomi::key_mapping::set_input_session_active(false);
    tv_gate::reset();
    mark_atvv_subscribed(false);
    let _ = device.RemoveConnectionStatusChanged(conn_token);
    if let Some(session) = gatt_session {
        let _ = session.SetMaintainConnection(false);
        let _ = session.Close();
    }
    runtime.end_session(session_id, "input_session_cleanup");
    log::info!("XIAOMI INPUT SESSION cleanup id={session_id}");
    for (ch, token) in tokens {
        let _ = ch.RemoveValueChanged(token);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn publish_battery(app: &AppHandle, level: u8, last: &mut Option<u8>, force_log: bool) {
    use crate::bridges::{BridgeState, BridgeType};
    use tauri::Manager;

    let changed = last.map(|v| v != level).unwrap_or(true);
    *last = Some(level);
    if let Some(state) = app.try_state::<BridgeState>() {
        state.update_device_info(BridgeType::Xiaomi, None, None, Some(level));
    }
    crate::bridges::emit_device_status(app, BridgeType::Xiaomi);
    if force_log || changed {
        emit_message(app, &format!("电量 {level}%"));
        log::info!("XIAOMI BATTERY level={level}%");
    }
}

#[cfg(target_os = "windows")]
fn setup_battery_monitor(
    app: &AppHandle,
    service: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
) -> Result<
    (
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        Option<windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic>,
    ),
    String,
> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattOpenStatus, GattSharingMode,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;
    use tauri::Manager;

    match service.OpenAsync(GattSharingMode::SharedReadOnly) {
        Ok(op) => match op.get() {
            Ok(status)
                if status == GattOpenStatus::Success
                    || status == GattOpenStatus::AlreadyOpened => {}
            Ok(status) => log::warn!("XIAOMI BATTERY OpenAsync status={status:?}"),
            Err(e) => log::warn!("XIAOMI BATTERY OpenAsync: {e}"),
        },
        Err(e) => log::warn!("XIAOMI BATTERY OpenAsync unavailable: {e}"),
    }

    let level_guid = GUID::from_u128(BATTERY_LEVEL);
    let result = service
        .GetCharacteristicsForUuidWithCacheModeAsync(level_guid, BluetoothCacheMode::Uncached)
        .map_err(|e| format!("Battery GetCharacteristics: {e}"))?
        .get()
        .map_err(|e| format!("Battery GetCharacteristics get: {e}"))?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err(format!("Battery characteristics status={:?}", result.Status()));
    }
    let chars = result
        .Characteristics()
        .map_err(|e| format!("Battery Characteristics: {e}"))?;
    if chars.Size().unwrap_or(0) == 0 {
        return Err("Battery Level characteristic missing".into());
    }
    let ch = chars
        .GetAt(0)
        .map_err(|e| format!("Battery GetAt: {e}"))?;

    let status_guid = GUID::from_u128(BATTERY_LEVEL_STATUS);
    let status_ch = match service
        .GetCharacteristicsForUuidWithCacheModeAsync(status_guid, BluetoothCacheMode::Uncached)
        .and_then(|op| op.get())
    {
        Ok(result) if result.Status().ok() == Some(GattCommunicationStatus::Success) => result
            .Characteristics()
            .ok()
            .and_then(|chars| (chars.Size().unwrap_or(0) > 0).then_some(chars))
            .and_then(|chars| chars.GetAt(0).ok()),
        Ok(result) => {
            log::info!(
                "XIAOMI BATTERY status characteristic unavailable status={:?}",
                result.Status()
            );
            None
        }
        Err(e) => {
            log::info!("XIAOMI BATTERY status characteristic unavailable: {e}");
            None
        }
    };

    // 通知：电量变化时刷新 UI（可选，失败仍可轮询读）
    let app2 = app.clone();
    let handler = TypedEventHandler::new(
        move |_sender: &Option<GattCharacteristic>,
              args: &Option<
            windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
        >| {
            if let Some(args) = args {
                if let Ok(buf) = args.CharacteristicValue() {
                    if let Ok(reader) = DataReader::FromBuffer(&buf) {
                        let len = reader.UnconsumedBufferLength().unwrap_or(0);
                        if len > 0 {
                            let mut data = [0u8; 1];
                            if reader.ReadBytes(&mut data).is_ok() {
                                let level = data[0].min(100);
                                if let Some(state) = app2.try_state::<crate::bridges::BridgeState>()
                                {
                                    state.update_device_info(
                                        crate::bridges::BridgeType::Xiaomi,
                                        None,
                                        None,
                                        Some(level),
                                    );
                                }
                                crate::bridges::emit_device_status(
                                    &app2,
                                    crate::bridges::BridgeType::Xiaomi,
                                );
                                emit_message(&app2, &format!("电量 {level}%"));
                                log::info!("XIAOMI BATTERY notify level={level}%");
                            }
                        }
                    }
                }
            }
            Ok(())
        },
    );
    if let Ok(token) = ch.ValueChanged(&handler) {
        let cccd_ok = ch
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .and_then(|op| op.get())
            .map(|s| s == GattCommunicationStatus::Success)
            .unwrap_or(false);
        if cccd_ok {
            tokens.push((ch.clone(), token));
            log::info!("XIAOMI BATTERY notify subscribed");
        } else {
            let _ = ch.RemoveValueChanged(token);
            log::info!("XIAOMI BATTERY notify unsupported; will poll");
        }
    }

    if let Some(status_ch) = status_ch.as_ref() {
        subscribe_battery_status_notify(app, status_ch, tokens);
    } else {
        log::info!(
            "XIAOMI BATTERY status characteristic 0x2BED not found; using percentage-only fallback"
        );
    }

    Ok((ch, status_ch))
}

#[cfg(target_os = "windows")]
fn subscribe_battery_status_notify(
    app: &AppHandle,
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattValueChangedEventArgs,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;

    let app2 = app.clone();
    let handler = TypedEventHandler::new(
        move |_sender: &Option<GattCharacteristic>, args: &Option<GattValueChangedEventArgs>| {
            if let Some(args) = args {
                if let Ok(buf) = args.CharacteristicValue() {
                    if let Ok(reader) = DataReader::FromBuffer(&buf) {
                        let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                        let mut data = vec![0u8; len];
                        if reader.ReadBytes(&mut data).is_ok() {
                            if let Some(charging) = parse_battery_charging_state(&data) {
                                if let Some(state) =
                                    app2.try_state::<crate::bridges::BridgeState>()
                                {
                                    state.update_battery_charging(
                                        crate::bridges::BridgeType::Xiaomi,
                                        charging.is_charging(),
                                    );
                                }
                                crate::bridges::emit_device_status(
                                    &app2,
                                    crate::bridges::BridgeType::Xiaomi,
                                );
                                log::info!(
                                    "XIAOMI BATTERY charge_state notify={}",
                                    charging.label()
                                );
                            }
                        }
                    }
                }
            }
            Ok(())
        },
    );
    if let Ok(token) = ch.ValueChanged(&handler) {
        let cccd_ok = ch
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .and_then(|op| op.get())
            .map(|s| s == GattCommunicationStatus::Success)
            .unwrap_or(false);
        if cccd_ok {
            tokens.push((ch.clone(), token));
            log::info!("XIAOMI BATTERY charge-state notify subscribed");
        } else {
            let _ = ch.RemoveValueChanged(token);
            log::info!("XIAOMI BATTERY charge-state notify unsupported; will poll");
        }
    }
}

#[cfg(target_os = "windows")]
fn read_battery_charging_status(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
) -> Option<BatteryChargingState> {
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    use windows::Storage::Streams::DataReader;

    let result = ch
        .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .ok()?
        .get()
        .ok()?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return None;
    }
    let buf = result.Value().ok()?;
    let reader = DataReader::FromBuffer(&buf).ok()?;
    let len = reader.UnconsumedBufferLength().ok()? as usize;
    let mut data = vec![0u8; len];
    reader.ReadBytes(&mut data).ok()?;
    parse_battery_charging_state(&data)
}

/// Parses BAS 1.1 Battery Level Status (0x2BED). The first byte is Flags, followed
/// by a little-endian 16-bit Power State whose bits 5..=6 are Battery Charge State.
fn parse_battery_charging_state(payload: &[u8]) -> Option<BatteryChargingState> {
    // A Battery Level Status value always contains one-byte Flags and two-byte Power State.
    if payload.len() < 3 {
        return None;
    }
    let power_state = u16::from_le_bytes([payload[1], payload[2]]);
    match (power_state >> 5) & 0b11 {
        0 => Some(BatteryChargingState::Unknown),
        1 => Some(BatteryChargingState::Charging),
        2 => Some(BatteryChargingState::DischargingActive),
        3 => Some(BatteryChargingState::DischargingInactive),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn publish_battery_charging(
    app: &AppHandle,
    charging: BatteryChargingState,
    last: &mut Option<BatteryChargingState>,
    force_log: bool,
) {
    let changed = last.map(|value| value != charging).unwrap_or(true);
    *last = Some(charging);
    if let Some(state) = app.try_state::<crate::bridges::BridgeState>() {
        state.update_battery_charging(
            crate::bridges::BridgeType::Xiaomi,
            charging.is_charging(),
        );
    }
    crate::bridges::emit_device_status(app, crate::bridges::BridgeType::Xiaomi);
    if force_log || changed {
        log::info!("XIAOMI BATTERY charge_state={}", charging.label());
    }
}

#[cfg(target_os = "windows")]
fn read_battery_level(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
) -> Option<u8> {
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    use windows::Storage::Streams::DataReader;

    let result = ch
        .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .ok()?
        .get()
        .ok()?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return None;
    }
    let buf = result.Value().ok()?;
    let reader = DataReader::FromBuffer(&buf).ok()?;
    let len = reader.UnconsumedBufferLength().unwrap_or(0);
    if len == 0 {
        return None;
    }
    let mut data = [0u8; 1];
    reader.ReadBytes(&mut data).ok()?;
    Some(data[0].min(100))
}

/// Windows HID 关闭时的可选 GATT HID fallback（默认不调用）
#[cfg(target_os = "windows")]
fn try_subscribe_gatt_hid(
    app: &AppHandle,
    hid: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    gate: &Arc<KeyEmitGate>,
    active_usages: &Arc<Mutex<HashSet<u16>>>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    hid_ok: &mut bool,
    protocol_guid: windows::core::GUID,
    control_point_guid: windows::core::GUID,
    report_guid: windows::core::GUID,
    report_ref_guid: windows::core::GUID,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattCharacteristicProperties,
        GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
        GattSharingMode,
    };
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;

    // 对齐 Python：只用 SharedReadOnly；绝不 SharedReadAndWrite（会抢占）
    if let Err(e) = hid
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
    {
        log::warn!("HID DIRECT open SharedReadOnly failed: {e}");
        emit_message(app, "GATT HID 无法 SharedReadOnly（Windows HID 可能独占）");
        return;
    }

    match hid.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached) {
        Ok(op) => match op.get() {
            Ok(chars_result)
                if chars_result.Status().ok() == Some(GattCommunicationStatus::Success) =>
            {
                if let Ok(chars) = chars_result.Characteristics() {
                    let n = chars.Size().unwrap_or(0);
                    for i in 0..n {
                        let Ok(ch) = chars.GetAt(i) else { continue };
                        let Ok(uuid) = ch.Uuid() else { continue };
                        let props = ch
                            .CharacteristicProperties()
                            .unwrap_or(GattCharacteristicProperties(0));

                        if uuid == protocol_guid
                            && (props.contains(GattCharacteristicProperties::Write)
                                || props.contains(
                                    GattCharacteristicProperties::WriteWithoutResponse,
                                ))
                        {
                            write_gatt_byte(&ch, 1, "protocol_report_mode");
                            continue;
                        }
                        if uuid == control_point_guid
                            && (props.contains(GattCharacteristicProperties::Write)
                                || props.contains(
                                    GattCharacteristicProperties::WriteWithoutResponse,
                                ))
                        {
                            write_gatt_byte(&ch, 1, "exit_suspend");
                            continue;
                        }

                        if uuid != report_guid {
                            continue;
                        }
                        let can_notify = props.contains(GattCharacteristicProperties::Notify)
                            || props.contains(GattCharacteristicProperties::Indicate);
                        if !can_notify {
                            continue;
                        }

                        let (report_id, report_type) =
                            read_report_reference(&ch, report_ref_guid);
                        if report_type != 0 && report_type != 1 {
                            continue;
                        }

                        let app2 = app.clone();
                        let usages2 = Arc::clone(active_usages);
                        let gate2 = Arc::clone(gate);
                        let handler = TypedEventHandler::new(
                            move |_sender: &Option<GattCharacteristic>,
                                  args: &Option<
                                windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
                            >| {
                                if let Some(args) = args {
                                    if let Ok(buf) = args.CharacteristicValue() {
                                        if let Ok(reader) = DataReader::FromBuffer(&buf) {
                                            let len = reader
                                                .UnconsumedBufferLength()
                                                .unwrap_or(0)
                                                as usize;
                                            let mut data = vec![0u8; len];
                                            let _ = reader.ReadBytes(&mut data);
                                            handle_hid_payload(
                                                &app2, &usages2, &gate2, &data,
                                            );
                                        }
                                    }
                                }
                                Ok(())
                            },
                        );

                        let cccd = if props.contains(GattCharacteristicProperties::Notify) {
                            GattClientCharacteristicConfigurationDescriptorValue::Notify
                        } else {
                            GattClientCharacteristicConfigurationDescriptorValue::Indicate
                        };

                        if let Ok(token) = ch.ValueChanged(&handler) {
                            match ch
                                .WriteClientCharacteristicConfigurationDescriptorAsync(cccd)
                                .and_then(|op| op.get())
                            {
                                Ok(status) if status == GattCommunicationStatus::Success => {
                                    tokens.push((ch.clone(), token));
                                    *hid_ok = true;
                                    log::info!(
                                        "Subscribed HID report id={report_id} type={report_type}"
                                    );
                                }
                                Ok(status) => {
                                    let _ = ch.RemoveValueChanged(token);
                                    log::warn!("HID CCCD write failed status={status:?}");
                                }
                                Err(e) => {
                                    let _ = ch.RemoveValueChanged(token);
                                    log::warn!("HID CCCD write error: {e}");
                                }
                            }
                        }
                    }
                }
                if !*hid_ok {
                    log::warn!("HID DIRECT unavailable no_input_reports");
                    let _ = hid.Close();
                }
            }
            Ok(_) => {
                log::warn!(
                    "HID DIRECT unavailable characteristics_status; windows_hid_active=true"
                );
                let _ = hid.Close();
            }
            Err(e) => log::warn!("HID GetCharacteristics failed: {e}"),
        },
        Err(e) => log::warn!("HID GetCharacteristicsAsync failed: {e}"),
    }
}

/// ATVV 订阅失败分类（写入日志 / UI；便于区分可自愈与需用户操作）
#[derive(Debug, Clone)]
struct AtvvFailReason {
    /// 机器可读：access_denied / unreachable / protocol_error / fromid_null / …
    code: &'static str,
    /// 短中文标签
    label: &'static str,
    /// 处理建议
    hint: &'static str,
    /// 后台重试/重连是否可能恢复
    recoverable: bool,
}

impl AtvvFailReason {
    fn unknown() -> Self {
        Self {
            code: "unknown",
            label: "未知错误",
            hint: "查看 app.log 中 ATVV FAIL 行",
            recoverable: true,
        }
    }

    fn service_missing() -> Self {
        Self {
            code: "service_missing",
            label: "设备上未发现 ATVV 服务",
            hint: "确认已配对小米 2 Pro，并靠近电脑后重连",
            recoverable: false,
        }
    }

    fn chars_incomplete() -> Self {
        Self {
            code: "chars_incomplete",
            label: "ATVV 特征不完整（缺 Control）",
            hint: "固件/缓存异常，尝试断开蓝牙后重连",
            recoverable: true,
        }
    }

    fn from_error(err: &str) -> Self {
        let lower = err.to_ascii_lowercase();
        // Windows GattCommunicationStatus: Success=0 Unreachable=1 ProtocolError=2 AccessDenied=3
        if err.contains("GattCommunicationStatus(3)")
            || lower.contains("accessdenied")
            || lower.contains("access denied")
        {
            return Self {
                code: "access_denied",
                label: "GATT 拒绝访问（特征被占用）",
                hint: "常见于 HID Tap/WUDFHost 抢占；软件会先停 Tap 再订、并后台重试",
                recoverable: true,
            };
        }
        if err.contains("GattCommunicationStatus(1)") || lower.contains("unreachable") {
            return Self {
                code: "unreachable",
                label: "遥控器 GATT 不可达",
                hint: "请靠近电脑、确认遥控器未休眠后重连",
                recoverable: true,
            };
        }
        if err.contains("GattCommunicationStatus(2)") || lower.contains("protocolerror") {
            return Self {
                code: "protocol_error",
                label: "GATT 协议错误",
                hint: "链路抖动；软件会重试，仍失败请重连",
                recoverable: true,
            };
        }
        if lower.contains("fromid") && (err.contains("0x00000000") || lower.contains("null") || err.contains("操作成功完成"))
        {
            return Self {
                code: "fromid_null",
                label: "FromId 返回空服务对象",
                hint: "接口路径失效或服务未就绪；会改走地址路径并重试",
                recoverable: true,
            };
        }
        if lower.contains("cccd") {
            return Self {
                code: "cccd_failed",
                label: "无法写入 Notify（CCCD）",
                hint: "通知订阅被拒，多与 AccessDenied 同类；后台会重试",
                recoverable: true,
            };
        }
        if lower.contains("getcharacteristics") {
            return Self {
                code: "get_chars_failed",
                label: "读取 ATVV 特征失败",
                hint: "见具体 GattCommunicationStatus；软件已做 Uncached→Cached 回退",
                recoverable: true,
            };
        }
        Self {
            code: "other",
            label: "ATVV 订阅失败",
            hint: "详见日志原文",
            recoverable: true,
        }
    }
}

fn log_atvv_fail(path: &str, reason: &AtvvFailReason, attempt: u32) {
    log::warn!(
        "ATVV FAIL path={path} attempt={attempt} code={} recoverable={} label={} hint={}",
        reason.code,
        reason.recoverable,
        reason.label,
        reason.hint
    );
}

/// 后台恢复时若 Windows 报 AccessDenied，暂停 HID Tap 后只重试一次。
/// 这比多个会话各自无限重订阅更安全，也保留失败时的返回/音量通道。
#[cfg(target_os = "windows")]
fn subscribe_atvv_periodic_retry(
    app: &AppHandle,
    atvv: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    gate: &Arc<KeyEmitGate>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    gain_db: f32,
    runtime: &Arc<XiaomiRuntime>,
    session_id: u64,
) -> Result<bool, String> {
    let first = subscribe_atvv_service(app, atvv, gate, tokens, gain_db);
    let access_denied = first
        .as_ref()
        .err()
        .map(|e| AtvvFailReason::from_error(e).code == "access_denied")
        .unwrap_or(false);
    if !access_denied || !crate::bridges::xiaomi::hid_report_tap::is_running() {
        return first;
    }

    log::warn!("ATVV periodic retry pausing HID Tap session={session_id} after AccessDenied");
    crate::bridges::xiaomi::hid_report_tap::stop_and_join();
    std::thread::sleep(Duration::from_millis(150));
    let retry = subscribe_atvv_service(app, atvv, gate, tokens, gain_db);

    if runtime.session_active(session_id) {
        let tap_enabled = app
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("xiaomi").ok())
            .map(|c| c.hid_report_tap_enabled)
            .unwrap_or(true);
        if tap_enabled {
            let _ = crate::bridges::xiaomi::hid_report_tap::ensure_started(
                app.clone(),
                Arc::clone(gate),
            );
        }
    }
    retry
}

#[cfg(target_os = "windows")]
fn describe_gatt_comm_status(
    status: Option<windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus>,
) -> &'static str {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    match status {
        Some(GattCommunicationStatus::Success) => "Success(0)",
        Some(GattCommunicationStatus::Unreachable) => "Unreachable(1)=遥控器不可达",
        Some(GattCommunicationStatus::ProtocolError) => "ProtocolError(2)=协议错误",
        Some(GattCommunicationStatus::AccessDenied) => "AccessDenied(3)=特征被占用/拒绝访问",
        Some(_) => "UnknownStatus",
        None => "StatusUnavailable",
    }
}

#[cfg(target_os = "windows")]
fn subscribe_atvv_from_interface(
    app: &AppHandle,
    interface_id: &str,
    gate: &Arc<KeyEmitGate>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    gain_db: f32,
) -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattDeviceService, GattSharingMode,
    };

    let id = HSTRING::from(interface_id);
    let service = GattDeviceService::FromIdAsync(&id)
        .map_err(|e| format!("ATVV FromIdAsync: {e}"))?
        .get()
        .map_err(|e| format!("ATVV FromId get: {e}"))?;

    // 对齐 v1.3.3：FromId 后显式 Open（SharedReadOnly 优先）
    let _ = service
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
        .or_else(|_| {
            service
                .OpenAsync(GattSharingMode::SharedReadAndWrite)
                .and_then(|op| op.get())
        });

    subscribe_atvv_service(app, &service, gate, tokens, gain_db)
}

/// ATVV 语音会话共享状态
struct AtvvVoiceState {
    decoder: crate::bridges::xiaomi::adpcm_decoder::AdpcmDecoder,
    streaming: bool,
    pending: Vec<u8>,
    frame_size: usize,
    pending_sync: Option<(i32, i32)>,
    last_mic_off: Option<Instant>,
    gain_db: f32,
    frames: u64,
    /// 当前 ATVV 流的 ADPCM 源采样率。协议默认/旧设备兼容 16kHz。
    sample_rate_hz: u32,
    /// 显式报告未知 codec 时阻止错误速率的音频进入虚拟声卡。
    codec_supported: bool,
    /// 遥控语音键当前是否按下
    remote_pressed: bool,
    /// 按下时刻（点击模式区分短按/长按）
    press_at: Option<Instant>,
    /// 已发出语音快捷键 DOWN；保留字段以兼容现有会话诊断。
    hold_chord_armed: bool,
    /// AUDIO_START 时固定的 Click 最短按住时间，配置变化不得影响本次释放。
    minimum_press_ms: u64,
    /// 取消过期的「长按判定」定时器
    press_gen: u64,
    /// Monotonic per-session timing, emitted once on AUDIO_STOP.
    audio_start_at: Option<Instant>,
    shortcut_ready_at: Option<Instant>,
    injection_route: String,
    first_audio_at: Option<Instant>,
    first_decoded_at: Option<Instant>,
}

const ATVV_LEGACY_SAMPLE_RATE_HZ: u32 = 16_000;

fn atvv_codec_sample_rate(codec: u8) -> Option<u32> {
    match codec {
        // 部分旧固件把“未声明编码”填为 0；按历史默认值继续以 16kHz 处理。
        0x00 => Some(16_000),
        0x01 => Some(8_000),
        0x02 => Some(16_000),
        _ => None,
    }
}

fn update_atvv_codec(state: &Arc<Mutex<AtvvVoiceState>>, codec: Option<u8>, source: &str) {
    let Ok(mut st) = state.lock() else {
        return;
    };
    match codec {
        Some(codec) => match atvv_codec_sample_rate(codec) {
            Some(rate) => {
                let changed = st.sample_rate_hz != rate || !st.codec_supported;
                st.sample_rate_hz = rate;
                st.codec_supported = true;
                if changed {
                    st.pending.clear();
                    log::info!("XIAOMI ATVV {source} codec=0x{codec:02X} sample_rate={rate}Hz");
                }
            }
            None => {
                st.codec_supported = false;
                st.pending.clear();
                log::warn!("XIAOMI ATVV {source} unsupported codec=0x{codec:02X}; audio muted");
            }
        },
        None => {
            st.sample_rate_hz = ATVV_LEGACY_SAMPLE_RATE_HZ;
            st.codec_supported = true;
            log::debug!("XIAOMI ATVV {source} codec missing; fallback=16000Hz");
        }
    }
}

fn voice_trigger_is_toggle(app: &AppHandle) -> bool {
    app.try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok())
        .map(|c| matches!(c.trigger_mode, TriggerMode::Toggle))
        .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn atvv_write_tx(
    tx: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    bytes: &[u8],
    label: &str,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattWriteOption;
    use windows::Storage::Streams::DataWriter;
    if let Ok(writer) = DataWriter::new() {
        if writer.WriteBytes(bytes).is_ok() {
            if let Ok(buf) = writer.DetachBuffer() {
                let _ = tx.WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse);
                log::info!("ATVV {label} sent");
            }
        }
    }
}

fn notify_voice_phase(app: &AppHandle, gate: &KeyEmitGate, pressed: bool) {
    if pressed {
        let _ = gate.try_emit("mic");
    }
    emit_key_phase(app, "mic", button_label("mic"), pressed);
}

fn reset_pcm_session(state: &Arc<Mutex<AtvvVoiceState>>, clear_frames: bool) {
    use crate::bridges::xiaomi::voice_pcm;
    if let Ok(mut st) = state.lock() {
        st.streaming = true;
        st.pending.clear();
        st.decoder.reset_with(0, 0);
        st.pending_sync = None;
        st.last_mic_off = None;
        if clear_frames {
            st.frames = 0;
            st.audio_start_at = Some(Instant::now());
            st.shortcut_ready_at = None;
            st.injection_route.clear();
            st.first_audio_at = None;
            st.first_decoded_at = None;
        }
    }
    voice_pcm::begin_session();
}

fn mark_voice_shortcut_ready(state: &Arc<Mutex<AtvvVoiceState>>, route: Option<&str>) {
    let route = match route {
        Some(route) => route,
        None => key_mapping::current_voice_route_label(),
    };
    if let Ok(mut st) = state.lock() {
        st.shortcut_ready_at.get_or_insert_with(Instant::now);
        st.injection_route = route.to_string();
    }
    crate::bridges::xiaomi::voice_pcm::release_input_gate();
}

fn log_voice_timing(state: &Arc<Mutex<AtvvVoiceState>>, phase: &str) {
    let Ok(st) = state.lock() else { return };
    let Some(start) = st.audio_start_at else { return };
    let ms = |at: Option<Instant>| at.map(|at| at.duration_since(start).as_millis());
    let (pre_roll_ms, dropped_ms) = crate::bridges::xiaomi::voice_pcm::pre_roll_stats();
    let sent = crate::bridges::xiaomi::voice_pcm::first_send_at()
        .map(|at| at.duration_since(start).as_millis());
    log::info!(
        "XIAOMI VOICE LATENCY phase={phase} route={} key_ms={:?} first_audio_ms={:?} first_decode_ms={:?} first_udp_ms={sent:?} preroll_ms={pre_roll_ms} dropped_preroll_ms={dropped_ms}",
        st.injection_route,
        ms(st.shortcut_ready_at),
        ms(st.first_audio_at),
        ms(st.first_decoded_at),
    );
}

/// 遥控语音键按下：传声 + 按模式注入快捷键
fn on_voice_remote_press(app: &AppHandle, gate: &KeyEmitGate, state: &Arc<Mutex<AtvvVoiceState>>) {
    let toggle = voice_trigger_is_toggle(app);
    let minimum_press_ms = if toggle {
        key_mapping::voice_shortcut_min_hold_ms(app)
    } else {
        0
    };
    {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if st.remote_pressed {
            return;
        }
        st.remote_pressed = true;
        st.press_at = Some(Instant::now());
        st.hold_chord_armed = true;
        st.minimum_press_ms = minimum_press_ms;
        st.press_gen = st.press_gen.wrapping_add(1);
    }

    reset_pcm_session(state, true);
    notify_voice_phase(app, gate, true);
    if !crate::bridges::xiaomi::voice_pcm::is_ready() {
        crate::bridges::xiaomi::voice_pcm::warmup_async();
    }
    crate::bridges::xiaomi::voice_meter::set_session(true);

    // Both modes must activate the IME as soon as AUDIO_START arrives.  Click
    // keeps a minimum press duration on release; it no longer delays the
    // floating voice bar until the remote button is released.
    key_mapping::on_remote_button(app, "mic", true);
    mark_voice_shortcut_ready(state, None);
    log::info!(
        "XIAOMI ATVV AUDIO_START mode={} → shortcut DOWN immediately",
        if toggle { "click" } else { "hold" }
    );
}

/// 遥控语音键抬起：结束传声 + 短按 TAP / 长按 UP
fn on_voice_remote_release(app: &AppHandle, gate: &KeyEmitGate, state: &Arc<Mutex<AtvvVoiceState>>) {
    use crate::bridges::xiaomi::voice_pcm;
    let toggle = voice_trigger_is_toggle(app);
    let (was_pressed, press_ms, minimum_press_ms) = {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if !st.remote_pressed {
            return;
        }
        let ms = st
            .press_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        st.remote_pressed = false;
        st.press_at = None;
        st.hold_chord_armed = false;
        st.press_gen = st.press_gen.wrapping_add(1); // 作废阈值定时器
        st.streaming = false;
        st.last_mic_off = Some(Instant::now());
        st.pending.clear();
        (true, ms, st.minimum_press_ms)
    };
    if !was_pressed {
        return;
    }

    notify_voice_phase(app, gate, false);

    if toggle {
        if press_ms < minimum_press_ms {
            std::thread::sleep(Duration::from_millis(minimum_press_ms - press_ms));
        }
    }
    std::thread::sleep(Duration::from_millis(40));
    voice_pcm::end_session();
    key_mapping::on_remote_button(app, "mic", false);
    log::info!(
        "XIAOMI ATVV AUDIO_STOP mode={} release ms={press_ms}",
        if toggle { "click" } else { "hold" }
    );

    log_voice_timing(state, if toggle { "click" } else { "hold" });

    crate::bridges::xiaomi::key_mapping::disarm_voice_native_suppress();
    crate::bridges::xiaomi::voice_meter::set_session(false);
}

#[cfg(target_os = "windows")]
fn subscribe_atvv_service(
    app: &AppHandle,
    atvv: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    gate: &Arc<KeyEmitGate>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    gain_db: f32,
) -> Result<bool, String> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattSharingMode, GattWriteOption,
    };
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::{DataReader, DataWriter};

    // 对齐 v1.3.3：订阅前 Open（SharedReadOnly 优先；Exclusive 仅作最后手段）
    let _ = atvv
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
        .or_else(|_| {
            atvv.OpenAsync(GattSharingMode::SharedReadAndWrite)
                .and_then(|op| op.get())
        })
        .or_else(|_| {
            atvv.OpenAsync(GattSharingMode::Exclusive)
                .and_then(|op| op.get())
        });

    let tx_guid = GUID::from_u128(ATVV_TX);
    let audio_guid = GUID::from_u128(ATVV_AUDIO);
    let atvv_control_guid = GUID::from_u128(ATVV_CONTROL);

    let chars_result = atvv
        .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let chars_result = if chars_result.Status().ok() == Some(GattCommunicationStatus::Success) {
        chars_result
    } else {
        log::warn!(
            "ATVV GetCharacteristics uncached status={}, retry cached",
            describe_gatt_comm_status(chars_result.Status().ok())
        );
        atvv
            .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Cached)
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?
    };
    if chars_result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err(format!(
            "ATVV GetCharacteristics status failed: {}",
            describe_gatt_comm_status(chars_result.Status().ok())
        ));
    }
    let chars = chars_result.Characteristics().map_err(|e| e.to_string())?;
    let n = chars.Size().unwrap_or(0);
    let mut tx: Option<GattCharacteristic> = None;
    let mut audio: Option<GattCharacteristic> = None;
    let mut control: Option<GattCharacteristic> = None;
    for i in 0..n {
        let Ok(ch) = chars.GetAt(i) else { continue };
        let Ok(uuid) = ch.Uuid() else { continue };
        if uuid == tx_guid {
            tx = Some(ch);
        } else if uuid == audio_guid {
            audio = Some(ch);
        } else if uuid == atvv_control_guid {
            control = Some(ch);
        }
    }

    let Some(control) = control else {
        return Ok(false);
    };

    let voice_state = Arc::new(Mutex::new(AtvvVoiceState {
        decoder: crate::bridges::xiaomi::adpcm_decoder::AdpcmDecoder::new_ima(),
        streaming: false,
        pending: Vec::new(),
        frame_size: 120,
        pending_sync: None,
        last_mic_off: None,
        gain_db,
        frames: 0,
        sample_rate_hz: ATVV_LEGACY_SAMPLE_RATE_HZ,
        codec_supported: true,
        remote_pressed: false,
        press_at: None,
        hold_chord_armed: false,
        minimum_press_ms: 0,
        press_gen: 0,
        audio_start_at: None,
        shortcut_ready_at: None,
        injection_route: String::new(),
        first_audio_at: None,
        first_decoded_at: None,
    }));

    let app2 = app.clone();
    let gate2 = Arc::clone(gate);
    let tx_for_mic = tx.clone();
    let voice_ctrl = Arc::clone(&voice_state);
    let handler = TypedEventHandler::new(
        move |_sender: &Option<GattCharacteristic>,
              args: &Option<
            windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
        >| {
            if let Some(args) = args {
                if let Ok(buf) = args.CharacteristicValue() {
                    if let Ok(reader) = DataReader::FromBuffer(&buf) {
                        let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                        let mut data = vec![0u8; len];
                        let _ = reader.ReadBytes(&mut data);
                        handle_atvv_control(
                            &app2,
                            &gate2,
                            &voice_ctrl,
                            tx_for_mic.as_ref(),
                            &data,
                        );
                    }
                }
            }
            Ok(())
        },
    );

    let token = control
        .ValueChanged(&handler)
        .map_err(|e| format!("ATVV ValueChanged: {e}"))?;
    let cccd_status = control
        .WriteClientCharacteristicConfigurationDescriptorAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )
        .and_then(|op| op.get());
    let cccd_ok = matches!(cccd_status, Ok(GattCommunicationStatus::Success));
    if !cccd_ok {
        let _ = control.RemoveValueChanged(token);
        return Err(format!(
            "ATVV CCCD notify failed: {}",
            describe_gatt_comm_status(cccd_status.ok())
        ));
    }
    tokens.push((control.clone(), token));
    log::info!("Subscribed ATVV control characteristic");

    // 订阅 AUDIO 特征 → ADPCM → VB-CABLE
    if let Some(audio_ch) = audio {
        let voice_audio = Arc::clone(&voice_state);
        let audio_handler = TypedEventHandler::new(
            move |_sender: &Option<GattCharacteristic>,
                  args: &Option<
                windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
            >| {
                if let Some(args) = args {
                    if let Ok(buf) = args.CharacteristicValue() {
                        if let Ok(reader) = DataReader::FromBuffer(&buf) {
                            let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                            let mut data = vec![0u8; len];
                            let _ = reader.ReadBytes(&mut data);
                            handle_atvv_audio(&voice_audio, &data);
                        }
                    }
                }
                Ok(())
            },
        );
        if let Ok(audio_token) = audio_ch.ValueChanged(&audio_handler) {
            let audio_cccd = audio_ch
                .WriteClientCharacteristicConfigurationDescriptorAsync(
                    GattClientCharacteristicConfigurationDescriptorValue::Notify,
                )
                .and_then(|op| op.get())
                .map(|s| s == GattCommunicationStatus::Success)
                .unwrap_or(false);
            if audio_cccd {
                tokens.push((audio_ch.clone(), audio_token));
                log::info!("Subscribed ATVV audio characteristic");
                emit_message(app, "ATVV 麦克风音频已订阅 → VB-CABLE");
            } else {
                let _ = audio_ch.RemoveValueChanged(audio_token);
                log::warn!("ATVV audio CCCD failed");
            }
        }
    } else {
        log::warn!("ATVV audio characteristic not found");
    }

    if let Some(tx) = tx {
        if let Ok(writer) = DataWriter::new() {
            if writer.WriteBytes(&GET_CAPS_V10).is_ok() {
                if let Ok(buf) = writer.DetachBuffer() {
                    let _ = tx
                        .WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse)
                        .and_then(|op| op.get());
                    log::info!("ATVV GET_CAPS sent");
                }
            }
        }
    }
    Ok(true)
}

fn handle_atvv_audio(state: &Arc<Mutex<AtvvVoiceState>>, payload: &[u8]) {
    use crate::bridges::xiaomi::adpcm_decoder::postprocess;
    use crate::bridges::xiaomi::voice_pcm;

    // Keep decoder mutation and frame assembly serialized, but move gain,
    // resampling and UDP work outside this lock so an AUDIO_STOP/AUDIO_START
    // control packet is never queued behind PCM processing.
    let decoded = {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if !st.codec_supported {
            return;
        }
        st.first_audio_at.get_or_insert_with(Instant::now);
        if !st.streaming {
            // 按键已按下但 streaming 尚未置位时，音频首帧可直接入流
            if st.remote_pressed {
                st.streaming = true;
                st.pending.clear();
            } else if let Some(t) = st.last_mic_off {
                if t.elapsed() < Duration::from_millis(300) {
                    return;
                }
                st.streaming = true;
                st.pending.clear();
                voice_pcm::clear();
                log::info!("XIAOMI ATVV MIC ON session=implicit_audio_race");
            } else {
                st.streaming = true;
                st.pending.clear();
                voice_pcm::clear();
                log::info!("XIAOMI ATVV MIC ON session=implicit_audio_race");
            }
        }
        st.pending.extend_from_slice(payload);
        let mut out = Vec::new();
        while st.pending.len() >= st.frame_size {
            let frame_size = st.frame_size;
            let frame: Vec<u8> = st.pending.drain(..frame_size).collect();
            if let Some((pred, idx)) = st.pending_sync.take() {
                st.decoder.reset_with(pred, idx);
            }
            let samples = st.decoder.decode_bytes(&frame);
            st.first_decoded_at.get_or_insert_with(Instant::now);
            st.frames += 1;
            out.push((samples, st.gain_db, st.sample_rate_hz, st.frames));
        }
        out
    };

    for (samples, gain_db, sample_rate_hz, frames) in decoded {
        let samples = postprocess(&samples, gain_db);
        voice_pcm::push_pcm(&samples, sample_rate_hz);
        if frames == 1 || frames == 10 || frames % 200 == 0 {
            let (sent, drop) = voice_pcm::stats();
            log::debug!("XIAOMI ATVV AUDIO frames={frames} sent={sent} drop={drop}");
        }
    }
}

fn handle_atvv_control(
    app: &AppHandle,
    gate: &KeyEmitGate,
    state: &Arc<Mutex<AtvvVoiceState>>,
    tx: Option<&windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic>,
    payload: &[u8],
) {
    if payload.is_empty() {
        return;
    }
    match payload[0] {
        0x08 => {
            key_mapping::mark_direct_signal("voice");
            key_mapping::mark_direct_signal("mic");
            if let Some(tx) = tx {
                atvv_write_tx(tx, &[0x0C, 0x00], "MIC_OPEN");
            }
            log::info!("XIAOMI ATVV MIC_OPEN request opcode=0x08");
        }
        0x04 => {
            key_mapping::mark_direct_signal("voice");
            key_mapping::mark_direct_signal("mic");
            // AUDIO_START: [opcode, reason, codec, stream_id].
            update_atvv_codec(state, payload.get(2).copied(), "AUDIO_START");
            on_voice_remote_press(app, gate, state);
        }
        0x00 => {
            on_voice_remote_release(app, gate, state);
        }
        0x0A if payload.len() >= 7 => {
            // AUDIO_SYNC: [opcode, codec, frame_no_hi, frame_no_lo, predictor_hi, predictor_lo, step].
            update_atvv_codec(state, payload.get(1).copied(), "AUDIO_SYNC");
            let predictor = i16::from_be_bytes([payload[4], payload[5]]) as i32;
            let step_index = payload[6] as i32;
            if let Ok(mut st) = state.lock() {
                st.pending.clear();
                st.pending_sync = Some((predictor, step_index));
            }
            log::info!("XIAOMI ATVV AUDIO_SYNC predictor={predictor} step={step_index} codec=0x{:02X}", payload[1]);
        }
        0x0B if payload.len() >= 7 => {
            let frame_size = u16::from_be_bytes([payload[5], payload[6]]) as usize;
            if let Ok(mut st) = state.lock() {
                if frame_size > 0 {
                    st.frame_size = frame_size;
                }
            }
            log::info!("XIAOMI ATVV CAPS received frame_size={frame_size}");
        }
        0x0B => log::info!("XIAOMI ATVV CAPS received"),
        other => log::debug!("XIAOMI ATVV opcode=0x{other:02X}"),
    }
}

#[cfg(target_os = "windows")]
fn write_gatt_byte(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    value: u8,
    label: &str,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattWriteOption;
    use windows::Storage::Streams::DataWriter;

    if let Ok(writer) = DataWriter::new() {
        if writer.WriteBytes(&[value]).is_ok() {
            if let Ok(buf) = writer.DetachBuffer() {
                match ch
                    .WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse)
                    .and_then(|op| op.get())
                {
                    Ok(_) => log::info!("HID write {label}={value}"),
                    Err(e) => log::warn!("HID write {label} failed: {e}"),
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn read_report_reference(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    report_ref_guid: windows::core::GUID,
) -> (u8, u8) {
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    use windows::Storage::Streams::DataReader;

    let Ok(op) = ch.GetDescriptorsWithCacheModeAsync(BluetoothCacheMode::Uncached) else {
        return (0, 0);
    };
    let Ok(result) = op.get() else {
        return (0, 0);
    };
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return (0, 0);
    }
    let Ok(descriptors) = result.Descriptors() else {
        return (0, 0);
    };
    let n = descriptors.Size().unwrap_or(0);
    for i in 0..n {
        let Ok(desc) = descriptors.GetAt(i) else { continue };
        let Ok(uuid) = desc.Uuid() else { continue };
        if uuid != report_ref_guid {
            continue;
        }
        let Ok(read_op) = desc.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached) else {
            continue;
        };
        let Ok(value_result) = read_op.get() else { continue };
        if value_result.Status().ok() != Some(GattCommunicationStatus::Success) {
            continue;
        }
        let Ok(buf) = value_result.Value() else { continue };
        let Ok(reader) = DataReader::FromBuffer(&buf) else { continue };
        let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
        let mut data = vec![0u8; len];
        let _ = reader.ReadBytes(&mut data);
        if data.len() >= 2 {
            return (data[0], data[1]);
        }
    }
    (0, 0)
}

fn handle_hid_payload(
    app: &AppHandle,
    active: &Arc<Mutex<HashSet<u16>>>,
    gate: &KeyEmitGate,
    payload: &[u8],
) {
    let usages = parse_hid_usages(payload);
    let Ok(mut guard) = active.lock() else {
        return;
    };
    let pressed: Vec<u16> = usages.difference(&guard).copied().collect();
    let released: Vec<u16> = guard.difference(&usages).copied().collect();
    *guard = usages;
    drop(guard);

    for usage in pressed {
        let btn = match usage {
            0x00E9 => XiaomiButton::VolumeUp,
            0x00EA => XiaomiButton::VolumeDown,
            0x00E2 => XiaomiButton::Mute,
            other => XiaomiButton::from_hid_usage(other),
        };
        let id = btn.to_button_id();
        if id == "unknown" {
            log::debug!("HID usage 0x{usage:04X} ignored");
            continue;
        }
        if gate.try_emit(id) {
            emit_key_and_map(app, id, button_label(id), true);
        } else {
            key_mapping::on_remote_button(app, id, true);
        }
        log::info!("XIAOMI HID key={id} usage=0x{usage:04X}");
    }
    for usage in released {
        let btn = XiaomiButton::from_hid_usage(usage);
        let id = btn.to_button_id();
        if id != "unknown" {
            emit_key_and_map(app, id, button_label(id), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        atvv_codec_sample_rate, disconnect_confirmed, parse_battery_charging_state, parse_hid_usages,
        BatteryChargingState,
    };

    #[test]
    fn atvv_codec_selects_expected_sample_rate() {
        assert_eq!(atvv_codec_sample_rate(0x01), Some(8_000));
        assert_eq!(atvv_codec_sample_rate(0x02), Some(16_000));
        assert_eq!(atvv_codec_sample_rate(0x00), Some(16_000));
        assert_eq!(atvv_codec_sample_rate(0x03), None);
    }

    #[test]
    fn parse_six_byte_usages() {
        // back=0xF1, vol+=0x80
        let data = [0xF1u8, 0x00, 0x80, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
        assert!(u.contains(&0x0080));
    }

    #[test]
    fn parse_report_id_prefix() {
        let data = [0x01u8, 0xF1, 0x00, 0x81, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
        assert!(u.contains(&0x0081));
    }

    #[test]
    fn parse_hidogatt_prefix() {
        let data = [0x01u8, 0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
    }

    #[test]
    fn parses_battery_level_status_charge_state() {
        // Flags + 16-bit little-endian Power State; bits 5..=6 encode charging state.
        assert_eq!(
            parse_battery_charging_state(&[0x00, 0b0010_0000, 0x00]),
            Some(BatteryChargingState::Charging)
        );
        assert_eq!(
            parse_battery_charging_state(&[0x00, 0b0100_0000, 0x00]),
            Some(BatteryChargingState::DischargingActive)
        );
        assert_eq!(
            parse_battery_charging_state(&[0x00, 0b0110_0000, 0x00]),
            Some(BatteryChargingState::DischargingInactive)
        );
        assert_eq!(
            parse_battery_charging_state(&[0x00, 0x00, 0x00]),
            Some(BatteryChargingState::Unknown)
        );
        assert_eq!(
            parse_battery_charging_state(&[0x06, 0b0010_0000, 0x00, 72, 0x00]),
            Some(BatteryChargingState::Charging)
        );
    }

    #[test]
    fn rejects_truncated_battery_level_status() {
        assert_eq!(parse_battery_charging_state(&[0x00, 0x20]), None);
    }

    #[test]
    fn disconnect_requires_all_debounced_checks() {
        assert!(disconnect_confirmed(true, true, true));
        assert!(!disconnect_confirmed(true, false, true));
        assert!(!disconnect_confirmed(true, true, false));
    }
}
