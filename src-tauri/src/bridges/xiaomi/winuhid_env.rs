//! Bundled WinUHid deployment and first-run driver installation.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const ASSET_DIR: &str = "assets/winuhid";

fn asset_candidates(relative: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("resources").join(ASSET_DIR).join(relative));
            candidates.push(dir.join(ASSET_DIR).join(relative));
            candidates.push(dir.join(relative));
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest).join(ASSET_DIR).join(relative));
    }
    candidates
}

fn first_asset(relative: &str) -> Option<PathBuf> {
    asset_candidates(relative).into_iter().find(|path| path.is_file())
}

fn driver_dir() -> Result<PathBuf, String> {
    let marker = first_asset("driver/WinUHidDriver.inf")
        .ok_or_else(|| "bundled WinUHid driver INF is missing".to_string())?;
    marker
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "invalid bundled WinUHid driver path".to_string())
}

fn configure_dll() -> Result<PathBuf, String> {
    let bundled_dll = first_asset("WinUHid.dll")
        .ok_or_else(|| "bundled WinUHid.dll is missing".to_string())?;
    // Load directly from the packaged resource instead of copying alongside the
    // executable. A per-user install can be read-only or retain a same-size stale DLL.
    std::env::set_var("REMOTE_BRIDGE_WINUHID_DLL", &bundled_dll);
    Ok(bundled_dll)
}

pub fn ensure_runtime_quiet() {
    match configure_dll() {
        Ok(path) => {
            crate::bridges::xiaomi::hid_injector::reset_and_retry();
            log::info!("WinUHid SDK configured from bundled resource: {}", path.display());
        }
        Err(error) => log::warn!("WinUHid SDK configuration failed: {error}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallOutcome {
    Ready,
    RestartRequired,
}

fn run_installer(force: bool) -> Result<InstallOutcome, String> {
    // Do not destroy a device that currently owns a held modifier before releasing it.
    if force {
        crate::bridges::xiaomi::key_mapping::force_release_voice_shortcut("virtual_keyboard_repair");
    }
    let dll = configure_dll()?;
    let script = first_asset("install-winuhid.ps1")
        .ok_or_else(|| "bundled WinUHid install script is missing".to_string())?;
    let package = driver_dir()?;
    log::info!(
        "Installing bundled WinUHid driver force={force} script={} package={}",
        script.display(),
        package.display()
    );
    let mut args = vec![
        "-NoProfile".to_string(),
        "-WindowStyle".to_string(),
        "Hidden".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script.display().to_string(),
        "-Mode".to_string(),
        "Install".to_string(),
        "-PackageDir".to_string(),
        package.display().to_string(),
        "-DllSource".to_string(),
        dll.display().to_string(),
    ];
    if force {
        args.push("-Force".to_string());
    }
    let output = Command::new("powershell.exe")
        .args(args)
        .output()
        .map_err(|error| format!("start WinUHid installer: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        log::info!("WinUHid installer: {stdout}");
    }
    if !stderr.is_empty() {
        log::warn!("WinUHid installer stderr: {stderr}");
    }
    if output.status.code() == Some(3010) {
        return Ok(InstallOutcome::RestartRequired);
    }
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("WinUHid installer failed exit={:?}: {detail}", output.status.code()));
    }
    for _ in 0..20 {
        crate::bridges::xiaomi::hid_injector::reset_and_retry();
        if crate::bridges::xiaomi::hid_injector::is_available() {
            log::info!("WinUHid virtual keyboard ready");
            return Ok(InstallOutcome::Ready);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(InstallOutcome::RestartRequired)
}

pub fn install_if_needed() -> Result<(), String> {
    if crate::bridges::xiaomi::hid_injector::is_available() {
        log::info!("WinUHid virtual keyboard already ready");
        return Ok(());
    }
    match run_installer(false)? {
        InstallOutcome::Ready => Ok(()),
        InstallOutcome::RestartRequired => {
            log::warn!("WinUHid driver installation needs a Windows restart before it can be used");
            Ok(())
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinUHidRepairResult {
    pub ready: bool,
    pub restart_required: bool,
    pub message: String,
}

pub fn repair() -> Result<WinUHidRepairResult, String> {
    match run_installer(true)? {
        InstallOutcome::Ready => Ok(WinUHidRepairResult {
            ready: true,
            restart_required: false,
            message: "虚拟键盘已修复。现在可重新测试豆包、微信或其它输入法快捷键。".into(),
        }),
        InstallOutcome::RestartRequired => Ok(WinUHidRepairResult {
            ready: false,
            restart_required: true,
            message: "虚拟键盘驱动已安装，但尚未就绪。请重启 Windows 后再测试输入法快捷键。".into(),
        }),
    }
}
