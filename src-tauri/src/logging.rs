//! 本地运行日志：语义事件按天保存，后台写盘，默认保留最近七天。

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_RETENTION_DAYS: usize = 7;
const MAX_SEGMENT_BYTES: u64 = 10 * 1024 * 1024;
const QUEUE_CAPACITY: usize = 8_192;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static RETENTION_DAYS: AtomicUsize = AtomicUsize::new(DEFAULT_RETENTION_DAYS);
static WRITER: OnceLock<LogWriter> = OnceLock::new();
static SESSION_ENVIRONMENT: OnceLock<String> = OnceLock::new();
/// 单调递增的日志事件序号。它跨日志文件保持连续，便于把异步 BLE、音频和
/// 前端事件重新排序；进程重启后从 1 重新开始。
static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct LogWriter {
    sender: mpsc::SyncSender<WriterMessage>,
    last_error: Arc<Mutex<Option<String>>>,
    last_written_path: Arc<Mutex<Option<PathBuf>>>,
}

enum WriterMessage {
    Line(String),
    Flush(mpsc::SyncSender<Result<(), String>>),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLogFile {
    pub name: String,
    pub size: u64,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogCleanupResult {
    pub deleted_files: usize,
    pub freed_bytes: u64,
}

fn valid_retention_days(days: usize) -> usize {
    match days {
        1 | 3 | 7 | 14 | 30 => days,
        _ => DEFAULT_RETENTION_DAYS,
    }
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn format_line(level: log::Level, msg: &str) -> String {
    let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let utc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .replace([' ', '\t', '\r', '\n'], "_");
    format!(
        "[{}] [utc={utc}] [seq={sequence}] [thread={thread}] [{level}] {}\n",
        timestamp(),
        msg.replace(['\r', '\n'], " ")
    )
}

fn is_log_file(name: &str) -> bool {
    (name.starts_with("app-") && name.ends_with(".log")) || matches!(name, "app.log" | "app.log.1")
}

fn active_log_path(dir: &Path) -> PathBuf {
    let day = chrono::Local::now().format("%Y-%m-%d");
    let base = dir.join(format!("app-{day}.log"));
    if fs::metadata(&base)
        .map(|meta| meta.len() < MAX_SEGMENT_BYTES)
        .unwrap_or(true)
    {
        return base;
    }
    for segment in 1u32.. {
        let path = dir.join(format!("app-{day}.{segment}.log"));
        if fs::metadata(&path)
            .map(|meta| meta.len() < MAX_SEGMENT_BYTES)
            .unwrap_or(true)
        {
            return path;
        }
    }
    unreachable!("unbounded log segment counter")
}

fn cleanup_logs(dir: &Path, retention_days: usize) -> Result<(), String> {
    let cutoff = chrono::Local::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(retention_days.saturating_sub(1) as u64))
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_log_file(name) {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(chrono::DateTime::<chrono::Local>::from)
            .map(|time| time.date_naive());
        if modified.is_some_and(|date| date < cutoff) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn is_current_day_log(name: &str) -> bool {
    let prefix = format!("app-{}", chrono::Local::now().format("%Y-%m-%d"));
    name.starts_with(&prefix) && is_log_file(name)
}

fn clear_old_logs_in(dir: &Path) -> Result<LogCleanupResult, String> {
    let mut result = LogCleanupResult {
        deleted_files: 0,
        freed_bytes: 0,
    };
    let entries = fs::read_dir(dir).map_err(|e| format!("读取日志目录失败: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取日志文件失败: {e}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_log_file(&name) || is_current_day_log(&name) {
            continue;
        }
        let size = entry.metadata().map(|meta| meta.len()).unwrap_or_default();
        match fs::remove_file(&path) {
            Ok(()) => {
                result.deleted_files += 1;
                result.freed_bytes = result.freed_bytes.saturating_add(size);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("清理日志 {} 失败: {error}", path.display())),
        }
    }
    Ok(result)
}

fn start_writer(dir: PathBuf, cleanup_enabled: bool) -> LogWriter {
    let (sender, receiver) = mpsc::sync_channel::<WriterMessage>(QUEUE_CAPACITY);
    let last_error = Arc::new(Mutex::new(None));
    let error_slot = Arc::clone(&last_error);
    let last_written_path = Arc::new(Mutex::new(None));
    let written_path_slot = Arc::clone(&last_written_path);
    let _ = std::thread::Builder::new()
        .name("nexus-log-writer".into())
        .spawn(move || {
            let mut last_cleanup = Instant::now() - Duration::from_secs(24 * 60 * 60);
            let mut previous_path = None;
            while let Ok(message) = receiver.recv() {
                let line = match message {
                    WriterMessage::Line(line) => line,
                    WriterMessage::Flush(reply) => {
                        let result = error_slot
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone()
                            .map_or(Ok(()), Err);
                        let _ = reply.send(result);
                        continue;
                    }
                };
                if let Err(error) = fs::create_dir_all(&dir) {
                    *error_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(format!("创建日志目录失败: {error}"));
                    continue;
                }
                if cleanup_enabled && last_cleanup.elapsed() >= Duration::from_secs(24 * 60 * 60) {
                    let _ = cleanup_logs(&dir, RETENTION_DAYS.load(Ordering::Acquire));
                    last_cleanup = Instant::now();
                }
                let path = active_log_path(&dir);
                // 每个会话以及跨日/分段后的文件都自带环境，不依赖另外一个文件。
                let environment_header = (previous_path.as_ref() != Some(&path))
                    .then(environment_snapshot_line);
                match OpenOptions::new().create(true).append(true).open(&path) {
                    Ok(mut file) => {
                        let result = (|| {
                            if let Some(header) = environment_header.as_deref() {
                                file.write_all(header.as_bytes())?;
                            }
                            file.write_all(line.as_bytes())?;
                            file.flush()
                        })();
                        if let Err(error) = result {
                            *error_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(format!("写入日志失败: {error}"));
                        } else {
                            *error_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                            *written_path_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
                            previous_path = Some(path);
                        }
                    }
                    Err(error) => {
                        *error_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(format!("打开日志失败: {error}"));
                    }
                }
            }
        });
    LogWriter { sender, last_error, last_written_path }
}

fn enqueue(line: String) {
    let Some(writer) = WRITER.get() else {
        return;
    };
    if writer.sender.try_send(WriterMessage::Line(line)).is_err() {
        *writer.last_error.lock().unwrap_or_else(|e| e.into_inner()) =
            Some("日志队列已满，部分日志未写入".into());
    }
}

struct AppFileLogger;

impl log::Log for AppFileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target();
        let ours = target.starts_with("remote_bridge_hub") || target.starts_with("nexus_prime_lib");
        if !ours && record.level() > log::Level::Warn {
            return;
        }
        enqueue(format_line(record.level(), &record.args().to_string()));
    }

    fn flush(&self) {}
}

/// 初始化日志；返回当前日期的默认日志路径。
pub fn init(logs_dir: &Path, retention_days: usize) -> PathBuf {
    let retention_days = valid_retention_days(retention_days);
    RETENTION_DAYS.store(retention_days, Ordering::Release);
    let _ = fs::create_dir_all(logs_dir);
    let _ = cleanup_logs(logs_dir, retention_days);
    let _ = LOG_DIR.set(logs_dir.to_path_buf());
    cache_session_environment();
    let _ = WRITER.set(start_writer(logs_dir.to_path_buf(), true));

    let console = std::env::var_os("RUST_LOG").is_some();
    if console {
        let env = env_logger::Builder::from_default_env().build();
        let _ = log::set_boxed_logger(Box::new(TeeLogger {
            file: AppFileLogger,
            console: Some(env),
        }));
    } else {
        let _ = log::set_boxed_logger(Box::new(AppFileLogger));
    }
    log::set_max_level(log::LevelFilter::Info);
    append("—— 应用启动 ——");
    active_log_path(logs_dir)
}

/// 等待已入队的日志落盘，确保复制/导出包含刚刚记录的诊断快照。
pub fn flush() -> Result<(), String> {
    let writer = WRITER.get().ok_or_else(|| "日志尚未初始化".to_string())?;
    let (reply, result) = mpsc::sync_channel(1);
    writer
        .sender
        .try_send(WriterMessage::Flush(reply))
        .map_err(|_| "日志队列忙，暂时无法读取完整日志".to_string())?;
    result
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "等待日志写入超时，请重试".to_string())?
}

/// 子进程与主进程写入相同的按日目录。
pub fn init_from_env() {
    let dir = std::env::var("REMOTE_BRIDGE_LOG_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("REMOTE_BRIDGE_LOG_PATH")
                .ok()
                .map(PathBuf::from)
                .and_then(|path| path.parent().map(Path::to_path_buf))
        });
    let Some(dir) = dir else { return };
    let days = std::env::var("REMOTE_BRIDGE_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map(valid_retention_days)
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    RETENTION_DAYS.store(days, Ordering::Release);
    let _ = fs::create_dir_all(&dir);
    let _ = LOG_DIR.set(dir.clone());
    cache_session_environment();
    // The main process owns retention.  A child must never race a changed
    // retention setting and delete the parent's historical files.
    let _ = WRITER.set(start_writer(dir, false));
    let _ = log::set_boxed_logger(Box::new(AppFileLogger));
    log::set_max_level(log::LevelFilter::Info);
}

pub fn set_retention_days(days: usize) -> usize {
    let days = valid_retention_days(days);
    RETENTION_DAYS.store(days, Ordering::Release);
    if let Some(dir) = LOG_DIR.get() {
        let _ = cleanup_logs(dir, days);
    }
    days
}

pub fn clear_old_logs() -> Result<LogCleanupResult, String> {
    let dir = LOG_DIR.get().ok_or_else(|| "日志尚未初始化".to_string())?;
    clear_old_logs_in(dir)
}

pub fn log_path() -> Option<PathBuf> {
    WRITER.get()
        .and_then(|writer| writer.last_written_path.lock().ok().and_then(|path| path.clone()))
        .filter(|path| is_current_log_path(path))
        .or_else(|| LOG_DIR.get().map(|dir| active_log_path(dir)))
}

fn is_current_log_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_current_day_log)
}

pub fn last_write_error() -> Option<String> {
    WRITER.get().and_then(|writer| {
        writer
            .last_error
            .lock()
            .ok()
            .and_then(|error| error.clone())
    })
}

pub fn list_log_files() -> Vec<AppLogFile> {
    let Some(dir) = LOG_DIR.get() else {
        return Vec::new();
    };
    let current = log_path().unwrap_or_else(|| active_log_path(dir))
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let mut files = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_log_file(&name) {
                return None;
            }
            Some(AppLogFile {
                current: name == current,
                size: entry.metadata().map(|meta| meta.len()).unwrap_or_default(),
                name,
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| right.name.cmp(&left.name));
    files
}

fn resolve_log_file(name: Option<&str>) -> Result<PathBuf, String> {
    let dir = LOG_DIR.get().ok_or_else(|| "日志尚未初始化".to_string())?;
    match name {
        None | Some("") => Ok(log_path().unwrap_or_else(|| active_log_path(dir))),
        Some(name) => list_log_files()
            .into_iter()
            .find(|file| file.name == name)
            .map(|file| dir.join(file.name))
            .ok_or_else(|| "日志文件不存在或名称无效".to_string()),
    }
}

pub fn read_log_text_for(name: Option<&str>, max_chars: usize) -> Result<String, String> {
    let path = resolve_log_file(name)?;
    if !path.exists() {
        return Ok(String::new());
    }
    let text = String::from_utf8_lossy(&fs::read(&path).map_err(|e| format!("读取日志失败: {e}"))?)
        .into_owned();
    Ok(log_text_with_environment(&text, max_chars))
}

fn is_environment_line(line: &str) -> bool {
    line.contains("EVENT category=environment action=snapshot ")
        || line.contains("EVENT category=diagnostics action=snapshot ")
}

fn log_text_with_environment(text: &str, max_chars: usize) -> String {
    let environment_lines = text.lines().filter(|line| is_environment_line(line));
    let has_environment = environment_lines.clone().next().is_some();
    let missing_environment = if has_environment {
        ""
    } else {
        "【电脑环境】此日志未采集电脑环境（旧版日志或采集失败），无法从本文件还原当时环境。\n"
    };
    let character_count = text.chars().count();
    if character_count <= max_chars {
        return format!("{missing_environment}{text}");
    }
    let skip = character_count.saturating_sub(max_chars);
    let byte_offset = text.char_indices().nth(skip).map_or(text.len(), |(offset, _)| offset);
    // 按整行截取，避免截断 JSON 环境记录；把被截掉的环境记录一起保留。
    let tail_start = text[byte_offset..]
        .find('\n')
        .map_or(text.len(), |offset| byte_offset + offset + 1);
    let preserved_environment = text[..tail_start]
        .lines()
        .filter(|line| is_environment_line(line))
        .collect::<Vec<_>>()
        .join("\n");
    let preserved_header = if preserved_environment.is_empty() {
        String::new()
    } else {
        format!("【所选日志内的电脑环境与运行状态】\n{preserved_environment}\n")
    };
    format!(
        "{missing_environment}{preserved_header}……（日志正文仅显示末尾 {max_chars} 字符，已保留原文件中的环境记录）\n{}",
        &text[tail_start..]
    )
}

pub fn read_log_text(max_chars: usize) -> Result<String, String> {
    read_log_text_for(None, max_chars)
}

pub fn open_log_in_editor_for(name: Option<&str>) -> Result<(), String> {
    let path = resolve_log_file(name)?;
    if !path.exists() {
        append("（日志文件已创建）");
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("打开日志失败: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("仅支持 Windows".into())
    }
}

pub fn open_log_in_editor() -> Result<(), String> {
    open_log_in_editor_for(None)
}

pub fn append(message: &str) {
    enqueue(format_line(log::Level::Info, message));
}

pub fn append_event(category: &str, action: &str, outcome: &str, details: &str) {
    append(&format!(
        "EVENT category={category} action={action} outcome={outcome} {details}"
    ));
}

fn cache_session_environment() {
    let snapshot = collect_environment_snapshot();
    let value = serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null);
    let details = redact_json_value(value).to_string();
    let _ = SESSION_ENVIRONMENT.set(details);
}

fn environment_snapshot_line() -> String {
    // 仅使用带采集时间的会话快照，写盘线程不做设备查询或产生嵌套日志。
    let details = SESSION_ENVIRONMENT.get().map(String::as_str).unwrap_or("null");
    format_line(log::Level::Info, &format!(
        "EVENT category=environment action=snapshot outcome=success source=session_start snapshot={details}"
    ))
}

/// 供诊断包使用的电脑环境摘要。这里故意只保留排障所需的状态，不写入
/// 用户名、完整路径、蓝牙 MAC 或配置文件原文。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub captured_at_local: String,
    pub captured_at_utc: String,
    pub event_sequence: u64,
    pub app_version: String,
    pub build_profile: String,
    pub process_id: u32,
    pub executable_path: String,
    pub executable_sha256: Option<String>,
    pub operating_system: String,
    pub os_version: String,
    pub architecture: String,
    pub cpu_count: Option<usize>,
    pub memory_mb: Option<u64>,
    pub foreground_process: Option<String>,
    pub audio_inputs: Vec<AudioEndpointSnapshot>,
    pub audio_outputs: Vec<AudioEndpointSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEndpointSnapshot {
    pub name: String,
    pub is_default: bool,
    pub format: Option<String>,
}

fn redact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<redacted>\\{name}"))
        .unwrap_or_else(|| "<redacted>".into())
}

fn executable_hash(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(target_os = "windows")]
fn foreground_process_name() -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut name = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|value| *value == 0)
                        .unwrap_or(entry.szExeFile.len());
                    name = Some(String::from_utf16_lossy(&entry.szExeFile[..end]));
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        name
    }
}

#[cfg(not(target_os = "windows"))]
fn foreground_process_name() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn audio_endpoint_names() -> (Vec<AudioEndpointSnapshot>, Vec<AudioEndpointSnapshot>) {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_input = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let default_output = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    let inputs = host
        .input_devices()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|device| {
            let name = device.name().ok()?.chars().take(160).collect::<String>();
            let format = device.default_input_config().ok().map(|config| {
                format!(
                    "{}Hz/{}ch/{:?}",
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format()
                )
            });
            Some(AudioEndpointSnapshot {
                is_default: default_input.as_deref() == Some(name.as_str()),
                name,
                format,
            })
        })
        .collect();
    let outputs = host
        .output_devices()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|device| {
            let name = device.name().ok()?.chars().take(160).collect::<String>();
            let format = device.default_output_config().ok().map(|config| {
                format!(
                    "{}Hz/{}ch/{:?}",
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format()
                )
            });
            Some(AudioEndpointSnapshot {
                is_default: default_output.as_deref() == Some(name.as_str()),
                name,
                format,
            })
        })
        .collect();
    (inputs, outputs)
}

#[cfg(not(target_os = "windows"))]
fn audio_endpoint_names() -> (Vec<AudioEndpointSnapshot>, Vec<AudioEndpointSnapshot>) {
    (Vec::new(), Vec::new())
}

fn windows_version_summary() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("cmd");
        command.creation_flags(0x0800_0000);
        command
            .args(["/C", "ver"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Windows（版本未知）".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::consts::OS.to_string()
    }
}

fn physical_memory_mb() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        // WMIC 已在较新的 Windows 中被移除；失败时保留 None，避免把错误
        // 文本伪装成内存数值。
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("wmic");
        command.creation_flags(0x0800_0000);
        let output = command
            .args(["ComputerSystem", "get", "TotalPhysicalMemory", "/value"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let bytes = text.lines().find_map(|line| {
            line.strip_prefix("TotalPhysicalMemory=")?
                .trim()
                .parse::<u64>()
                .ok()
        })?;
        Some(bytes / 1024 / 1024)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// 收集一次轻量、脱敏的电脑环境快照。命令层再把它和桥接状态、配置摘要
/// 组合成完整诊断包；该函数本身不访问配置文件，也不会修改系统状态。
pub fn collect_environment_snapshot() -> EnvironmentSnapshot {
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("NexusPrime.exe"));
    let (audio_inputs, audio_outputs) = audio_endpoint_names();
    EnvironmentSnapshot {
        captured_at_local: timestamp(),
        captured_at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        event_sequence: EVENT_SEQUENCE.load(Ordering::Acquire),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into(),
        process_id: std::process::id(),
        executable_path: redact_path(&executable),
        executable_sha256: executable_hash(&executable),
        operating_system: std::env::consts::OS.into(),
        os_version: windows_version_summary(),
        architecture: std::env::consts::ARCH.into(),
        cpu_count: std::thread::available_parallelism()
            .ok()
            .map(|value| value.get()),
        memory_mb: physical_memory_mb(),
        foreground_process: foreground_process_name(),
        audio_inputs,
        audio_outputs,
    }
}

/// 复制和导出使用相同脱敏规则；保留引号/JSON 边界，隐藏包含空格的完整路径。
pub fn redact_sensitive_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let is_drive_path = index + 2 < chars.len()
            && chars[index].is_ascii_alphabetic()
            && chars[index + 1] == ':'
            && (chars[index + 2] == '\\' || chars[index + 2] == '/');
        let is_unc_path = index + 2 < chars.len()
            && chars[index] == '\\'
            && chars[index + 1] == '\\'
            && !chars[index + 2].is_whitespace();
        let is_device_path = is_unc_path
            && chars[index..].iter().find(|value| **value != '\\') == Some(&'?');
        let is_mac = index + 16 < chars.len()
            && (0..17).all(|offset| {
                let value = chars[index + offset];
                if offset % 3 == 2 {
                    value == ':' || value == '-'
                } else {
                    value.is_ascii_hexdigit()
                }
            });
        let is_hex_address = index + 13 < chars.len()
            && chars[index] == '0'
            && matches!(chars[index + 1], 'x' | 'X')
            && chars[index + 2..index + 14].iter().all(char::is_ascii_hexdigit)
            && chars.get(index + 14).is_none_or(|value| !value.is_ascii_hexdigit());
        if is_drive_path || is_unc_path {
            output.push_str("<path-redacted>");
            index += if is_drive_path { 3 } else { 2 };
            while index < chars.len()
                && !"\r\n\t\"',;)]".contains(chars[index])
                && (is_device_path || chars[index] != '}')
            {
                // 无引号路径可带空格；遇到下一个 key=value 字段才结束。
                if chars[index] == ' ' {
                    let next = chars[index..]
                        .iter()
                        .position(|value| !value.is_whitespace())
                        .map(|offset| index + offset);
                    if let Some(next) = next {
                        let key_end = chars[next..]
                            .iter()
                            .position(|value| !value.is_ascii_alphanumeric() && *value != '_')
                            .map(|offset| next + offset);
                        if key_end.is_some_and(|end| end > next && chars[end] == '=') {
                            break;
                        }
                    }
                }
                index += 1;
            }
        } else if is_mac {
            output.push_str("<bluetooth-address-redacted>");
            index += 17;
        } else if is_hex_address {
            output.push_str("<bluetooth-address-redacted>");
            index += 14;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

pub fn redact_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => serde_json::Value::String(redact_sensitive_text(&text)),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items.into_iter().map(redact_json_value).collect(),
        ),
        serde_json::Value::Object(items) => serde_json::Value::Object(
            items.into_iter().map(|(key, value)| (key, redact_json_value(value))).collect(),
        ),
        other => other,
    }
}

struct TeeLogger {
    file: AppFileLogger,
    console: Option<env_logger::Logger>,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.file.enabled(metadata)
            || self
                .console
                .as_ref()
                .map(|console| console.enabled(metadata))
                .unwrap_or(false)
    }

    fn log(&self, record: &log::Record) {
        self.file.log(record);
        if let Some(console) = &self.console {
            console.log(record);
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_retention_defaults_to_seven_days() {
        assert_eq!(valid_retention_days(0), 7);
        assert_eq!(valid_retention_days(14), 14);
    }

    #[test]
    fn recognizes_daily_and_legacy_log_names() {
        assert!(is_log_file("app-2026-08-27.log"));
        assert!(is_log_file("app-2026-08-27.2.log"));
        assert!(is_log_file("app.log"));
        assert!(!is_log_file("other.log"));
    }

    #[test]
    fn clear_old_logs_keeps_today_and_non_log_files() {
        let unique = format!(
            "nexus-prime-log-cleanup-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        fs::write(dir.join(format!("app-{today}.log")), b"today").unwrap();
        fs::write(dir.join(format!("app-{today}.1.log")), b"segment").unwrap();
        fs::write(dir.join("app-2000-01-01.log"), b"old").unwrap();
        fs::write(dir.join("app.log"), b"legacy").unwrap();
        fs::write(dir.join("notes.txt"), b"keep").unwrap();

        let result = clear_old_logs_in(&dir).unwrap();

        assert_eq!(result.deleted_files, 2);
        assert_eq!(result.freed_bytes, 9);
        assert!(dir.join(format!("app-{today}.log")).exists());
        assert!(dir.join(format!("app-{today}.1.log")).exists());
        assert!(dir.join("notes.txt").exists());
        assert!(!dir.join("app-2000-01-01.log").exists());
        assert!(!dir.join("app.log").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exported_text_redacts_paths_and_bluetooth_addresses() {
        let text = r"path=C:\Users\alice\Nexus Prime\app.exe remote=C0:5D:39:C2:7E:5B";
        let redacted = redact_sensitive_text(text);
        assert!(!redacted.contains("C:\\Users\\alice"));
        assert!(!redacted.contains("C0:5D:39:C2:7E:5B"));
        assert!(redacted.contains("<path-redacted>"));
        assert!(redacted.contains("<bluetooth-address-redacted>"));
        assert!(!redacted.contains("Nexus Prime\\app.exe"));
    }

    #[test]
    fn redaction_keeps_json_valid_and_removes_spaced_and_device_paths() {
        let value = serde_json::json!({
            "error": r"cannot open C:\Users\alice\Nexus Prime\app.exe status=failed",
            "device": r"\\?\BTHLEDevice#{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4_c05d39c27e5b#8&abc#{00001812-0000-1000-8000-00805f9b34fb}",
            "address": "0xc05d39c27e5b",
        });
        let encoded = serde_json::to_string(&value).unwrap();
        let redacted = redact_sensitive_text(&encoded);
        let parsed: serde_json::Value = serde_json::from_str(&redacted).unwrap();
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("Nexus Prime"));
        assert!(!redacted.contains("c05d39c27e5b"));
        assert!(parsed["error"].as_str().unwrap().contains("status=failed"));
        assert_eq!(parsed["device"], "<path-redacted>");
    }

    #[test]
    fn truncated_log_preserves_its_own_environment_and_diagnostic_records() {
        let text = concat!(
            "EVENT category=environment action=snapshot outcome=success historical_pc=true\n",
            "EVENT category=diagnostics action=snapshot outcome=success atvv=false\n",
            "old event\nold event\nold event\nlatest event\n",
        );
        let result = log_text_with_environment(text, 20);
        assert!(result.contains("historical_pc=true"));
        assert!(result.contains("atvv=false"));
        assert!(result.contains("latest event"));
        assert!(result.contains("仅显示末尾"));
        assert_eq!(result.matches("historical_pc=true").count(), 1);
    }

    #[test]
    fn legacy_log_reports_missing_environment_without_fabricating_a_snapshot() {
        let result = log_text_with_environment("old log entry\n", 80_000);
        assert!(result.contains("此日志未采集电脑环境"));
        assert!(result.contains("old log entry"));
        assert!(!result.contains("snapshot="));
    }

    #[test]
    fn writer_flush_keeps_environment_with_each_new_segment() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-log-segment-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        ));
        fs::create_dir_all(&dir).unwrap();
        let writer = start_writer(dir.clone(), false);
        let flush_writer = || {
            let (reply, receiver) = mpsc::sync_channel(1);
            writer.sender.send(WriterMessage::Flush(reply)).unwrap();
            receiver.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
        };
        writer.sender.send(WriterMessage::Line("first\n".into())).unwrap();
        flush_writer();
        let initial = active_log_path(&dir);
        let text = fs::read_to_string(&initial).unwrap();
        assert!(text.contains("category=environment action=snapshot"));
        assert!(text.ends_with("first\n"));
        OpenOptions::new().write(true).open(&initial).unwrap().set_len(MAX_SEGMENT_BYTES).unwrap();
        // 满段后的下一写目标尚不存在；读取仍必须绑定实际已写的这一段。
        assert_ne!(active_log_path(&dir), initial);
        assert_eq!(*writer.last_written_path.lock().unwrap(), Some(initial.clone()));
        writer.sender.send(WriterMessage::Line("second\n".into())).unwrap();
        flush_writer();
        let next = active_log_path(&dir);
        assert_ne!(initial, next);
        let text = fs::read_to_string(next).unwrap();
        assert!(text.contains("category=environment action=snapshot"));
        assert!(text.ends_with("second\n"));
        drop(writer);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prior_day_written_file_is_not_treated_as_the_current_log() {
        let yesterday = chrono::Local::now().date_naive()
            .checked_sub_days(chrono::Days::new(1)).unwrap();
        assert!(!is_current_log_path(Path::new(&format!("app-{yesterday}.log"))));
        let today = chrono::Local::now().format("%Y-%m-%d");
        assert!(is_current_log_path(Path::new(&format!("app-{today}.2.log"))));
    }

    #[test]
    fn log_line_contains_sequence_and_thread_metadata() {
        let line = format_line(log::Level::Info, "hello");
        assert!(line.contains("[seq="));
        assert!(line.contains("[thread="));
        assert!(line.contains("[INFO]"));
    }
}
