//! Process-wide ownership of manual recovery; the blocking job owns the guard.
use parking_lot::Mutex;

static ACTIVE: Mutex<Option<&'static str>> = Mutex::new(None);
static DRIVER_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static BLUETOOTH_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn wait_for_bluetooth_owner(
    runtime: std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>,
) {
    BLUETOOTH_PENDING.store(true, std::sync::atomic::Ordering::Release);
    std::thread::spawn(move || {
        while runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        BLUETOOTH_PENDING.store(false, std::sync::atomic::Ordering::Release);
    });
}
pub fn set_driver_pending(pending: bool) {
    DRIVER_PENDING.store(pending, std::sync::atomic::Ordering::Release);
}
pub struct RepairGuard;

pub fn active() -> Option<String> {
    if BLUETOOTH_PENDING.load(std::sync::atomic::Ordering::Acquire) {
        return Some("Windows 蓝牙操作仍在退出".into());
    }
    if DRIVER_PENDING.load(std::sync::atomic::Ordering::Acquire) {
        return Some("Windows 驱动安装仍在执行".into());
    }
    ACTIVE.lock().map(str::to_owned)
}

pub fn acquire(name: &'static str) -> Result<RepairGuard, String> {
    if BLUETOOTH_PENDING.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Windows 蓝牙操作仍在退出，请等待完成后再修复。".into());
    }
    if DRIVER_PENDING.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Windows 驱动安装仍在执行，请等待结束后再修复。".into());
    }
    let mut active = ACTIVE.lock();
    if let Some(name) = *active {
        return Err(format!("{name}仍在执行，请等待完成；不要重复修复。"));
    }
    *active = Some(name);
    log::info!("RECOVERY begin operation={name}");
    Ok(RepairGuard)
}

impl Drop for RepairGuard {
    fn drop(&mut self) {
        let completed = ACTIVE.lock().take();
        log::info!("RECOVERY end operation={completed:?}");
    }
}

/// A failed/disabled device is installed too. Unknown must never mean absent.
pub fn driver_installed(pattern: &str) -> Result<bool, String> {
    let script = format!(
        r#"$ErrorActionPreference='Stop'; $roots = @('HKLM:\SYSTEM\CurrentControlSet\Services', 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\WUDF\Services'); $found = @(); foreach ($root in $roots) {{ if (Test-Path $root) {{ $found += @(Get-ChildItem $root | Where-Object {{ $_.PSChildName -match '{pattern}' }}) }} }}; if ($found.Count -gt 0) {{ 'installed' }} else {{ 'absent' }}"#
    );
    let output = crate::windows_command::software_helper_output(
        crate::windows_command::background_command("powershell.exe").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        std::time::Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err("无法读取驱动安装状态；已停止自动安装，请查看日志或重启 Windows。".into());
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "installed" => Ok(true),
        "absent" => Ok(false),
        _ => Err("驱动检测结果未知，已停止自动安装。".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_one_recovery_job_owns_the_slot() {
        let guard = super::acquire("test").unwrap();
        assert!(super::acquire("overlap").is_err());
        assert_eq!(super::active().as_deref(), Some("test"));
        drop(guard);
        assert!(super::active().is_none());
    }
}
