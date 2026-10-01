//! Link and service recovery policy. Pairing and worker liveness are not link proof.
use std::time::Duration;

pub const LINK_PROBE_INTERVAL: Duration = Duration::from_secs(5);

pub struct RecoveryHealth {
    pub paired: Option<bool>,
    pub connected: Option<bool>,
    pub key_bridge: &'static str,
    pub raw_ready: bool,
}

impl Default for RecoveryHealth {
    fn default() -> Self {
        Self {
            paired: None,
            connected: None,
            key_bridge: "waiting",
            raw_ready: false,
        }
    }
}

pub fn probe_restart_reason(
    previous: bool,
    current: Option<bool>,
    resumed: bool,
) -> Option<&'static str> {
    if resumed {
        Some("system_resume_refresh_handles")
    } else if current != Some(previous) {
        Some(if current == Some(true) {
            "bluetooth_online_refresh_handles"
        } else {
            "bluetooth_offline_or_handle_invalid"
        })
    } else {
        None
    }
}

pub fn tap_attach_due(connected: bool, atvv: bool, enabled: bool, running: bool) -> bool {
    connected && atvv && enabled && !running
}

pub fn subscription_complete(current: bool, count: usize, success: bool) -> bool {
    current && count == 2 && success
}

/// Failed batches retain ownership so their Drop cleanup runs exactly once.
pub fn commit_subscription<T>(
    current: bool,
    success: bool,
    pending: &mut Vec<T>,
    active: &mut Vec<T>,
) -> bool {
    if !subscription_complete(current, pending.len(), success) {
        return false;
    }
    active.append(pending);
    true
}

#[derive(Default)]
pub struct SubscriptionRecovery {
    failures: u8,
}

impl SubscriptionRecovery {
    pub fn observe(&mut self, connected: bool, subscribed: bool) -> bool {
        if !connected || subscribed {
            self.failures = 0;
            return false;
        }
        self.failures = self.failures.saturating_add(1);
        self.failures >= 3
    }
}

/// Watcher callbacks only coalesce a probe request. BLE work stays on its owner thread.
#[cfg(target_os = "windows")]
pub struct PresenceWatcher {
    watcher: windows::Devices::Enumeration::DeviceWatcher,
    added: windows::Foundation::EventRegistrationToken,
    updated: windows::Foundation::EventRegistrationToken,
    removed: windows::Foundation::EventRegistrationToken,
}

#[cfg(target_os = "windows")]
impl PresenceWatcher {
    pub fn start(
        runtime: std::sync::Arc<super::connect::XiaomiRuntime>,
        address: Option<&str>,
    ) -> windows::core::Result<Self> {
        use std::sync::atomic::Ordering;
        use windows::Devices::Bluetooth::BluetoothLEDevice;
        use windows::Devices::Enumeration::{
            DeviceInformation, DeviceInformationUpdate, DeviceWatcher,
        };
        use windows::Foundation::TypedEventHandler;
        let mut selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?.to_string();
        if let Some(address) = address {
            if let Ok(address) = super::connect::normalize_bluetooth_address(address) {
                selector.push_str(&format!(
                    " AND System.Devices.Aep.DeviceAddress:=\"{address}\""
                ));
            }
        }
        let properties: windows::Foundation::Collections::IIterable<windows::core::HSTRING> = vec![
            windows::core::HSTRING::from("System.Devices.Aep.IsConnected"),
            windows::core::HSTRING::from("System.Devices.Aep.IsPaired"),
        ]
        .try_into()?;
        let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
            &windows::core::HSTRING::from(selector),
            &properties,
            windows::Devices::Enumeration::DeviceInformationKind::AssociationEndpoint,
        )?;
        let added_runtime = runtime.clone();
        let added = watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(
            move |_, _| {
                added_runtime.probe_requested.store(true, Ordering::SeqCst);
                Ok(())
            },
        ))?;
        let updated_runtime = runtime.clone();
        let updated = watcher.Updated(&TypedEventHandler::<
            DeviceWatcher,
            DeviceInformationUpdate,
        >::new(move |_, _| {
            updated_runtime
                .probe_requested
                .store(true, Ordering::SeqCst);
            Ok(())
        }))?;
        let removed = watcher.Removed(&TypedEventHandler::<
            DeviceWatcher,
            DeviceInformationUpdate,
        >::new(move |_, _| {
            runtime.probe_requested.store(true, Ordering::SeqCst);
            Ok(())
        }))?;
        watcher.Start()?;
        log::info!("XIAOMI PRESENCE watcher started");
        Ok(Self {
            watcher,
            added,
            updated,
            removed,
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for PresenceWatcher {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
        let _ = self.watcher.RemoveAdded(self.added);
        let _ = self.watcher.RemoveUpdated(self.updated);
        let _ = self.watcher.RemoveRemoved(self.removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_power_on_bluetooth_toggle_and_sleep_request_handle_refresh() {
        assert_eq!(probe_restart_reason(false, Some(false), false), None);
        assert_eq!(
            probe_restart_reason(false, Some(true), false),
            Some("bluetooth_online_refresh_handles")
        );
        assert_eq!(
            probe_restart_reason(true, Some(false), false),
            Some("bluetooth_offline_or_handle_invalid")
        );
        assert_eq!(
            probe_restart_reason(true, None, false),
            Some("bluetooth_offline_or_handle_invalid")
        );
        assert_eq!(
            probe_restart_reason(true, Some(true), true),
            Some("system_resume_refresh_handles")
        );
        for _ in 0..100 {
            assert_eq!(probe_restart_reason(true, Some(true), false), None);
        }
    }
    #[test]
    fn late_atvv_starts_tap_once_and_partial_subscriptions_cannot_commit() {
        assert!(!tap_attach_due(false, true, true, false));
        assert!(!tap_attach_due(true, false, true, false));
        assert!(tap_attach_due(true, true, true, false));
        assert!(!tap_attach_due(true, true, true, true));
        assert!(!tap_attach_due(true, true, false, false));
        assert!(!subscription_complete(true, 1, true));
        assert!(!subscription_complete(false, 2, true));
        assert!(!subscription_complete(true, 2, false));
        assert!(subscription_complete(true, 2, true));
    }
    #[test]
    fn partial_failed_and_stale_batches_roll_back_once_without_accumulating_callbacks() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct Token(Arc<AtomicUsize>);
        impl Drop for Token {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let removed = Arc::new(AtomicUsize::new(0));
        let mut active = Vec::new();
        for (count, current, success) in [(1, true, true), (2, true, false), (2, false, true)] {
            let mut pending = (0..count).map(|_| Token(removed.clone())).collect();
            assert!(!commit_subscription(
                current,
                success,
                &mut pending,
                &mut active
            ));
            drop(pending);
            assert!(active.is_empty());
        }
        assert_eq!(removed.load(Ordering::SeqCst), 5);
        let mut pending = vec![Token(removed.clone()), Token(removed.clone())];
        assert!(commit_subscription(true, true, &mut pending, &mut active));
        assert!(pending.is_empty());
        assert_eq!(active.len(), 2);
        drop(pending);
        assert_eq!(removed.load(Ordering::SeqCst), 5);
        drop(active);
        assert_eq!(removed.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn offline_and_idle_do_not_restart_but_three_online_failures_do() {
        let mut recovery = SubscriptionRecovery::default();
        for _ in 0..100 {
            assert!(!recovery.observe(false, false));
            assert!(!recovery.observe(true, true));
        }
        assert!(!recovery.observe(true, false));
        assert!(!recovery.observe(true, false));
        assert!(recovery.observe(true, false));
    }

    #[test]
    fn success_and_disconnect_reset_the_consecutive_failure_budget() {
        let mut recovery = SubscriptionRecovery::default();
        assert!(!recovery.observe(true, false));
        assert!(!recovery.observe(true, true));
        assert!(!recovery.observe(true, false));
        assert!(!recovery.observe(false, false));
        assert!(!recovery.observe(true, false));
        assert!(!recovery.observe(true, false));
    }
}
#[cfg(target_os = "windows")]
static POWER_RUNTIME: parking_lot::Mutex<Option<std::sync::Weak<super::connect::XiaomiRuntime>>> =
    parking_lot::Mutex::new(None);

#[cfg(target_os = "windows")]
pub struct PowerSubscription {
    handle: windows::Win32::System::Power::HPOWERNOTIFY,
    _parameters: Box<windows::Win32::System::Power::DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn power_event(
    _: *const std::ffi::c_void,
    kind: u32,
    _: *const std::ffi::c_void,
) -> u32 {
    use std::sync::atomic::Ordering;
    use windows::Win32::UI::WindowsAndMessaging::{PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND};
    if kind == PBT_APMRESUMEAUTOMATIC || kind == PBT_APMRESUMESUSPEND {
        if let Some(runtime) = POWER_RUNTIME.lock().as_ref().and_then(|r| r.upgrade()) {
            if !runtime.should_stop() {
                runtime.resume_requested.store(true, Ordering::SeqCst);
                runtime.probe_requested.store(true, Ordering::SeqCst);
            }
        }
    }
    0
}

#[cfg(target_os = "windows")]
impl PowerSubscription {
    pub fn start(
        runtime: &std::sync::Arc<super::connect::XiaomiRuntime>,
    ) -> windows::core::Result<Self> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Power::{
            PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
            HPOWERNOTIFY,
        };
        use windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS;
        *POWER_RUNTIME.lock() = Some(std::sync::Arc::downgrade(runtime));
        let parameters = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(power_event),
            Context: std::ptr::null_mut(),
        });
        let mut handle = std::ptr::null_mut();
        let result = unsafe {
            PowerRegisterSuspendResumeNotification(
                REGISTER_NOTIFICATION_FLAGS(2),
                HANDLE(
                    (parameters.as_ref() as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                        .cast_mut()
                        .cast(),
                ),
                &mut handle,
            )
        };
        result.ok()?;
        Ok(Self {
            handle: HPOWERNOTIFY(handle as isize),
            _parameters: parameters,
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for PowerSubscription {
    fn drop(&mut self) {
        let _ = unsafe {
            windows::Win32::System::Power::PowerUnregisterSuspendResumeNotification(self.handle)
        };
        *POWER_RUNTIME.lock() = None;
    }
}
#[cfg(target_os = "windows")]
pub fn paired_record(address: Option<&str>) -> Option<bool> {
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Enumeration::DeviceInformation;
    let address = super::connect::normalize_bluetooth_address(address?).ok()?;
    let selector = format!(
        "{} AND System.Devices.Aep.DeviceAddress:=\"{address}\"",
        BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).ok()?
    );
    DeviceInformation::FindAllAsyncAqsFilter(&windows::core::HSTRING::from(selector))
        .ok()?
        .get()
        .ok()?
        .Size()
        .ok()
        .map(|size| size > 0)
}
