//! Windows 后台控制台命令。
//!
//! GUI 子进程直接启动 `icacls`、`netstat` 等控制台程序时，Windows 会短暂
//! 创建黑色控制台窗口。该 helper 保留输出管道，只禁止创建可见控制台。

use std::ffi::OsStr;
use std::process::Command;

pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Only for bounded software helpers; never use this to terminate a driver installer.
pub fn software_helper_output(
    command: &mut Command,
    timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().ok_or("helper stdout missing")?;
    let mut stderr = child.stderr.take().ok_or("helper stderr missing")?;
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let started = std::time::Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            other => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("软件辅助进程超时或失败: {other:?}"));
            }
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    result.map(|status| std::process::Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn background_command_keeps_output_capture_available() {
        let output = background_command("cmd.exe")
            .args(["/D", "/S", "/C", "echo nexus-prime"])
            .output()
            .unwrap();

        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("nexus-prime"));
    }
}
