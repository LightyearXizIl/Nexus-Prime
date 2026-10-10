//! Windows default-recording-device routing for the remote microphone.
//!
//! Device policy changes run in a persistent hidden helper process. Windows'
//! undocumented audio policy COM interface is isolated from the Tauri
//! process, so a device or COM failure cannot take down the application.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROUTE_SCRIPT_NAME: &str = "microphone-route.ps1";
const ROUTE_TIMEOUT: Duration = Duration::from_secs(3);
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);
const WORKER_RESTART_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    AlwaysOn,
}

#[derive(Debug, Default)]
struct State {
    mode: Option<Mode>,
    pressed: bool,
    owned_cable: bool,
    previous: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum RouteAction {
    EnsureCable,
    Restore,
}

impl RouteAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::EnsureCable => "EnsureCable",
            Self::Restore => "Restore",
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct RouteResponse {
    ok: bool,
    #[serde(default)]
    changed: bool,
    #[serde(default)]
    skipped: bool,
    #[serde(default)]
    current_id: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    previous_id: Option<String>,
    #[serde(default)]
    message: String,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static ROUTE_OPERATION: OnceLock<Mutex<()>> = OnceLock::new();
static MONITOR: OnceLock<()> = OnceLock::new();
static ROUTE_WORKER: OnceLock<Mutex<Option<RouteWorkerHandle>>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn route_operation() -> &'static Mutex<()> {
    ROUTE_OPERATION.get_or_init(|| Mutex::new(()))
}

fn route_worker() -> &'static Mutex<Option<RouteWorkerHandle>> {
    ROUTE_WORKER.get_or_init(|| Mutex::new(None))
}

fn begin_press(current: &mut State) -> bool {
    if current.pressed {
        return false;
    }
    current.pressed = true;
    true
}

fn end_press(current: &mut State) -> bool {
    if !current.pressed {
        return false;
    }
    current.pressed = false;
    current.mode != Some(Mode::AlwaysOn) && current.owned_cable
}

fn press_requires_route(current: &State) -> bool {
    current.mode != Some(Mode::AlwaysOn)
}

fn commit_ensure(current: &mut State, response: &RouteResponse) {
    if response.changed {
        current.previous = non_empty(response.previous_id.clone())
            .or_else(|| non_empty(response.current_id.clone()));
        current.owned_cable = current.previous.is_some();
    }
}

fn commit_restore(current: &mut State) {
    current.owned_cable = false;
    current.previous = None;
}

/// Apply the saved setting. This is idempotent and safe during startup,
/// settings save, and bridge restart.
pub fn apply_settings(always_on: bool) {
    start_monitor();
    let _operation = route_operation()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    if !always_on {
        let legacy = legacy_previous();
        {
            let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
            if current.previous.is_none() {
                if let Some(previous) = legacy {
                    current.previous = Some(previous);
                    current.owned_cable = true;
                }
            }
        }
        restore_owned();
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.mode = Some(Mode::Off);
        current.pressed = false;
        return;
    }

    {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.mode = Some(Mode::AlwaysOn);
        current.pressed = false;
    }
    ensure_cable();
}

/// Switch to CABLE before the voice shortcut is sent.
pub fn voice_pressed() {
    let _operation = route_operation()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let should_route = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if !begin_press(&mut current) {
            return;
        }
        press_requires_route(&current)
    };
    // Always-on mode already routes the default capture device in the
    // background.  The voice edge must stay free of process creation so it
    // cannot steal focus from the selected input field.
    if should_route {
        ensure_cable();
    }
}

/// Restore only when the application still owns the default endpoint. A user
/// selection made while speaking therefore always wins over the old snapshot.
pub fn voice_released() {
    let _operation = route_operation()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let should_restore = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        end_press(&mut current)
    };
    if should_restore {
        restore_owned();
    }
}

/// Used by disconnect, bridge restart, tray quit, and process exit.
pub fn cleanup(reason: &str) {
    let _operation = route_operation()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let should_restore = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.owned_cable && current.mode != Some(Mode::AlwaysOn)
    };
    if should_restore {
        restore_owned();
    }
    let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
    current.pressed = false;
    if reason == "app_exit" {
        current.mode = Some(Mode::Off);
    }
    if current.mode != Some(Mode::AlwaysOn) {
        current.owned_cable = false;
        current.previous = None;
    }
    shutdown_route_worker();
    log::debug!("microphone route cleanup reason={reason}");
}

fn start_monitor() {
    if MONITOR.set(()).is_ok() {
        let _ = std::thread::Builder::new()
            .name("microphone-route-monitor".into())
            .spawn(|| loop {
                std::thread::sleep(MONITOR_INTERVAL);
                let _operation = route_operation()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let always_on = state()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .mode
                    == Some(Mode::AlwaysOn);
                if always_on {
                    ensure_cable();
                }
            });
    }
}

#[derive(Debug)]
enum WorkerRequest {
    Route {
        action: RouteAction,
        previous: Option<String>,
        reply: SyncSender<Result<RouteResponse, String>>,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
struct RouteWorkerHandle {
    requests: mpsc::Sender<WorkerRequest>,
}

fn ensure_route_worker() -> Result<RouteWorkerHandle, String> {
    let mut slot = route_worker()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(handle) = slot.as_ref() {
        return Ok(handle.clone());
    }

    let (requests, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("microphone-route-worker".into())
        .spawn(move || route_worker_loop(receiver))
        .map_err(|error| format!("start microphone route worker failed: {error}"))?;
    let handle = RouteWorkerHandle { requests };
    *slot = Some(handle.clone());
    log::info!("microphone route worker started");
    Ok(handle)
}

fn shutdown_route_worker() {
    let handle = route_worker()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(handle) = handle {
        let _ = handle.requests.send(WorkerRequest::Shutdown);
        log::info!("microphone route worker stopped");
    }
}

fn request_route(action: RouteAction, previous: Option<&str>) -> Result<RouteResponse, String> {
    let handle = ensure_route_worker()?;
    let (reply, result) = mpsc::sync_channel(1);
    let send_result = handle.requests.send(WorkerRequest::Route {
        action,
        previous: previous.map(str::to_owned),
        reply,
    });
    if let Err(error) = send_result {
        route_worker()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        return Err(format!("send microphone route request failed: {error}"));
    }
    result
        .recv_timeout(ROUTE_TIMEOUT)
        .map_err(|error| format!("microphone route worker timed out: {error}"))?
}

fn route_worker_loop(receiver: Receiver<WorkerRequest>) {
    #[cfg(target_os = "windows")]
    let mut process: Option<RouteProcess> = None;
    #[cfg(target_os = "windows")]
    let mut last_spawn: Option<Instant> = None;

    while let Ok(request) = receiver.recv() {
        match request {
            WorkerRequest::Shutdown => {
                #[cfg(target_os = "windows")]
                if let Some(mut process) = process.take() {
                    let _ = process.child.kill();
                    let _ = process.child.wait();
                }
                return;
            }
            WorkerRequest::Route {
                action,
                previous,
                reply,
            } => {
                #[cfg(target_os = "windows")]
                let result = {
                    let now = Instant::now();
                    if process.is_none()
                        && last_spawn
                            .map(|attempt| now.duration_since(attempt) < WORKER_RESTART_BACKOFF)
                            .unwrap_or(false)
                    {
                        Err("microphone route worker restart is backing off".into())
                    } else {
                        if process.is_none() {
                            last_spawn = Some(now);
                            match spawn_route_process() {
                                Ok(value) => {
                                    process = Some(value);
                                    log::info!("microphone route helper process started");
                                }
                                Err(error) => {
                                    log::warn!("microphone route helper start failed: {error}");
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                            }
                        }
                        let result = process
                            .as_mut()
                            .expect("route process initialized")
                            .request(action, previous.as_deref());
                        if result.is_err() {
                            if let Some(mut failed) = process.take() {
                                let _ = failed.child.kill();
                                let _ = failed.child.wait();
                            }
                            log::warn!(
                                "microphone route helper stopped; will restart on the next request"
                            );
                        }
                        result
                    }
                };
                #[cfg(not(target_os = "windows"))]
                let result = Err("microphone routing is only supported on Windows".into());
                let _ = reply.send(result);
            }
        }
    }
}

fn ensure_cable() {
    let started = Instant::now();
    match run_route(RouteAction::EnsureCable, None) {
        Ok(response) if response.ok => {
            if response.changed {
                let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
                commit_ensure(&mut current, &response);
            }
            log::info!(
                "microphone route action=EnsureCable result=success changed={} elapsed_ms={} target_present={}",
                response.changed,
                started.elapsed().as_millis(),
                non_empty(response.target_id.clone()).is_some()
            );
        }
        Ok(response) => log::warn!(
            "microphone route action=EnsureCable result=failed changed={} skipped={} elapsed_ms={} message={}",
            response.changed,
            response.skipped,
            started.elapsed().as_millis(),
            response.message
        ),
        Err(error) => log::warn!(
            "microphone route action=EnsureCable result=error elapsed_ms={} error={error}",
            started.elapsed().as_millis()
        ),
    }
}

fn restore_owned() {
    let previous = state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .previous
        .clone();
    let Some(previous) = previous else {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.owned_cable = false;
        return;
    };

    let started = Instant::now();
    match run_route(RouteAction::Restore, Some(&previous)) {
        Ok(response) if response.ok => {
            let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
            commit_restore(&mut current);
            remove_legacy_previous();
            log::info!(
                "microphone route action=Restore result={} skipped={} elapsed_ms={} current_present={}",
                if response.skipped { "skipped" } else { "success" },
                response.skipped,
                started.elapsed().as_millis(),
                non_empty(response.current_id.clone()).is_some()
            );
        }
        Ok(response) => log::warn!(
            "microphone route action=Restore result=failed skipped={} elapsed_ms={} message={}",
            response.skipped,
            started.elapsed().as_millis(),
            response.message
        ),
        Err(error) => log::warn!(
            "microphone route action=Restore result=error elapsed_ms={} error={error}",
            started.elapsed().as_millis()
        ),
    }
}

#[cfg(target_os = "windows")]
fn route_script_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets").join("xiaomi").join(ROUTE_SCRIPT_NAME));
            candidates.push(
                dir.join("resources")
                    .join("assets")
                    .join("xiaomi")
                    .join(ROUTE_SCRIPT_NAME),
            );
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(
            std::path::PathBuf::from(manifest)
                .join("assets")
                .join("xiaomi")
                .join(ROUTE_SCRIPT_NAME),
        );
    }
    candidates
}

#[cfg(target_os = "windows")]
fn find_route_script() -> Option<std::path::PathBuf> {
    route_script_candidates()
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
struct RouteProcess {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

#[cfg(target_os = "windows")]
impl RouteProcess {
    fn request(
        &mut self,
        action: RouteAction,
        previous: Option<&str>,
    ) -> Result<RouteResponse, String> {
        let request = serde_json::json!({
            "action": action.as_str(),
            "previous_id": previous.unwrap_or_default(),
        });
        writeln!(self.stdin, "{request}")
            .map_err(|error| format!("write microphone route request failed: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("flush microphone route request failed: {error}"))?;

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|error| format!("read microphone route response failed: {error}"))?;
        if line.trim().is_empty() {
            return Err("microphone route helper returned an empty response".into());
        }
        serde_json::from_str(line.trim())
            .map_err(|error| format!("microphone route helper returned invalid JSON: {error}"))
    }
}

#[cfg(target_os = "windows")]
fn spawn_route_process() -> Result<RouteProcess, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = find_route_script()
        .ok_or_else(|| format!("microphone route helper is missing: {ROUTE_SCRIPT_NAME}"))?;
    let mut child = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
            "-Server",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("start microphone route helper failed: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "microphone route helper stdin is unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "microphone route helper stdout is unavailable".to_string())?;
    Ok(RouteProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

fn run_route(action: RouteAction, previous: Option<&str>) -> Result<RouteResponse, String> {
    request_route(action, previous)
}

#[cfg(target_os = "windows")]
fn legacy_previous() -> Option<String> {
    let root = std::env::var_os("LOCALAPPDATA")?;
    let path = std::path::PathBuf::from(root)
        .join("2655AI/BridgeAudio/XiaomiRemoteBridge/previous-default-microphone.txt");
    let value = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(not(target_os = "windows"))]
fn legacy_previous() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn remove_legacy_previous() {
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        let path = std::path::PathBuf::from(root)
            .join("2655AI/BridgeAudio/XiaomiRemoteBridge/previous-default-microphone.txt");
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(not(target_os = "windows"))]
fn remove_legacy_previous() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_safe_and_idempotent() {
        let mut value = State::default();
        value.mode = Some(Mode::Off);
        assert!(!value.pressed);
        assert!(!value.owned_cable);
        assert!(value.previous.is_none());
    }

    #[test]
    fn duplicate_edges_do_not_change_press_state() {
        let mut value = State::default();
        value.pressed = true;
        value.pressed = true;
        assert!(value.pressed);
    }

    #[test]
    fn route_actions_keep_stable_helper_names() {
        assert_eq!(RouteAction::EnsureCable.as_str(), "EnsureCable");
        assert_eq!(RouteAction::Restore.as_str(), "Restore");
    }

    #[test]
    fn release_only_restores_owned_cable_in_off_mode() {
        let mut state = State {
            mode: Some(Mode::Off),
            pressed: true,
            owned_cable: true,
            previous: Some("previous".into()),
        };
        assert!(end_press(&mut state));
        assert!(!state.pressed);

        state.mode = Some(Mode::AlwaysOn);
        state.pressed = true;
        assert!(!end_press(&mut state));
    }

    #[test]
    fn duplicate_press_is_ignored_and_restore_commit_clears_ownership() {
        let mut state = State::default();
        assert!(begin_press(&mut state));
        assert!(!begin_press(&mut state));
        state.owned_cable = true;
        state.previous = Some("previous".into());
        commit_restore(&mut state);
        assert!(!state.owned_cable);
        assert!(state.previous.is_none());
    }

    #[test]
    fn always_on_press_does_not_route_again() {
        let state = State {
            mode: Some(Mode::AlwaysOn),
            ..State::default()
        };
        assert!(!press_requires_route(&state));
    }

    #[test]
    fn off_mode_press_keeps_temporary_route_behavior() {
        let state = State {
            mode: Some(Mode::Off),
            ..State::default()
        };
        assert!(press_requires_route(&state));
    }
}
