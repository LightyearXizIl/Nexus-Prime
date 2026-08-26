//! 开机自启：仅使用当前用户 Run 注册表，并清理旧版 Startup 快捷方式。

use std::path::PathBuf;

#[cfg(target_os = "windows")]
fn startup_dir() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA missing".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup"))
}

#[cfg(target_os = "windows")]
fn shortcut_path() -> Result<PathBuf, String> {
    Ok(startup_dir()?.join("NexusPrime.lnk"))
}

/// 启用/禁用开机自启。新入口使用 `--autostart`，旧版快捷方式仅作兼容检测。
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if enable {
            // 先写入规范入口，避免迁移失败时意外丢失已有自启。
            set_run_key(true)?;
            remove_startup_shortcut()?;
        } else {
            // 禁用时两处都清理，兼容旧版 Startup 快捷方式。
            remove_startup_shortcut()?;
            set_run_key(false)?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Err("仅支持 Windows".into())
    }
}

pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        run_key_exists() || shortcut_path().map(|p| p.is_file()).unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn set_run_key(enable: bool) -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
        REG_SZ,
    };

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let value = format!("\"{}\" {}", exe.display(), crate::AUTOSTART_ARGUMENT);
    let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let name = w!("NexusPrime");

    unsafe {
        let mut key = Default::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_WRITE,
            &mut key,
        )
        .ok()
        .map_err(|e| format!("RegOpenKeyExW: {e}"))?;

        let result = if enable {
            let bytes = std::slice::from_raw_parts(
                value_wide.as_ptr() as *const u8,
                value_wide.len() * 2,
            );
            RegSetValueExW(key, name, 0, REG_SZ, Some(bytes))
        } else {
            // 删除；不存在也算成功
            let _ = RegDeleteValueW(key, name);
            ERROR_SUCCESS
        };
        let _ = RegCloseKey(key);
        if result != ERROR_SUCCESS && enable {
            return Err(format!("RegSetValueExW failed {result:?}"));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_key_exists() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
    };
    unsafe {
        let mut key = Default::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let mut data_len = 0u32;
        let mut ty = REG_VALUE_TYPE::default();
        let q = RegQueryValueExW(
            key,
            w!("NexusPrime"),
            None,
            Some(&mut ty),
            None,
            Some(&mut data_len),
        );
        let _ = RegCloseKey(key);
        q.is_ok()
    }
}

#[cfg(target_os = "windows")]
fn remove_startup_shortcut() -> Result<(), String> {
    let link = shortcut_path()?;
    match std::fs::remove_file(&link) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove legacy startup shortcut {}: {error}", link.display())),
    }
}
