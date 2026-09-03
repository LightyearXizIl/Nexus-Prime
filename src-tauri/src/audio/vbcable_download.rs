//! 受限的 VB-CABLE 官方包下载：只保存已完整校验的 ZIP，不执行安装。

use crate::audio::vb_cable::{DOWNLOAD_ZIP_URL, DRIVER_ZIP_SHA256};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    percent: Option<u8>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadComplete {
    final_path: String,
    downloaded_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadError {
    message: String,
}

pub fn start(app: AppHandle, destination: PathBuf) -> Result<(), String> {
    if destination.as_os_str().is_empty() {
        return Err("下载保存路径不能为空".into());
    }
    if !destination
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err("下载文件必须保存为 .zip".into());
    }
    if DOWNLOADING.swap(true, Ordering::AcqRel) {
        return Err("已有 VB-CABLE 下载正在进行".into());
    }
    CANCEL_REQUESTED.store(false, Ordering::Release);

    std::thread::Builder::new()
        .name("vbcable-download".into())
        .spawn(move || {
            let temporary = temporary_path_for(&destination);
            let result = download_verified(&app, &destination, &temporary);
            if let Err(error) = result {
                // 失败或取消只影响本次临时文件，绝不触碰用户已有目标文件。
                let _ = fs::remove_file(&temporary);
                let _ = app.emit("vbcable-download-error", DownloadError { message: error });
            }
            DOWNLOADING.store(false, Ordering::Release);
            CANCEL_REQUESTED.store(false, Ordering::Release);
        })
        .map_err(|error| {
            DOWNLOADING.store(false, Ordering::Release);
            format!("启动下载线程失败: {error}")
        })?;
    Ok(())
}

pub fn cancel() -> bool {
    if !DOWNLOADING.load(Ordering::Acquire) {
        return false;
    }
    CANCEL_REQUESTED.store(true, Ordering::Release);
    true
}

fn temporary_path_for(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("VBCABLE_Driver_Pack45.zip");
    destination.with_file_name(format!(".{name}.nexus-prime.part"))
}

fn download_verified(app: &AppHandle, destination: &Path, temporary: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "下载保存路径没有父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建保存目录失败: {error}"))?;
    if temporary.exists() {
        return Err("已有未完成下载临时文件，请稍后重试".into());
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| format!("创建下载连接失败: {error}"))?;
    let mut response = client
        .get(DOWNLOAD_ZIP_URL)
        .send()
        .map_err(|error| format!("下载 VB-CABLE 官方包失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("官方下载返回 HTTP {}", response.status()));
    }
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
        return Err(format!(
            "官方下载文件超过 {} MiB 限制",
            MAX_DOWNLOAD_BYTES / 1024 / 1024
        ));
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| format!("创建临时下载文件失败: {error}"))?;
    let mut hash = Sha256::new();
    let mut downloaded = 0u64;
    let mut head = Vec::with_capacity(4);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if CANCEL_REQUESTED.load(Ordering::Acquire) {
            return Err("下载已取消".into());
        }
        let read = response
            .read(&mut chunk)
            .map_err(|error| format!("读取下载数据失败: {error}"))?;
        if read == 0 {
            break;
        }
        downloaded = downloaded.saturating_add(read as u64);
        if downloaded > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "下载内容超过 {} MiB 限制",
                MAX_DOWNLOAD_BYTES / 1024 / 1024
            ));
        }
        if head.len() < 4 {
            let remaining = 4 - head.len();
            head.extend_from_slice(&chunk[..read.min(remaining)]);
        }
        file.write_all(&chunk[..read])
            .map_err(|error| format!("写入临时下载文件失败: {error}"))?;
        hash.update(&chunk[..read]);
        let percent =
            total.map(|size| ((downloaded.saturating_mul(100) / size.max(1)).min(100)) as u8);
        let _ = app.emit(
            "vbcable-download-progress",
            DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes: total,
                percent,
            },
        );
    }
    file.flush()
        .map_err(|error| format!("刷新临时文件失败: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("同步临时文件失败: {error}"))?;
    if let Some(expected) = total.filter(|expected| *expected != downloaded) {
        return Err(format!(
            "下载长度不匹配：期望 {expected} 字节，实际 {downloaded} 字节"
        ));
    }
    if !matches!(head.as_slice(), [b'P', b'K', 3, 4] | [b'P', b'K', 5, 6]) {
        return Err("官方下载内容不是 ZIP 文件".into());
    }
    let actual_hash = format!("{:x}", hash.finalize());
    if actual_hash != DRIVER_ZIP_SHA256 {
        return Err("官方下载文件校验失败（SHA-256 不匹配）".into());
    }

    replace_verified_file(temporary, destination)?;
    app.emit(
        "vbcable-download-complete",
        DownloadComplete {
            final_path: destination.display().to_string(),
            downloaded_bytes: downloaded,
        },
    )
    .map_err(|error| format!("通知下载完成失败: {error}"))?;
    Ok(())
}

/// 仅在所有校验完成后替换目标。第二次移动失败时恢复原文件。
fn replace_verified_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .map_err(|error| format!("保存下载文件失败: {error}"));
    }
    let backup = destination.with_file_name(format!(
        ".{}.nexus-prime.previous",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download.zip")
    ));
    if backup.exists() {
        return Err("发现同名恢复文件，请先处理后重试".into());
    }
    fs::rename(destination, &backup).map_err(|error| format!("准备替换原下载文件失败: {error}"))?;
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(format!("保存下载文件失败，原文件已恢复: {error}"));
    }
    fs::remove_file(&backup).map_err(|error| format!("新文件已保存，但清理旧备份失败: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_file_stays_next_to_destination() {
        let destination = PathBuf::from(r"C:\\temp\\driver.zip");
        assert_eq!(
            temporary_path_for(&destination),
            PathBuf::from(r"C:\\temp\\.driver.zip.nexus-prime.part")
        );
    }

    #[test]
    fn replacing_verified_file_preserves_existing_target_until_the_final_step() {
        let base =
            std::env::temp_dir().join(format!("nexus-prime-download-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&base);
        let destination = base.join("driver.zip");
        let temporary = base.join(".driver.zip.nexus-prime.part");
        fs::write(&destination, b"old").unwrap();
        fs::write(&temporary, b"new").unwrap();
        replace_verified_file(&temporary, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(!temporary.exists());
        let _ = fs::remove_file(&destination);
        let _ = fs::remove_dir(&base);
    }
}
