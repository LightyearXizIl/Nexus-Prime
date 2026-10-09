//! Windows default-recording-device routing for the remote microphone.
//!
//! Device policy changes run in a short-lived hidden helper process. Windows'
//! undocumented audio policy COM interface is isolated from the Tauri
//! process, so a device or COM failure cannot take down the application.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROUTE_SCRIPT_NAME: &str = "microphone-route.ps1";
const ROUTE_TIMEOUT: Duration = Duration::from_secs(3);
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);

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

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn route_operation() -> &'static Mutex<()> {
    ROUTE_OPERATION.get_or_init(|| Mutex::new(()))
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
    {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if !begin_press(&mut current) {
            return;
        }
    }
    ensure_cable();
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
    if current.mode != Some(Mode::AlwaysOn) {
        current.owned_cable = false;
        current.previous = None;
    }
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
fn run_route(action: RouteAction, previous: Option<&str>) -> Result<RouteResponse, String> {
    use std::process::{Command, Stdio};

    let script = find_route_script()
        .ok_or_else(|| format!("microphone route helper is missing: {ROUTE_SCRIPT_NAME}"))?;
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
            "-Action",
            action.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(previous) = previous {
        command.args(["-PreviousId", previous]);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("start microphone route helper failed: {error}"))?;
    let deadline = Instant::now() + ROUTE_TIMEOUT;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("poll microphone route helper failed: {error}"))?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "microphone route helper timed out after {}ms",
                ROUTE_TIMEOUT.as_millis()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("read microphone route helper failed: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let response: RouteResponse = serde_json::from_str(&stdout).map_err(|error| {
        format!(
            "microphone route helper returned invalid JSON: {error}; exit={:?}; stderr={stderr}; stdout={stdout}",
            output.status.code()
        )
    })?;
    if !output.status.success() && response.ok {
        return Err(format!(
            "microphone route helper failed with exit {:?}",
            output.status.code()
        ));
    }
    Ok(response)
}

#[cfg(not(target_os = "windows"))]
fn run_route(_: RouteAction, _: Option<&str>) -> Result<RouteResponse, String> {
    Err("microphone routing is only supported on Windows".into())
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
}
