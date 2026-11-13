//! 对齐 Python `virtual_hid_keyboard.py`：动态加载 WinUHid.dll，优先虚拟 HID，失败回退 SendInput

use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01,
    0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01,
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0x73, 0x05, 0x07, 0x19, 0x00, 0x29, 0x73, 0x81, 0x00, 0xC0,
];

const CONSUMER_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x19, 0x00, 0x2A, 0xFF, 0x03,
    0x75, 0x10, 0x95, 0x01, 0x81, 0x00, 0xC0,
];

#[repr(C, packed)]
struct WinUHidDeviceConfig {
    supported_events: i32,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
    report_descriptor_length: u16,
    report_descriptor: *const u8,
    container_id: [u8; 16],
    instance_id: *const u16,
    hardware_ids: *const u16,
    read_report_period_us: u32,
}

type FnGetVersion = unsafe extern "system" fn() -> u32;
type FnCreateDevice = unsafe extern "system" fn(*const WinUHidDeviceConfig) -> *mut c_void;
type FnStartDevice = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
type FnSubmitReport = unsafe extern "system" fn(*mut c_void, *const u8, u32) -> i32;
type FnDestroyDevice = unsafe extern "system" fn(*mut c_void);

struct Api {
    _module: windows::Win32::Foundation::HMODULE,
    create: FnCreateDevice,
    start: FnStartDevice,
    submit: FnSubmitReport,
    destroy: FnDestroyDevice,
}

struct Devices {
    api: Api,
    keyboard: *mut c_void,
    consumer: *mut c_void,
    owners: ReportOwners,
}

#[cfg(target_os = "windows")]
impl Drop for Devices {
    fn drop(&mut self) {
        unsafe {
            if !self.keyboard.is_null() {
                (self.api.destroy)(self.keyboard);
                self.keyboard = std::ptr::null_mut();
            }
            if !self.consumer.is_null() {
                (self.api.destroy)(self.consumer);
                self.consumer = std::ptr::null_mut();
            }
            let _ = windows::Win32::Foundation::FreeLibrary(self.api._module);
        }
    }
}

// 句柄仅在本模块内通过 Mutex 使用
unsafe impl Send for Devices {}

#[derive(Default, Clone)]
struct ReportOwners(BTreeMap<u64, Vec<u16>>);
impl ReportOwners {
    fn keys(&self) -> Vec<u16> {
        let mut keys: Vec<u16> = self.0.values().flatten().copied().collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
    fn keyboard_report(&self) -> Result<[u8; 8], String> {
        build_keyboard_report(
            &self
                .keys()
                .into_iter()
                .filter(|vk| consumer_usage(*vk).is_none())
                .collect::<Vec<_>>(),
        )
    }
    fn consumer_report(&self) -> Result<[u8; 2], String> {
        let usages: Vec<u16> = self.keys().into_iter().filter_map(consumer_usage).collect();
        if usages.len() > 1 {
            return Err("Overlapping Consumer Control reports unsupported".into());
        }
        Ok(usages.first().copied().unwrap_or(0).to_le_bytes())
    }
}
static INIT_LOCK: Mutex<()> = Mutex::new(());
static NEXT_OWNER: AtomicU64 = AtomicU64::new(2);
pub fn new_owner() -> u64 {
    NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
}

static INIT_TRIED: AtomicBool = AtomicBool::new(false);
static DEVICES: Mutex<Option<Devices>> = Mutex::new(None);
static LAST_REPORT_OK: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
/// A bad driver handle must never make the next voice press block on another
/// DLL/device probe.  The hot path uses SendInput until the background retry
/// has had a chance to recreate the virtual devices.
static RETRY_AFTER: Mutex<Option<Instant>> = Mutex::new(None);
static RECOVERY_QUEUED: AtomicBool = AtomicBool::new(false);
const RECOVERY_COOLDOWN: Duration = Duration::from_secs(5);

/// Live health of the primary virtual-HID layer. `report_verified` becomes true
/// only after a real report was accepted by the driver in this process.
#[derive(Clone, Debug)]
pub struct VirtualHidHealth {
    pub ready: bool,
    pub report_verified: bool,
    pub last_error: Option<String>,
}

pub fn health() -> VirtualHidHealth {
    VirtualHidHealth {
        ready: is_available(),
        report_verified: LAST_REPORT_OK.load(Ordering::Acquire),
        last_error: LAST_ERROR.lock().clone(),
    }
}

fn record_success() {
    LAST_REPORT_OK.store(true, Ordering::Release);
    *LAST_ERROR.lock() = None;
    *RETRY_AFTER.lock() = None;
}

/// A failed report means the handle cannot be trusted for the next shortcut.
/// Destroy it immediately so the next shortcut re-probes WinUHid and can fall
/// back to SendInput instead of repeatedly using a stale virtual device.
fn record_failure(error: &str) {
    let _init = INIT_LOCK.lock();
    LAST_REPORT_OK.store(false, Ordering::Release);
    *LAST_ERROR.lock() = Some(error.to_string());
    *DEVICES.lock() = None;
    INIT_TRIED.store(false, Ordering::Release);
    *RETRY_AFTER.lock() = Some(Instant::now() + RECOVERY_COOLDOWN);
    schedule_recovery();
}

fn retry_is_blocked() -> bool {
    RETRY_AFTER
        .lock()
        .as_ref()
        .is_some_and(|deadline| Instant::now() < *deadline)
}

/// Re-probe once, outside the keyboard/ATVV notification callback.  A later
/// real failure may schedule a fresh attempt; a continuously missing driver
/// never creates a retry storm.
fn schedule_recovery() {
    if RECOVERY_QUEUED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("xiaomi-winuhid-recovery".into())
        .spawn(|| {
            std::thread::sleep(RECOVERY_COOLDOWN);
            *RETRY_AFTER.lock() = None;
            INIT_TRIED.store(false, Ordering::Release);
            let _ = ensure_init();
            RECOVERY_QUEUED.store(false, Ordering::Release);
        });
}

/// Start initialization before the user presses the voice key.  Failure is
/// intentionally silent to callers: SendInput remains the immediate route.
pub fn warmup_async() {
    if DEVICES.lock().is_some() || RECOVERY_QUEUED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("xiaomi-winuhid-warmup".into())
        .spawn(|| {
            *RETRY_AFTER.lock() = None;
            INIT_TRIED.store(false, Ordering::Release);
            let ready = ensure_init();
            RECOVERY_QUEUED.store(false, Ordering::Release);
            if !ready {
                schedule_recovery();
            }
        });
}

/// The bundled driver can become available after the process has already
/// attempted initialization. Clear the one-shot probe state before retrying.
pub fn reset_and_retry() {
    let _init = INIT_LOCK.lock();
    *DEVICES.lock() = None;
    INIT_TRIED.store(false, Ordering::SeqCst);
    *RETRY_AFTER.lock() = None;
    RECOVERY_QUEUED.store(false, Ordering::Release);
    let _ = ensure_init_locked();
}

fn dll_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_WINUHID_DLL") {
        let t = p.trim();
        if !t.is_empty() {
            out.push(PathBuf::from(t));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("WinUHid.dll"));
            out.push(dir.join("assets").join("winuhid").join("WinUHid.dll"));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("winuhid")
                    .join("WinUHid.dll"),
            );
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        out.push(
            PathBuf::from(manifest)
                .join("assets")
                .join("winuhid")
                .join("WinUHid.dll"),
        );
    }
    out
}

fn consumer_usage(vk: u16) -> Option<u16> {
    match vk {
        0xAD => Some(0x00E2),
        0xAE => Some(0x00EA),
        0xAF => Some(0x00E9),
        0xB0 => Some(0x00B6),
        0xB1 => Some(0x00B5),
        0xB2 => Some(0x00B7),
        0xB3 => Some(0x00CD),
        _ => None,
    }
}

fn modifier_bit(vk: u16) -> Option<u8> {
    match vk {
        0x11 | 0xA2 => Some(0x01),
        0x10 | 0xA0 => Some(0x02),
        0x12 | 0xA4 => Some(0x04),
        0x5B => Some(0x08),
        0xA3 => Some(0x10),
        0xA1 => Some(0x20),
        0xA5 => Some(0x40),
        0x5C => Some(0x80),
        _ => None,
    }
}

fn vk_usage(vk: u16) -> Option<u8> {
    if modifier_bit(vk).is_some() {
        return None;
    }
    Some(match vk {
        0x08 => 0x2A,
        0x09 => 0x2B,
        0x0D => 0x28,
        0x1B => 0x29,
        0x20 => 0x2C,
        0x21 => 0x4B,
        0x22 => 0x4E,
        0x23 => 0x4D,
        0x24 => 0x4A,
        0x25 => 0x50,
        0x26 => 0x52,
        0x27 => 0x4F,
        0x28 => 0x51,
        0x2D => 0x49,
        0x2E => 0x4C,
        0x5D => 0x65,
        0x70..=0x7B => (vk - 0x70 + 0x3A) as u8,
        // F13-F24 live in the second HID function-key range.  F24 is used as
        // a neutral companion before releasing a voice-only Win/Alt chord so
        // Windows does not open Start/the system menu for an unhandled hotkey.
        0x7C..=0x87 => (vk - 0x7C + 0x68) as u8,
        0x41..=0x5A => (vk - 0x41 + 0x04) as u8,
        0x31..=0x39 => (vk - 0x31 + 0x1E) as u8,
        0x30 => 0x27,
        _ => return None,
    })
}

fn build_keyboard_report(vks: &[u16]) -> Result<[u8; 8], String> {
    let mut modifier = 0u8;
    let mut usages = Vec::new();
    for &vk in vks {
        if let Some(bit) = modifier_bit(vk) {
            modifier |= bit;
            continue;
        }
        let Some(usage) = vk_usage(vk) else {
            return Err(format!("WinUHid keyboard unsupported VK 0x{vk:02X}"));
        };
        if !usages.contains(&usage) {
            usages.push(usage);
        }
    }
    if usages.len() > 6 {
        return Err("WinUHid keyboard supports at most six non-modifier keys".into());
    }
    let mut report = [0u8; 8];
    report[0] = modifier;
    for (i, u) in usages.into_iter().enumerate() {
        report[2 + i] = u;
    }
    Ok(report)
}

/// 尝试初始化 WinUHid（幂等）；无 DLL/驱动时保持关闭
pub fn ensure_init() -> bool {
    let _init = INIT_LOCK.lock();
    ensure_init_locked()
}

fn ensure_init_locked() -> bool {
    if DEVICES.lock().is_some() {
        return true;
    }
    if retry_is_blocked() {
        return false;
    }
    if INIT_TRIED.swap(true, Ordering::SeqCst) {
        return DEVICES.lock().is_some();
    }
    #[cfg(target_os = "windows")]
    {
        match windows_init() {
            Ok(dev) => {
                *DEVICES.lock() = Some(dev);
                *LAST_ERROR.lock() = None;
                log::info!("WinUHid ready (keyboard+consumer)");
                true
            }
            Err(e) => {
                LAST_REPORT_OK.store(false, Ordering::Release);
                *LAST_ERROR.lock() = Some(e.clone());
                log::warn!("WinUHid unavailable, SendInput fallback: {e}");
                false
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn is_available() -> bool {
    DEVICES.lock().is_some() || ensure_init()
}

/// Ordinary taps get their own lease; voice DOWN/UP use the reserved lease 1.
pub fn tap_vks(vks: &[u16], hold_ms: u64) -> bool {
    if !ensure_init() {
        return false;
    }
    tap_vks_ready(vks, hold_ms)
}
pub fn tap_vks_ready(vks: &[u16], hold_ms: u64) -> bool {
    let owner = new_owner();
    if press_owned(owner, vks).is_err() {
        return false;
    }
    std::thread::sleep(Duration::from_millis(hold_ms.clamp(20, 1000)));
    release_owned(owner).is_ok()
}
pub fn press(vks: &[u16]) -> Result<(), String> {
    if !ensure_init() {
        return Err("WinUHid not open".into());
    }
    press_ready(vks)
}
pub fn press_ready(vks: &[u16]) -> Result<(), String> {
    press_owned(1, vks)
}
pub fn release(vks: &[u16]) -> Result<(), String> {
    release_ready(vks)
}
pub fn release_ready(_vks: &[u16]) -> Result<(), String> {
    release_owned(1)
}

pub fn press_owned(owner: u64, vks: &[u16]) -> Result<(), String> {
    if vks.is_empty() {
        return Err("Empty chord".into());
    }
    build_keyboard_report(
        &vks.iter()
            .copied()
            .filter(|vk| consumer_usage(*vk).is_none())
            .collect::<Vec<_>>(),
    )?;
    change_owner(owner, Some(vks))
}
pub fn release_owned(owner: u64) -> Result<(), String> {
    change_owner(owner, None)
}
fn change_owner(owner: u64, keys: Option<&[u16]>) -> Result<(), String> {
    let result = {
        let mut guard = DEVICES.lock();
        let dev = guard.as_mut().ok_or("WinUHid not open")?;
        let mut next = dev.owners.clone();
        if let Some(keys) = keys {
            if owner != 1 && next.0.contains_key(&owner) {
                return Err("HID lease already pressed".into());
            }
            next.0.insert(owner, keys.to_vec());
        } else if next.0.remove(&owner).is_none() {
            return Ok(());
        }
        // Encode before touching hardware: unsupported combinations preserve existing leases.
        let kb = next.keyboard_report()?;
        let cc = next.consumer_report()?;
        let result = submit(dev, dev.keyboard, &kb).and_then(|_| submit(dev, dev.consumer, &cc));
        if result.is_ok() {
            dev.owners = next;
        }
        result
    };
    match &result {
        Ok(()) => record_success(),
        Err(error) => record_failure(error),
    }
    result
}
pub fn release_all() {
    let mut guard = DEVICES.lock();
    if let Some(dev) = guard.as_mut() {
        let _ = submit(dev, dev.keyboard, &[0; 8]);
        let _ = submit(dev, dev.consumer, &[0; 2]);
        dev.owners.0.clear();
    }
}

fn submit(dev: &Devices, handle: *mut c_void, report: &[u8]) -> Result<(), String> {
    unsafe {
        if (dev.api.submit)(handle, report.as_ptr(), report.len() as u32) == 0 {
            return Err("WinUHidSubmitInputReport failed".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_init() -> Result<Devices, String> {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    let path = dll_candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "WinUHid.dll was not found".to_string())?;
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
        .map_err(|e| format!("LoadLibraryW WinUHid.dll: {e}"))?;

    unsafe fn proc<T>(
        module: windows::Win32::Foundation::HMODULE,
        name: &[u8],
    ) -> Result<T, String> {
        let p = GetProcAddress(module, PCSTR(name.as_ptr()))
            .ok_or_else(|| format!("GetProcAddress missing {}", String::from_utf8_lossy(name)))?;
        Ok(std::mem::transmute_copy(&p))
    }

    let get_ver: FnGetVersion = unsafe { proc(module, b"WinUHidGetDriverInterfaceVersion\0")? };
    let create: FnCreateDevice = unsafe { proc(module, b"WinUHidCreateDevice\0")? };
    let start: FnStartDevice = unsafe { proc(module, b"WinUHidStartDevice\0")? };
    let submit: FnSubmitReport = unsafe { proc(module, b"WinUHidSubmitInputReport\0")? };
    let destroy: FnDestroyDevice = unsafe { proc(module, b"WinUHidDestroyDevice\0")? };

    let version = unsafe { get_ver() };
    if version < 1 {
        unsafe {
            let _ = FreeLibrary(module);
        }
        return Err(format!("WinUHid driver unavailable version={version}"));
    }

    let api = Api {
        _module: module,
        create,
        start,
        submit,
        destroy,
    };

    let keyboard = create_device(
        &api,
        KEYBOARD_REPORT_DESCRIPTOR,
        1,
        "XiaomiRemoteBridgeKeyboard",
    )?;
    let consumer = match create_device(
        &api,
        CONSUMER_REPORT_DESCRIPTOR,
        2,
        "XiaomiRemoteBridgeConsumer",
    ) {
        Ok(h) => h,
        Err(e) => {
            unsafe { (api.destroy)(keyboard) };
            unsafe {
                let _ = FreeLibrary(module);
            }
            return Err(e);
        }
    };

    // 空报告复位
    let devices = Devices {
        api,
        keyboard,
        consumer,
        owners: ReportOwners::default(),
    };
    let zero_kb = [0u8; 8];
    let zero_cc = [0u8; 2];
    let _ = unsafe { (devices.api.submit)(devices.keyboard, zero_kb.as_ptr(), 8) };
    let _ = unsafe { (devices.api.submit)(devices.consumer, zero_cc.as_ptr(), 2) };
    Ok(devices)
}

#[cfg(target_os = "windows")]
fn create_device(
    api: &Api,
    descriptor: &[u8],
    product_id: u16,
    instance_id: &str,
) -> Result<*mut c_void, String> {
    let mut wide: Vec<u16> = instance_id
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let config = WinUHidDeviceConfig {
        supported_events: 0,
        vendor_id: 0,
        product_id,
        version_number: 1,
        report_descriptor_length: descriptor.len() as u16,
        report_descriptor: descriptor.as_ptr(),
        container_id: [0u8; 16],
        instance_id: wide.as_mut_ptr(),
        hardware_ids: std::ptr::null(),
        read_report_period_us: 0,
    };
    let device = unsafe { (api.create)(&config) };
    if device.is_null() {
        return Err(format!("WinUHidCreateDevice failed for {instance_id}"));
    }
    if unsafe { (api.start)(device, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
        unsafe { (api.destroy)(device) };
        return Err(format!("WinUHidStartDevice failed for {instance_id}"));
    }
    Ok(device)
}

// ---- 兼容旧 API（测试 / 旧调用方）----

#[repr(C)]
pub struct KeyboardReport {
    pub modifier: u8,
    pub reserved: u8,
    pub keys: [u8; 6],
}

impl KeyboardReport {
    pub fn new() -> Self {
        Self {
            modifier: 0,
            reserved: 0,
            keys: [0; 6],
        }
    }
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0] = self.modifier;
        buf[2..8].copy_from_slice(&self.keys);
        buf
    }
}

pub struct ConsumerReport {
    pub usage: u16,
}

impl ConsumerReport {
    pub fn volume_up() -> Self {
        Self { usage: 0xE9 }
    }
    pub fn to_bytes(&self) -> [u8; 2] {
        self.usage.to_le_bytes()
    }
}

pub struct WinUHidInjector;

impl WinUHidInjector {
    pub fn new() -> Self {
        Self
    }
    pub fn is_open(&self) -> bool {
        is_available()
    }
    pub fn init(&mut self) -> Result<(), String> {
        if ensure_init() {
            Ok(())
        } else {
            Err("WinUHid unavailable".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_actions_keep_other_owners_and_shared_modifiers() {
        let mut owners = ReportOwners::default();
        owners.0.insert(1, vec![0xA5]);
        owners.0.insert(2, vec![0xA2, 0x1B]);
        owners.0.insert(3, vec![0xA2, 0x41]);
        assert_eq!(owners.keyboard_report().unwrap()[0], 0x41);
        owners.0.remove(&2);
        assert_eq!(owners.keyboard_report().unwrap()[0], 0x41);
        owners.0.remove(&3);
        assert_eq!(owners.keyboard_report().unwrap()[0], 0x40);
        owners.0.remove(&1);
        assert_eq!(owners.keyboard_report().unwrap(), [0; 8]);
    }
    #[test]
    fn report_rollover_and_consumer_overlap_are_rejected_before_submit() {
        let mut owners = ReportOwners::default();
        owners.0.insert(1, vec![0x41, 0x42, 0x43, 0x44, 0x45, 0x46]);
        assert!(owners.keyboard_report().is_ok());
        owners.0.insert(2, vec![0x47]);
        assert!(owners.keyboard_report().is_err());
        owners.0.insert(3, vec![0xAE, 0xAF]);
        assert!(owners.consumer_report().is_err());
    }
    #[test]
    fn keyboard_report_escape() {
        let r = build_keyboard_report(&[0x1B]).unwrap();
        assert_eq!(r[2], 0x29);
    }

    #[test]
    fn consumer_volume() {
        assert_eq!(consumer_usage(0xAF), Some(0xE9));
    }

    #[test]
    fn keyboard_report_f24() {
        let r = build_keyboard_report(&[0x87]).unwrap();
        assert_eq!(r[2], 0x73);
    }
}
