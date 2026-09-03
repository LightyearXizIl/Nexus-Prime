//! GitHub Release 检查、校验下载与启动 NSIS 安装器。

use parking_lot::Mutex;
use semver::Version;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/LightyearXizIl/Nexus-Prime/releases/latest";
const RELEASE_MANIFEST_URL: &str =
    "https://github.com/LightyearXizIl/Nexus-Prime/releases/latest/download/latest.json";
const REPOSITORY_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/LightyearXizIl/Nexus-Prime/main/latest.json";
const MAX_ASSET_SIZE: u64 = 512 * 1024 * 1024;
const USER_AGENT: &str = "Nexus-Prime-Updater";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRelease {
    pub version: String,
    pub title: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub asset_name: String,
    pub asset_size: u64,
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub update: Option<UpdateRelease>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckError {
    pub stage: UpdateCheckStage,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheckStage {
    Network,
    Http,
    Parse,
    Validation,
    Task,
}

impl UpdateCheckError {
    fn new(stage: UpdateCheckStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            retry_after_seconds: None,
        }
    }

    fn rate_limited(retry_after_seconds: Option<u64>) -> Self {
        let message = match retry_after_seconds {
            Some(seconds) if seconds < 60 => "GitHub 请求额度暂时用尽，应用将在不到 1 分钟后自动重试。".into(),
            Some(seconds) => format!(
                "GitHub 请求额度暂时用尽，应用将在约 {} 分钟后自动重试。",
                (seconds + 59) / 60
            ),
            None => "GitHub 请求额度暂时用尽，请稍后重试。".into(),
        };
        Self {
            stage: UpdateCheckStage::Http,
            message,
            retry_after_seconds,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedUpdate {
    pub version: String,
    pub asset_name: String,
    pub asset_size: u64,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    public: UpdateRelease,
    download_url: String,
    sha256: String,
}

struct FetchedRelease {
    release: GithubRelease,
    source: &'static str,
}

#[derive(Default)]
struct UpdateState {
    candidate: Option<Candidate>,
    cancel_download: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
pub struct UpdateManager {
    state: Mutex<UpdateState>,
}

fn normalize_version(raw: &str) -> Result<Version, String> {
    Version::parse(raw.trim().trim_start_matches(['v', 'V']))
        .map_err(|_| format!("无效的版本号：{raw}"))
}

fn parse_sha256(digest: &str) -> Result<String, String> {
    let value = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| "安装包缺少 SHA-256 校验信息".to_string())?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("安装包的 SHA-256 校验信息无效".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn expected_asset_name(version: &Version) -> String {
    // GitHub Release uploads normalize spaces in asset filenames to dots.
    format!("Nexus.Prime_{version}_x64-setup.exe")
}

fn candidate_from_release(
    release: GithubRelease,
    current: &Version,
) -> Result<Option<Candidate>, String> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let latest = normalize_version(&release.tag_name)?;
    if latest <= *current {
        return Ok(None);
    }

    let asset_name = expected_asset_name(&latest);
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| format!("版本 v{latest} 未找到 Windows x64 安装包"))?;
    if asset.size == 0 || asset.size > MAX_ASSET_SIZE {
        return Err("安装包大小不符合安全限制".into());
    }
    if asset.name.contains(['/', '\\']) {
        return Err("安装包文件名不安全".into());
    }
    let sha256 = parse_sha256(
        asset
            .digest
            .as_deref()
            .ok_or_else(|| "安装包缺少 SHA-256 校验信息".to_string())?,
    )?;
    if !asset
        .browser_download_url
        .starts_with("https://github.com/")
    {
        return Err("安装包下载地址不受信任".into());
    }

    Ok(Some(Candidate {
        public: UpdateRelease {
            version: format!("v{latest}"),
            title: release
                .name
                .unwrap_or_else(|| format!("Nexus Prime v{latest}")),
            notes: release
                .body
                .unwrap_or_else(|| "该版本未提供更新说明。".into()),
            published_at: release.published_at,
            asset_name,
            asset_size: asset.size,
            downloaded: false,
        },
        download_url: asset.browser_download_url,
        sha256,
    }))
}

fn updates_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("updates"))
        .map_err(|error| format!("无法定位更新缓存目录：{error}"))
}

fn installer_path(app: &AppHandle, candidate: &Candidate) -> Result<PathBuf, String> {
    Ok(updates_dir(app)?.join(&candidate.public.asset_name))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("读取安装包失败：{error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("读取安装包失败：{error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_cached_installer(path: &Path, candidate: &Candidate) -> bool {
    fs::metadata(path)
        .map(|meta| meta.len() == candidate.public.asset_size)
        .unwrap_or(false)
        && sha256_file(path)
            .map(|actual| actual == candidate.sha256)
            .unwrap_or(false)
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    if let Some(seconds) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Some(seconds);
    }

    let reset_at = headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(reset_at.saturating_sub(now))
}

fn http_error(response: &reqwest::blocking::Response) -> UpdateCheckError {
    let status = response.status();
    let headers = response.headers();
    let rate_limited = status == reqwest::StatusCode::FORBIDDEN
        && headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            == Some("0");
    if rate_limited {
        return UpdateCheckError::rate_limited(retry_after_seconds(headers));
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return UpdateCheckError::new(UpdateCheckStage::Http, "尚未发布可用的正式更新。");
    }
    UpdateCheckError::new(
        UpdateCheckStage::Http,
        format!("更新服务返回 HTTP {}，请稍后重试。", status.as_u16()),
    )
}

fn fetch_json<T: DeserializeOwned>(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<T, UpdateCheckError> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Network, format!("无法连接更新服务：{error}")))?;
    if !response.status().is_success() {
        return Err(http_error(&response));
    }
    response
        .json::<T>()
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Parse, format!("读取更新信息失败：{error}")))
}

fn fetch_latest_release() -> Result<FetchedRelease, UpdateCheckError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Network, format!("创建更新请求失败：{error}")))?;

    for (url, source) in [
        (RELEASE_MANIFEST_URL, "release-manifest"),
        (REPOSITORY_MANIFEST_URL, "repository-manifest"),
    ] {
        match fetch_json::<GithubRelease>(&client, url) {
            Ok(release) => return Ok(FetchedRelease { release, source }),
            Err(error) if matches!(error.stage, UpdateCheckStage::Network | UpdateCheckStage::Http) => continue,
            Err(error) => return Err(error),
        }
    }

    fetch_json::<GithubRelease>(&client, LATEST_RELEASE_URL).map(|release| FetchedRelease {
        release,
        source: "github-api",
    })
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    manager: State<'_, UpdateManager>,
) -> Result<UpdateCheckResult, UpdateCheckError> {
    let current_version = app.package_info().version.to_string();
    let current = normalize_version(&current_version)
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Validation, error))?;
    let fetched = tokio::task::spawn_blocking(fetch_latest_release)
        .await
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Task, format!("更新检查任务失败：{error}")))??;
    let mut candidate = candidate_from_release(fetched.release, &current)
        .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Validation, error))?;
    if let Some(found) = candidate.as_mut() {
        let path = installer_path(&app, found)
            .map_err(|error| UpdateCheckError::new(UpdateCheckStage::Validation, error))?;
        found.public.downloaded = verify_cached_installer(&path, found);
    }
    manager.state.lock().candidate = candidate.clone();
    Ok(UpdateCheckResult {
        current_version: format!("v{current}"),
        update: candidate.map(|value| value.public),
        source: fetched.source.into(),
    })
}

fn emit_progress(app: &AppHandle, downloaded: u64, total: u64) {
    let percent = ((downloaded.saturating_mul(100) / total.max(1)).min(100)) as u8;
    let _ = app.emit(
        "update-download-progress",
        UpdateDownloadProgress {
            downloaded_bytes: downloaded,
            total_bytes: total,
            percent,
        },
    );
}

fn download_candidate(
    app: &AppHandle,
    candidate: &Candidate,
    cancelled: &AtomicBool,
) -> Result<DownloadedUpdate, String> {
    let destination = installer_path(app, candidate)?;
    if verify_cached_installer(&destination, candidate) {
        emit_progress(
            app,
            candidate.public.asset_size,
            candidate.public.asset_size,
        );
        return Ok(DownloadedUpdate {
            version: candidate.public.version.clone(),
            asset_name: candidate.public.asset_name.clone(),
            asset_size: candidate.public.asset_size,
        });
    }
    let directory = destination
        .parent()
        .ok_or_else(|| "更新缓存目录无效".to_string())?;
    fs::create_dir_all(directory).map_err(|error| format!("创建更新缓存目录失败：{error}"))?;
    let part = destination.with_extension("exe.part");
    let outcome = (|| -> Result<DownloadedUpdate, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // 限制整个安装包下载时长，避免后台任务无限挂起；12MB 左右的正式包仍留有充足余量。
            .timeout(Duration::from_secs(10 * 60))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|error| format!("创建下载请求失败：{error}"))?;
        let mut response = client
            .get(&candidate.download_url)
            .send()
            .map_err(|error| format!("下载安装包失败：{error}"))?
            .error_for_status()
            .map_err(|error| format!("下载安装包失败：{error}"))?;
        let mut output =
            fs::File::create(&part).map_err(|error| format!("创建临时安装包失败：{error}"))?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        emit_progress(app, 0, candidate.public.asset_size);
        loop {
            if cancelled.load(Ordering::SeqCst) {
                return Err("下载已取消".into());
            }
            let count = response
                .read(&mut buffer)
                .map_err(|error| format!("下载安装包失败：{error}"))?;
            if count == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(count as u64);
            if downloaded > candidate.public.asset_size {
                return Err("安装包大小校验失败".into());
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("写入安装包失败：{error}"))?;
            hasher.update(&buffer[..count]);
            if last_emit.elapsed() >= Duration::from_millis(100) {
                emit_progress(app, downloaded, candidate.public.asset_size);
                last_emit = Instant::now();
            }
        }
        output
            .flush()
            .map_err(|error| format!("保存安装包失败：{error}"))?;
        if downloaded != candidate.public.asset_size {
            return Err("安装包下载不完整，请重试".into());
        }
        let digest = format!("{:x}", hasher.finalize());
        if digest != candidate.sha256 {
            return Err("安装包 SHA-256 校验失败，请重试".into());
        }
        if destination.exists() {
            fs::remove_file(&destination).map_err(|error| format!("替换旧安装包失败：{error}"))?;
        }
        fs::rename(&part, &destination).map_err(|error| format!("完成安装包下载失败：{error}"))?;
        emit_progress(app, downloaded, candidate.public.asset_size);
        Ok(DownloadedUpdate {
            version: candidate.public.version.clone(),
            asset_name: candidate.public.asset_name.clone(),
            asset_size: candidate.public.asset_size,
        })
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&part);
    }
    outcome
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    manager: State<'_, UpdateManager>,
) -> Result<DownloadedUpdate, String> {
    let (candidate, cancelled) = {
        let mut state = manager.state.lock();
        if state.cancel_download.is_some() {
            return Err("更新正在下载中".into());
        }
        let candidate = state
            .candidate
            .clone()
            .ok_or_else(|| "没有可下载的更新，请先重新检查更新".to_string())?;
        let cancelled = Arc::new(AtomicBool::new(false));
        state.cancel_download = Some(Arc::clone(&cancelled));
        (candidate, cancelled)
    };
    let app_for_download = app.clone();
    let joined = tokio::task::spawn_blocking(move || {
        download_candidate(&app_for_download, &candidate, cancelled.as_ref())
    })
    .await;
    manager.state.lock().cancel_download = None;
    joined.map_err(|error| format!("下载任务失败：{error}"))?
}

#[tauri::command]
pub fn cancel_update_download(manager: State<'_, UpdateManager>) {
    if let Some(cancelled) = manager.state.lock().cancel_download.as_ref() {
        cancelled.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
pub fn install_downloaded_update(
    app: AppHandle,
    manager: State<'_, UpdateManager>,
) -> Result<(), String> {
    let candidate = manager
        .state
        .lock()
        .candidate
        .clone()
        .ok_or_else(|| "没有可安装的更新".to_string())?;
    let installer = installer_path(&app, &candidate)?;
    if !verify_cached_installer(&installer, &candidate) {
        return Err("安装包尚未完成或校验失败，请重新下载".into());
    }
    Command::new(&installer)
        .spawn()
        .map_err(|error| format!("启动安装程序失败：{error}"))?;
    crate::ipc::tray::quit_app_public(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_prefixed_versions() {
        assert_eq!(normalize_version("v0.0.7").unwrap(), Version::new(0, 0, 7));
        assert!(normalize_version("latest").is_err());
    }

    #[test]
    fn validates_sha256_digest() {
        assert!(parse_sha256(
            "sha256:071ac25704f277a22e7b1763934aaa6fe1a1339ce1f2234198ee6fefea356374"
        )
        .is_ok());
        assert!(parse_sha256("sha256:not-a-hash").is_err());
    }

    #[test]
    fn only_selects_exact_x64_setup_asset() {
        let release = GithubRelease {
            tag_name: "v0.0.7".into(),
            name: None,
            body: None,
            published_at: None,
            draft: false,
            prerelease: false,
            assets: vec![GithubAsset {
                name: "Nexus.Prime_0.0.7_x64-setup.exe".into(),
                size: 42,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                browser_download_url: "https://github.com/LightyearXizIl/Nexus-Prime/releases/download/v0.0.7/Nexus%20Prime_0.0.7_x64-setup.exe".into(),
            }],
        };
        let candidate = candidate_from_release(release, &Version::new(0, 0, 6))
            .unwrap()
            .unwrap();
        assert_eq!(candidate.public.version, "v0.0.7");
    }

    #[test]
    fn reads_github_rate_limit_reset_as_retry_delay() {
        let mut headers = reqwest::header::HeaderMap::new();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        headers.insert(
            "x-ratelimit-reset",
            reqwest::header::HeaderValue::from_str(&(now + 120).to_string()).unwrap(),
        );

        let delay = retry_after_seconds(&headers).unwrap();
        assert!((119..=120).contains(&delay));
    }

    #[test]
    fn exposes_a_safe_rate_limit_error() {
        let error = UpdateCheckError::rate_limited(Some(90));
        let value = serde_json::to_value(error).unwrap();

        assert_eq!(value["stage"], "http");
        assert_eq!(value["retryAfterSeconds"], 90);
        assert!(value["message"].as_str().unwrap().contains("自动重试"));
    }
}
