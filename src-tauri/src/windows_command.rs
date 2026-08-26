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
