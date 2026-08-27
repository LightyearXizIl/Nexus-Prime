//! 本地运行日志：语义事件按天保存，后台写盘，默认保留最近七天。

use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_RETENTION_DAYS: usize = 7;
const MAX_SEGMENT_BYTES: u64 = 10 * 1024 * 1024;
const QUEUE_CAPACITY: usize = 8_192;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static RETENTION_DAYS: AtomicUsize = AtomicUsize::new(DEFAULT_RETENTION_DAYS);
static WRITER: OnceLock<LogWriter> = OnceLock::new();

#[derive(Clone)]
struct LogWriter {
    sender: mpsc::SyncSender<String>,
    last_error: Arc<Mutex<Option<String>>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLogFile {
    pub name: String,
    pub size: u64,
    pub current: bool,
}

fn valid_retention_days(days: usize) -> usize {
    match days {
        1 | 3 | 7 | 14 | 30 => days,
        _ => DEFAULT_RETENTION_DAYS,
    }
}

fn timestamp() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn format_line(level: log::Level, msg: &str) -> String {
    format!("[{}] [{level}] {}\n", timestamp(), msg.replace(['\r', '\n'], " "))
}

fn is_log_file(name: &str) -> bool {
    (name.starts_with("app-") && name.ends_with(".log")) || matches!(name, "app.log" | "app.log.1")
}

fn active_log_path(dir: &Path) -> PathBuf {
    let day = chrono::Local::now().format("%Y-%m-%d");
    let base = dir.join(format!("app-{day}.log"));
    if fs::metadata(&base).map(|meta| meta.len() < MAX_SEGMENT_BYTES).unwrap_or(true) {
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

fn start_writer(dir: PathBuf, cleanup_enabled: bool) -> LogWriter {
    let (sender, receiver) = mpsc::sync_channel::<String>(QUEUE_CAPACITY);
    let last_error = Arc::new(Mutex::new(None));
    let error_slot = Arc::clone(&last_error);
    let _ = std::thread::Builder::new()
        .name("nexus-log-writer".into())
        .spawn(move || {
            let mut last_cleanup = Instant::now() - Duration::from_secs(24 * 60 * 60);
            while let Ok(line) = receiver.recv() {
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
                match OpenOptions::new().create(true).append(true).open(&path) {
                    Ok(mut file) => {
                        if let Err(error) = file.write_all(line.as_bytes()).and_then(|_| file.flush()) {
                            *error_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(format!("写入日志失败: {error}"));
                        } else {
                            *error_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        }
                    }
                    Err(error) => {
                        *error_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(format!("打开日志失败: {error}"));
                    }
                }
            }
        });
    LogWriter { sender, last_error }
}

fn enqueue(line: String) {
    let Some(writer) = WRITER.get() else {
        return;
    };
    if writer.sender.try_send(line).is_err() {
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

pub fn log_path() -> Option<PathBuf> {
    LOG_DIR.get().map(|dir| active_log_path(dir))
}

pub fn last_write_error() -> Option<String> {
    WRITER
        .get()
        .and_then(|writer| writer.last_error.lock().ok().and_then(|error| error.clone()))
}

pub fn list_log_files() -> Vec<AppLogFile> {
    let Some(dir) = LOG_DIR.get() else { return Vec::new() };
    let current = active_log_path(dir)
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
        None | Some("") => Ok(active_log_path(dir)),
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
    let text = String::from_utf8_lossy(&fs::read(&path).map_err(|e| format!("读取日志失败: {e}"))?).into_owned();
    if text.chars().count() <= max_chars {
        return Ok(text);
    }
    let skip = text.chars().count().saturating_sub(max_chars);
    Ok(format!("……（仅显示末尾）\n{}", text.chars().skip(skip).collect::<String>()))
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
    append(&format!("EVENT category={category} action={action} outcome={outcome} {details}"));
}

struct TeeLogger {
    file: AppFileLogger,
    console: Option<env_logger::Logger>,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.file.enabled(metadata)
            || self.console.as_ref().map(|console| console.enabled(metadata)).unwrap_or(false)
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
}
