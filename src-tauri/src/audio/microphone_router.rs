//! Windows default-recording-device routing for the remote microphone.
//!
//! The voice path calls this module synchronously.  It talks to the Windows
//! audio policy COM interface directly, so pressing the remote never starts a
//! PowerShell process.  Non-Windows builds keep the same API for tests and
//! cross compilation.

use std::sync::{Mutex, OnceLock};

const CABLE_NAME: &str = "CABLE Output (VB-Audio Virtual Cable)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    AlwaysOn,
}

#[derive(Debug, Default)]
struct State {
    mode: Option<Mode>,
    pressed: bool,
    owned_cable: bool,
    previous: Option<String>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static MONITOR: OnceLock<()> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

/// Apply the saved setting.  This is intentionally idempotent and is safe to
/// call during startup, settings save, and bridge restart.
pub fn apply_settings(always_on: bool) {
    start_monitor();
    let mut state = state().lock().expect("microphone router mutex poisoned");
    let next = if always_on { Mode::AlwaysOn } else { Mode::Off };
    if !always_on && state.previous.is_none() && legacy_previous().is_some() {
        state.owned_cable = true;
        restore_if_owned(&mut state);
    }
    if state.mode == Some(next) {
        if always_on {
            ensure_cable(&mut state);
        }
        return;
    }
    if state.owned_cable && !always_on {
        restore_if_owned(&mut state);
    }
    state.mode = Some(next);
    state.pressed = false;
    state.owned_cable = false;
    state.previous = None;
    if always_on {
        ensure_cable(&mut state);
    }
}

/// Switch to CABLE before the voice shortcut is sent.
pub fn voice_pressed() {
    let mut state = state().lock().expect("microphone router mutex poisoned");
    if state.pressed {
        return;
    }
    state.pressed = true;
    if state.mode == Some(Mode::AlwaysOn) {
        ensure_cable(&mut state);
    } else {
        switch_to_cable(&mut state);
    }
}

/// Restore only when the application still owns the default endpoint.  A user
/// selection made while speaking therefore always wins over the old snapshot.
pub fn voice_released() {
    let mut state = state().lock().expect("microphone router mutex poisoned");
    if !state.pressed {
        return;
    }
    state.pressed = false;
    if state.mode != Some(Mode::AlwaysOn) && state.owned_cable {
        restore_if_owned(&mut state);
    }
}

/// Used by disconnect, bridge restart, tray quit, and process exit.
pub fn cleanup(reason: &str) {
    let mut state = state().lock().expect("microphone router mutex poisoned");
    if state.owned_cable && state.mode != Some(Mode::AlwaysOn) {
        restore_if_owned(&mut state);
    }
    state.pressed = false;
    state.owned_cable = false;
    state.previous = None;
    log::debug!("microphone route cleanup reason={reason}");
}

fn start_monitor() {
    if MONITOR.set(()).is_ok() {
        let _ = std::thread::Builder::new()
            .name("microphone-route-monitor".into())
            .spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let mut state = state().lock().expect("microphone router mutex poisoned");
                if state.mode == Some(Mode::AlwaysOn) {
                    ensure_cable(&mut state);
                }
            });
    }
}

fn switch_to_cable(state: &mut State) {
    #[cfg(target_os = "windows")]
    {
        let Some(cable) = find_cable() else {
            log::warn!(
                "CABLE microphone endpoint is unavailable; leaving current default unchanged"
            );
            return;
        };
        let current = current_default();
        if current.as_deref() == Some(cable.as_str()) {
            return;
        }
        if let Some(current) = current {
            state.previous = Some(current);
        }
        match set_default(&cable) {
            Ok(()) => state.owned_cable = true,
            Err(error) => {
                log::warn!("unable to set CABLE microphone default: {error}");
                state.previous = None;
            }
        }
    }
}

fn ensure_cable(state: &mut State) {
    #[cfg(target_os = "windows")]
    {
        let Some(cable) = find_cable() else {
            log::warn!("always-on microphone mode requested but CABLE endpoint is unavailable");
            return;
        };
        if current_default().as_deref() == Some(cable.as_str()) {
            return;
        }
        if state.previous.is_none() {
            state.previous = current_default();
        }
        match set_default(&cable) {
            Ok(()) => state.owned_cable = true,
            Err(error) => log::warn!("unable to enforce CABLE microphone default: {error}"),
        }
    }
}

fn restore_if_owned(state: &mut State) {
    #[cfg(target_os = "windows")]
    {
        let Some(cable) = find_cable() else { return };
        if current_default().as_deref() != Some(cable.as_str()) {
            log::debug!("microphone restore skipped: user selected another endpoint");
            remove_legacy_previous();
            state.owned_cable = false;
            state.previous = None;
            return;
        }
        let previous = state.previous.clone().or_else(legacy_previous);
        let Some(previous) = previous else { return };
        if endpoint_exists(&previous) {
            if let Err(error) = set_default(&previous) {
                log::warn!("unable to restore previous microphone: {error}");
            } else {
                remove_legacy_previous();
                state.owned_cable = false;
                state.previous = None;
            }
        } else {
            log::warn!("previous microphone endpoint is no longer present; leaving CABLE selected");
            remove_legacy_previous();
            state.owned_cable = false;
            state.previous = None;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn current_default() -> Option<String> {
    None
}
#[cfg(not(target_os = "windows"))]
fn find_cable() -> Option<String> {
    None
}
#[cfg(not(target_os = "windows"))]
fn endpoint_exists(_: &str) -> bool {
    false
}
#[cfg(not(target_os = "windows"))]
fn set_default(_: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(target_os = "windows"))]
fn legacy_previous() -> Option<String> {
    None
}
#[cfg(not(target_os = "windows"))]
fn remove_legacy_previous() {}

#[cfg(target_os = "windows")]
fn legacy_previous() -> Option<String> {
    let root = std::env::var_os("LOCALAPPDATA")?;
    let path = std::path::PathBuf::from(root)
        .join("2655AI/BridgeAudio/XiaomiRemoteBridge/previous-default-microphone.txt");
    let value = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}
#[cfg(target_os = "windows")]
fn remove_legacy_previous() {
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        let path = std::path::PathBuf::from(root)
            .join("2655AI/BridgeAudio/XiaomiRemoteBridge/previous-default-microphone.txt");
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::CABLE_NAME;
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, eMultimedia, ERole, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    const POLICY_CONFIG_CLSID: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const POLICY_CONFIG_IID: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    const DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    struct ComGuard {
        uninit: bool,
    }
    impl ComGuard {
        fn new() -> Result<Self, String> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                if hr.is_ok() {
                    Ok(Self { uninit: true })
                } else if hr == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                    Ok(Self { uninit: false })
                } else {
                    Err(format!("CoInitializeEx failed: {hr:?}"))
                }
            }
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.uninit {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    pub(super) fn endpoints() -> Result<Vec<(String, String)>, String> {
        let _com = ComGuard::new()?;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;
            let collection = enumerator
                .EnumAudioEndpoints(eCapture, windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE)
                .map_err(|e| e.to_string())?;
            let count = collection.GetCount().map_err(|e| e.to_string())?;
            let mut result = Vec::with_capacity(count as usize);
            for index in 0..count {
                let device = collection.Item(index).map_err(|e| e.to_string())?;
                let id = device.GetId().map_err(|e| e.to_string())?;
                let id_string = id.to_string().map_err(|e| e.to_string())?;
                CoTaskMemFree(Some(id.0 as _));
                let store = device
                    .OpenPropertyStore(windows::Win32::System::Com::STGM_READ)
                    .map_err(|e| e.to_string())?;
                let value = store
                    .GetValue(&DEVICE_FRIENDLY_NAME)
                    .map_err(|e| e.to_string())?;
                let name = value.to_string();
                result.push((id_string, name));
            }
            Ok(result)
        }
    }

    pub(super) fn default() -> Result<String, String> {
        let _com = ComGuard::new()?;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .map_err(|e| e.to_string())?;
            let id = device.GetId().map_err(|e| e.to_string())?;
            let result = id.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(id.0 as _));
            result
        }
    }

    #[repr(C)]
    struct PolicyConfigVtable {
        unknown: windows::core::IUnknown_Vtbl,
        slots: [usize; 10],
        set_default: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            PCWSTR,
            ERole,
        ) -> windows::core::HRESULT,
    }
    #[repr(transparent)]
    #[derive(Clone)]
    struct PolicyConfig(windows::core::IUnknown);
    unsafe impl windows::core::Interface for PolicyConfig {
        type Vtable = PolicyConfigVtable;
        const IID: GUID = POLICY_CONFIG_IID;
    }

    pub(super) fn set_default(id: &str) -> Result<(), String> {
        let _com = ComGuard::new()?;
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let policy: PolicyConfig = CoCreateInstance(&POLICY_CONFIG_CLSID, None, CLSCTX_ALL)
                .map_err(|e| e.to_string())?;
            let method = (*(policy.as_raw() as *mut PolicyConfigVtable)).set_default;
            for role in [
                eConsole,
                eMultimedia,
                windows::Win32::Media::Audio::eCommunications,
            ] {
                let hr = method(policy.as_raw() as _, PCWSTR(wide.as_ptr()), role);
                if hr.is_err() {
                    return Err(format!("PolicyConfig SetDefaultEndpoint failed: {hr:?}"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn cable() -> Option<String> {
        endpoints()
            .ok()?
            .into_iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(CABLE_NAME))
            .map(|(id, _)| id)
    }
    pub(super) fn exists(id: &str) -> bool {
        endpoints()
            .map(|items| items.into_iter().any(|(candidate, _)| candidate == id))
            .unwrap_or(false)
    }
}

#[cfg(target_os = "windows")]
fn current_default() -> Option<String> {
    windows_impl::default().ok()
}
#[cfg(target_os = "windows")]
fn find_cable() -> Option<String> {
    windows_impl::cable()
}
#[cfg(target_os = "windows")]
fn endpoint_exists(id: &str) -> bool {
    windows_impl::exists(id)
}
#[cfg(target_os = "windows")]
fn set_default(id: &str) -> Result<(), String> {
    windows_impl::set_default(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_state_is_safe_and_idempotent() {
        let mut value = State::default();
        value.mode = Some(Mode::Off);
        assert!(!value.pressed);
        assert!(!value.owned_cable);
        assert!(value.previous.is_none());
    }
    #[test]
    fn duplicate_edges_do_not_change_press_state() {
        let mut value = State::default();
        value.pressed = true;
        value.pressed = true;
        assert!(value.pressed);
    }
}
