//! Windows default-recording-device routing for the remote microphone.
//!
//! Device policy changes run in a persistent hidden helper process. Windows'
//! undocumented audio policy COM interface is isolated from the Tauri
//! process, so a device or COM failure cannot take down the application.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROUTE_SCRIPT_NAME: &str = "microphone-route.ps1";
const ROUTE_TIMEOUT: Duration = Duration::from_secs(3);
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);
const WORKER_RESTART_BACKOFF: Duration = Duration::from_secs(1);
const MAX_RESTORE_DELAY_SECONDS: u8 = 60;
const RESPONSE_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    recovery_pending: bool,
}

#[derive(Debug, Clone, Copy)]
struct RestoreSettings {
    enabled: bool,
    seconds: u8,
}

impl Default for RestoreSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            seconds: 5,
        }
    }
}

#[derive(Debug)]
struct RestoreSchedule {
    generation: u64,
    deadline: Option<Instant>,
    settings: RestoreSettings,
    shutdown: bool,
}

struct RestoreScheduler {
    state: Mutex<RestoreSchedule>,
    wake: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteAction {
    Warmup,
    EnsureCable,
    Restore,
}

impl RouteAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "Warmup",
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
    #[serde(default)]
    cache_hit: bool,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static ROUTE_OPERATION: OnceLock<Mutex<()>> = OnceLock::new();
static MONITOR: OnceLock<()> = OnceLock::new();
static ROUTE_WORKER: OnceLock<Mutex<Option<RouteWorkerHandle>>> = OnceLock::new();
static RESTORE_SCHEDULER: OnceLock<Arc<RestoreScheduler>> = OnceLock::new();
static ROUTE_EXITING: AtomicBool = AtomicBool::new(false);

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn route_operation() -> &'static Mutex<()> {
    ROUTE_OPERATION.get_or_init(|| Mutex::new(()))
}

fn lock_running_route_operation<'a>(
    operation: &'a Mutex<()>,
    exiting: &AtomicBool,
) -> Option<std::sync::MutexGuard<'a, ()>> {
    if exiting.load(Ordering::Acquire) {
        return None;
    }
    let guard = operation.lock().unwrap_or_else(|error| error.into_inner());
    if exiting.load(Ordering::Acquire) {
        return None;
    }
    Some(guard)
}

fn route_worker() -> &'static Mutex<Option<RouteWorkerHandle>> {
    ROUTE_WORKER.get_or_init(|| Mutex::new(None))
}

fn restore_scheduler() -> &'static Arc<RestoreScheduler> {
    RESTORE_SCHEDULER.get_or_init(|| {
        let scheduler = Arc::new(RestoreScheduler {
            state: Mutex::new(RestoreSchedule {
                generation: 0,
                deadline: None,
                settings: RestoreSettings::default(),
                shutdown: false,
            }),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&scheduler);
        let _ = std::thread::Builder::new()
            .name("microphone-restore-scheduler".into())
            .spawn(move || restore_scheduler_loop(worker));
        scheduler
    })
}

fn restore_scheduler_loop(scheduler: Arc<RestoreScheduler>) {
    loop {
        let (generation, deadline) = {
            let mut current = scheduler
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                if current.shutdown {
                    return;
                }
                let Some(deadline) = current.deadline else {
                    current = scheduler
                        .wake
                        .wait(current)
                        .unwrap_or_else(|error| error.into_inner());
                    continue;
                };
                let now = Instant::now();
                if now < deadline {
                    let timeout = deadline.saturating_duration_since(now);
                    let (next, _) = scheduler
                        .wake
                        .wait_timeout(current, timeout)
                        .unwrap_or_else(|error| error.into_inner());
                    current = next;
                    continue;
                }
                current.deadline = None;
                break (current.generation, deadline);
            }
        };

        let _operation = route_operation()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let still_current = {
            let current = scheduler
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            current.generation == generation && current.deadline.is_none()
        };
        let should_restore = still_current && {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            current.mode == Some(Mode::Off) && !current.pressed && current.owned_cable
        };
        if should_restore {
            log::info!(
                "microphone route delayed restore due generation={} elapsed_ms={}",
                generation,
                deadline.elapsed().as_millis()
            );
            restore_owned();
        } else if still_current {
            log::debug!(
                "microphone route delayed restore skipped generation={} state_changed=true",
                generation
            );
        }
    }
}

fn normalize_restore_seconds(seconds: u8) -> u8 {
    seconds.min(MAX_RESTORE_DELAY_SECONDS)
}

fn set_restore_settings(enabled: bool, seconds: u8) {
    let scheduler = restore_scheduler();
    let mut current = scheduler
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    current.settings = RestoreSettings {
        enabled,
        seconds: normalize_restore_seconds(seconds),
    };
    current.generation = current.generation.wrapping_add(1);
    current.deadline = None;
    scheduler.wake.notify_all();
}

fn cancel_pending_restore(reason: &str) {
    let scheduler = restore_scheduler();
    let mut current = scheduler
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if current.deadline.is_some() {
        log::info!(
            "microphone route delayed restore cancelled reason={} generation={}",
            reason,
            current.generation
        );
    }
    current.generation = current.generation.wrapping_add(1);
    current.deadline = None;
    scheduler.wake.notify_all();
}

fn schedule_restore() -> bool {
    let scheduler = restore_scheduler();
    let mut current = scheduler
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let settings = current.settings;
    if !settings.enabled || settings.seconds == 0 {
        return false;
    }
    current.generation = current.generation.wrapping_add(1);
    current.deadline = Some(Instant::now() + Duration::from_secs(u64::from(settings.seconds)));
    log::info!(
        "microphone route delayed restore scheduled seconds={} generation={}",
        settings.seconds,
        current.generation
    );
    scheduler.wake.notify_all();
    true
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
    current.recovery_pending = false;
}

fn commit_restore(current: &mut State) {
    current.owned_cable = false;
    current.previous = None;
    current.recovery_pending = false;
}

/// Apply the saved setting. This is idempotent and safe during startup,
/// settings save, and bridge restart.
pub fn apply_settings(always_on: bool, restore_delay_enabled: bool, restore_delay_seconds: u8) {
    let Some(_operation) = lock_running_route_operation(route_operation(), &ROUTE_EXITING) else {
        return;
    };
    start_monitor();
    set_restore_settings(restore_delay_enabled, restore_delay_seconds);
    cancel_pending_restore("settings_changed");

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
        drop(current);
        warmup_async();
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
    let Some(_operation) = lock_running_route_operation(route_operation(), &ROUTE_EXITING) else {
        return;
    };
    let should_route = {
        cancel_pending_restore("voice_pressed");
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
    let Some(_operation) = lock_running_route_operation(route_operation(), &ROUTE_EXITING) else {
        return;
    };
    let should_restore = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        end_press(&mut current)
    };
    if should_restore {
        if !schedule_restore() {
            log::info!("microphone route restore immediate reason=delay_disabled");
            restore_owned();
        }
    }
}

/// Used by disconnect, bridge restart, tray quit, and process exit.
pub fn cleanup(reason: &str) {
    let exiting = matches!(reason, "app_exit" | "tray_quit");
    if exiting {
        ROUTE_EXITING.store(true, Ordering::Release);
    }
    let _operation = route_operation()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cancel_pending_restore(reason);
    let should_restore = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.owned_cable && current.mode != Some(Mode::AlwaysOn)
    };
    if should_restore {
        restore_owned();
    }
    let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
    current.pressed = false;
    if exiting {
        current.mode = Some(Mode::Off);
    }
    // A failed restore keeps both ownership and the durable previous endpoint
    // so the monitor / next startup can retry instead of abandoning CABLE.
    drop(current);
    shutdown_route_worker();
    log::debug!("microphone route cleanup reason={reason}");
}

fn start_monitor() {
    if MONITOR.set(()).is_ok() {
        let _ = std::thread::Builder::new()
            .name("microphone-route-monitor".into())
            .spawn(|| loop {
                std::thread::sleep(MONITOR_INTERVAL);
                let Some(_operation) =
                    lock_running_route_operation(route_operation(), &ROUTE_EXITING)
                else {
                    break;
                };
                let (always_on, recovery_pending) = {
                    let current = state().lock().unwrap_or_else(|error| error.into_inner());
                    (
                        current.mode == Some(Mode::AlwaysOn),
                        current.recovery_pending && current.owned_cable,
                    )
                };
                if always_on {
                    ensure_cable();
                } else if recovery_pending {
                    restore_owned();
                }
            });
    }
}

#[derive(Debug)]
enum WorkerRequest {
    Route {
        action: RouteAction,
        previous: Option<String>,
        reply: Option<SyncSender<Result<RouteResponse, String>>>,
        control: Arc<RequestControl>,
    },
    Shutdown,
}

#[derive(Debug)]
struct RequestControl {
    deadline: Instant,
    cancelled: AtomicBool,
}

impl RequestControl {
    fn new(timeout: Duration) -> Self {
        Self {
            deadline: Instant::now() + timeout,
            cancelled: AtomicBool::new(false),
        }
    }

    fn check(&self, shutdown: &AtomicBool) -> Result<(), String> {
        if shutdown.load(Ordering::Acquire) || self.cancelled.load(Ordering::Acquire) {
            return Err("microphone route request cancelled".into());
        }
        if Instant::now() >= self.deadline {
            return Err("microphone route request deadline expired".into());
        }
        Ok(())
    }

    fn deadline_utc_ms(&self) -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            + self
                .deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
    }
}

#[derive(Clone, Debug)]
struct RouteWorkerHandle {
    requests: mpsc::Sender<WorkerRequest>,
    warmup_requested: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    join: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

fn ensure_route_worker(allow_during_exit: bool) -> Result<RouteWorkerHandle, String> {
    let mut slot = route_worker()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !allow_during_exit && ROUTE_EXITING.load(Ordering::Acquire) {
        return Err("microphone route worker is exiting".into());
    }
    if let Some(handle) = slot.as_ref() {
        return Ok(handle.clone());
    }

    let (requests, receiver) = mpsc::channel();
    let warmup_requested = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_warmup_requested = Arc::clone(&warmup_requested);
    let worker_shutdown = Arc::clone(&shutdown);
    let join = std::thread::Builder::new()
        .name("microphone-route-worker".into())
        .spawn(move || route_worker_loop(receiver, worker_warmup_requested, worker_shutdown))
        .map_err(|error| format!("start microphone route worker failed: {error}"))?;
    let handle = RouteWorkerHandle {
        requests,
        warmup_requested,
        shutdown,
        join: Arc::new(Mutex::new(Some(join))),
    };
    *slot = Some(handle.clone());
    log::info!("microphone route worker started");
    Ok(handle)
}

/// Prepare the isolated helper before a voice press without changing the
/// selected microphone. The worker owns the queued request, so a cancelled
/// warmup can never create a replacement worker after bridge cleanup.
pub fn warmup_async() {
    let Ok(handle) = ensure_route_worker(false) else {
        return;
    };
    if handle.shutdown.load(Ordering::Acquire)
        || handle.warmup_requested.swap(true, Ordering::AcqRel)
    {
        return;
    }
    if handle
        .requests
        .send(WorkerRequest::Route {
            action: RouteAction::Warmup,
            previous: None,
            reply: None,
            control: Arc::new(RequestControl::new(ROUTE_TIMEOUT)),
        })
        .is_err()
    {
        handle.warmup_requested.store(false, Ordering::Release);
    }
}

fn shutdown_route_worker() {
    let handle = route_worker()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(handle) = handle {
        shutdown_worker_handle(handle);
    }
}

fn shutdown_worker_handle(handle: RouteWorkerHandle) {
    handle.shutdown.store(true, Ordering::Release);
    let _ = handle.requests.send(WorkerRequest::Shutdown);
    let join = handle
        .join
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(join) = join {
        if join.join().is_err() {
            log::warn!("microphone route worker panicked during shutdown");
        }
    }
    log::info!("microphone route worker stopped helper_reaped=true");
}

fn action_allowed_during_exit(action: RouteAction) -> bool {
    action == RouteAction::Restore
}

fn request_route(action: RouteAction, previous: Option<&str>) -> Result<RouteResponse, String> {
    let handle = ensure_route_worker(action_allowed_during_exit(action))?;
    let (reply, result) = mpsc::sync_channel(1);
    let control = Arc::new(RequestControl::new(ROUTE_TIMEOUT));
    let send_result = handle.requests.send(WorkerRequest::Route {
        action,
        previous: previous.map(str::to_owned),
        reply: Some(reply),
        control: Arc::clone(&control),
    });
    if let Err(error) = send_result {
        route_worker()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        return Err(format!("send microphone route request failed: {error}"));
    }
    match result.recv_timeout(ROUTE_TIMEOUT + Duration::from_millis(100)) {
        Ok(response) => response,
        Err(error) => {
            control.cancelled.store(true, Ordering::Release);
            // The worker's bounded pipe polling normally acknowledges first.
            // Ownership is reserved before mutation even if scheduling stalls.
            result
                .recv_timeout(Duration::from_millis(250))
                .unwrap_or_else(|_| Err(format!("microphone route worker timed out: {error}")))
        }
    }
}

fn route_worker_loop(
    receiver: Receiver<WorkerRequest>,
    warmup_requested: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    #[cfg(target_os = "windows")]
    let mut process: Option<RouteProcess> = None;
    #[cfg(target_os = "windows")]
    let mut last_spawn: Option<Instant> = None;

    while let Ok(request) = receiver.recv() {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        match request {
            WorkerRequest::Shutdown => {
                break;
            }
            WorkerRequest::Route {
                action,
                previous,
                reply,
                control,
            } => {
                if let Err(error) = control.check(&shutdown) {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(error));
                    }
                    warmup_requested.store(false, Ordering::Release);
                    continue;
                }
                let started = Instant::now();
                #[cfg(target_os = "windows")]
                let cold = process.is_none();
                #[cfg(not(target_os = "windows"))]
                let cold = false;
                #[cfg(target_os = "windows")]
                let result = {
                    let now = Instant::now();
                    if process.is_none()
                        && action != RouteAction::Restore
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
                                    warmup_requested.store(false, Ordering::Release);
                                    if let Some(reply) = reply {
                                        let _ = reply.send(Err(error));
                                    }
                                    continue;
                                }
                            }
                        }
                        if shutdown.load(Ordering::Acquire) {
                            break;
                        }
                        let result = run_worker_request(
                            process.as_mut().expect("route process initialized"),
                            action,
                            previous.as_deref(),
                            &control,
                            &shutdown,
                        );
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
                let result: Result<RouteResponse, String> =
                    Err("microphone routing is only supported on Windows".into());
                if result.as_ref().map(|response| !response.ok).unwrap_or(true) {
                    warmup_requested.store(false, Ordering::Release);
                }
                log::info!(
                    "microphone route helper action={} cold={} elapsed_ms={} ready={} cache_hit={}",
                    action.as_str(),
                    cold,
                    started.elapsed().as_millis(),
                    result.as_ref().map(|response| response.ok).unwrap_or(false),
                    result
                        .as_ref()
                        .map(|response| response.cache_hit)
                        .unwrap_or(false),
                );
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                } else {
                    match result {
                        Ok(response) if !response.ok => {
                            log::warn!("microphone route warmup unavailable: {}", response.message)
                        }
                        Err(error) => log::warn!("microphone route warmup failed: {error}"),
                        _ => {}
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    if let Some(mut process) = process.take() {
        let _ = process.child.kill();
        let _ = process.child.wait();
    }
}

fn ensure_cable() {
    let started = Instant::now();
    match run_route(RouteAction::EnsureCable, None) {
        Ok(response) if response.ok => {
            let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
            commit_ensure(&mut current, &response);
            drop(current);
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
    let needs_recovery = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.mode != Some(Mode::AlwaysOn) && current.recovery_pending
    };
    if needs_recovery {
        log::warn!("microphone route recovering uncertain EnsureCable outcome");
        restore_owned();
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

    state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .recovery_pending = true;

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
    stdout: std::process::ChildStdout,
}

#[cfg(target_os = "windows")]
impl Drop for RouteProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "windows")]
fn reserve_route_ownership(response: &RouteResponse) -> Result<Option<String>, String> {
    let current = non_empty(response.current_id.clone())
        .ok_or_else(|| "microphone route preparation has no current endpoint".to_string())?;
    let target = non_empty(response.target_id.clone())
        .ok_or_else(|| "microphone route preparation has no target endpoint".to_string())?;
    if current == target {
        return Ok(None);
    }
    // Reserve and persist the recovery target BEFORE the helper can mutate
    // Windows policy. A lost/late reply must never leave an unowned CABLE.
    let root = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "cannot persist microphone recovery: LOCALAPPDATA is missing".to_string())?;
    let path = std::path::PathBuf::from(root)
        .join("2655AI/BridgeAudio/XiaomiRemoteBridge/previous-default-microphone.txt");
    std::fs::create_dir_all(path.parent().expect("recovery file has parent"))
        .and_then(|_| std::fs::write(&path, &current))
        .map_err(|error| format!("cannot persist microphone recovery before routing: {error}"))?;
    let mut owned = state().lock().unwrap_or_else(|error| error.into_inner());
    owned.previous = Some(current.clone());
    owned.owned_cable = true;
    owned.recovery_pending = true;
    Ok(Some(current))
}

#[cfg(target_os = "windows")]
fn run_worker_request(
    process: &mut RouteProcess,
    action: RouteAction,
    previous: Option<&str>,
    control: &RequestControl,
    shutdown: &AtomicBool,
) -> Result<RouteResponse, String> {
    run_worker_request_with(
        process,
        action,
        previous,
        control,
        shutdown,
        reserve_route_ownership,
    )
}

#[cfg(target_os = "windows")]
fn run_worker_request_with(
    process: &mut RouteProcess,
    action: RouteAction,
    previous: Option<&str>,
    control: &RequestControl,
    shutdown: &AtomicBool,
    reserve: impl FnOnce(&RouteResponse) -> Result<Option<String>, String>,
) -> Result<RouteResponse, String> {
    control.check(shutdown)?;
    if action == RouteAction::EnsureCable {
        let prepared = process.request(RouteAction::Warmup, None, control, shutdown)?;
        if !prepared.ok {
            return Ok(prepared);
        }
        control.check(shutdown)?;
        let Some(expected_current) = reserve(&prepared)? else {
            return Ok(prepared);
        };
        process.request(action, Some(&expected_current), control, shutdown)
    } else {
        process.request(action, previous, control, shutdown)
    }
}

#[cfg(target_os = "windows")]
impl RouteProcess {
    fn request(
        &mut self,
        action: RouteAction,
        previous: Option<&str>,
        control: &RequestControl,
        shutdown: &AtomicBool,
    ) -> Result<RouteResponse, String> {
        control.check(shutdown)?;
        let request = serde_json::json!({
            "action": action.as_str(),
            "previous_id": previous.unwrap_or_default(),
            "deadline_utc_ms": control.deadline_utc_ms(),
        });
        writeln!(self.stdin, "{request}")
            .map_err(|error| format!("write microphone route request failed: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("flush microphone route request failed: {error}"))?;

        let line = self.read_response(control, shutdown)?;
        if line.trim().is_empty() {
            return Err("microphone route helper returned an empty response".into());
        }
        serde_json::from_str(line.trim())
            .map_err(|error| format!("microphone route helper returned invalid JSON: {error}"))
    }

    fn read_response(
        &mut self,
        control: &RequestControl,
        shutdown: &AtomicBool,
    ) -> Result<String, String> {
        use std::os::windows::io::AsRawHandle;
        // Anonymous stdout pipes are synchronous. Peek before every small read
        // instead of blocking read_line, so timeout and shutdown stay effective.
        #[link(name = "kernel32")]
        extern "system" {
            fn PeekNamedPipe(
                pipe: *mut std::ffi::c_void,
                buffer: *mut std::ffi::c_void,
                size: u32,
                read: *mut u32,
                available: *mut u32,
                remaining: *mut u32,
            ) -> i32;
        }
        let mut line = Vec::new();
        loop {
            control.check(shutdown)?;
            let mut available = 0;
            let peeked = unsafe {
                PeekNamedPipe(
                    self.stdout.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if peeked == 0 {
                return Err(format!(
                    "read microphone route pipe failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if available == 0 {
                if self
                    .child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_some()
                {
                    return Err("microphone route helper exited before its response".into());
                }
                std::thread::sleep(
                    RESPONSE_POLL_INTERVAL
                        .min(control.deadline.saturating_duration_since(Instant::now())),
                );
                continue;
            }
            let mut bytes = vec![0; (available as usize).min(4096)];
            let count = self
                .stdout
                .read(&mut bytes)
                .map_err(|error| format!("read microphone route response failed: {error}"))?;
            if count == 0 {
                return Err("microphone route helper closed its response pipe".into());
            }
            line.extend_from_slice(&bytes[..count]);
            if line.len() > 65536 {
                return Err("microphone route response exceeded its size limit".into());
            }
            if let Some(end) = line.iter().position(|byte| *byte == b'\n') {
                return String::from_utf8(line[..end].to_vec())
                    .map_err(|error| format!("microphone route response is not UTF-8: {error}"));
            }
        }
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
        stdout,
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
        assert_eq!(RouteAction::Warmup.as_str(), "Warmup");
        assert_eq!(RouteAction::EnsureCable.as_str(), "EnsureCable");
        assert_eq!(RouteAction::Restore.as_str(), "Restore");
        assert!(!action_allowed_during_exit(RouteAction::Warmup));
        assert!(!action_allowed_during_exit(RouteAction::EnsureCable));
        assert!(action_allowed_during_exit(RouteAction::Restore));
    }

    #[test]
    fn release_only_restores_owned_cable_in_off_mode() {
        let mut state = State {
            mode: Some(Mode::Off),
            pressed: true,
            owned_cable: true,
            previous: Some("previous".into()),
            recovery_pending: false,
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

    #[test]
    fn restore_delay_defaults_to_five_seconds_and_clamps_to_sixty() {
        let settings = RestoreSettings::default();
        assert!(settings.enabled);
        assert_eq!(settings.seconds, 5);
        assert_eq!(normalize_restore_seconds(0), 0);
        assert_eq!(normalize_restore_seconds(60), 60);
        assert_eq!(normalize_restore_seconds(255), 60);
    }

    #[test]
    fn cancelled_worker_discards_queued_warmup_without_starting_a_helper() {
        let (requests, receiver) = mpsc::channel();
        let (reply, result) = mpsc::sync_channel(1);
        requests
            .send(WorkerRequest::Route {
                action: RouteAction::Warmup,
                previous: None,
                reply: Some(reply),
                control: Arc::new(RequestControl::new(ROUTE_TIMEOUT)),
            })
            .unwrap();
        route_worker_loop(
            receiver,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
        );
        assert!(matches!(
            result.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn helper_warmup_is_read_only_and_invalid_endpoint_cache_is_refreshed() {
        use std::os::windows::process::CommandExt;

        // Load only the helper's function definitions and replace every Windows
        // endpoint operation with an in-process fake. No real device is touched.
        let harness = r###"
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
public static class NexusMicrophoneRoute {
  public static string Current = "original";
  public static string Cable = "cable-1";
  public static int Changes = 0;
  public static string GetDefaultCapture() { return Current; }
  public static bool CaptureEndpointExists(string id) { return id == Cable || id == "original"; }
  public static void SetDefaultCapture(string id) { Current = id; Changes++; }
}
'@
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:NEXUS_MIC_ROUTE_TEST_SCRIPT, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "Helper script syntax error: $errors" }
foreach ($function in $ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
  Invoke-Expression $function.Extent.Text
}
$script:CachedVBCableCapture = $null
$script:VBCableCacheHit = $false
$script:DiscoveryCount = 0
function Get-VBCableCapture {
  $script:DiscoveryCount++
  if ([string]::IsNullOrEmpty([NexusMicrophoneRoute]::Cable)) { return $null }
  return [pscustomobject]@{ Id = [NexusMicrophoneRoute]::Cable; Name = 'Test CABLE Output' }
}
function Assert-RouteTest([bool] $Condition, [string] $Message) {
  if (-not $Condition) { throw $Message }
}
$first = Invoke-Route 'Warmup' '' | ConvertFrom-Json
Assert-RouteTest ($first.ok -and -not $first.changed -and -not $first.cache_hit) 'First warmup failed'
Assert-RouteTest ([NexusMicrophoneRoute]::Current -eq 'original' -and [NexusMicrophoneRoute]::Changes -eq 0) 'Warmup changed default microphone'
$second = Invoke-Route 'Warmup' '' | ConvertFrom-Json
Assert-RouteTest ($second.ok -and $second.cache_hit -and $script:DiscoveryCount -eq 1) 'Valid cache was not reused'
[NexusMicrophoneRoute]::Cable = 'cable-2'
$replacement = Invoke-Route 'Warmup' '' | ConvertFrom-Json
Assert-RouteTest ($replacement.ok -and -not $replacement.cache_hit -and $replacement.target_id -eq 'cable-2' -and $script:DiscoveryCount -eq 2) 'Inactive cache was not refreshed'
Assert-RouteTest ([NexusMicrophoneRoute]::Changes -eq 0) 'Cache refresh changed default microphone'
$expired = Invoke-Route 'EnsureCable' 'original' 1 | ConvertFrom-Json
Assert-RouteTest (-not $expired.ok -and [NexusMicrophoneRoute]::Changes -eq 0) 'Expired request changed default microphone'
$changedAfterPrepare = Invoke-Route 'EnsureCable' 'different-expected-device' | ConvertFrom-Json
Assert-RouteTest (-not $changedAfterPrepare.ok -and $changedAfterPrepare.skipped -and [NexusMicrophoneRoute]::Changes -eq 0) 'Stale prepared endpoint changed default microphone'
$pressed = Invoke-Route 'EnsureCable' '' | ConvertFrom-Json
Assert-RouteTest ($pressed.changed -and $pressed.previous_id -eq 'original' -and [NexusMicrophoneRoute]::Current -eq 'cable-2') 'Press did not retain temporary route semantics'
[NexusMicrophoneRoute]::Current = 'user-selected'
$restored = Invoke-Route 'Restore' 'original' | ConvertFrom-Json
Assert-RouteTest ($restored.skipped -and [NexusMicrophoneRoute]::Changes -eq 1 -and [NexusMicrophoneRoute]::Current -eq 'user-selected') 'User microphone selection was overwritten'
[NexusMicrophoneRoute]::Cable = ''
$missing = Invoke-Route 'Warmup' '' | ConvertFrom-Json
Assert-RouteTest (-not $missing.ok -and [NexusMicrophoneRoute]::Changes -eq 1) 'Missing endpoint was cached as ready'
'PASS'
"###;
        let output = std::process::Command::new("powershell.exe")
            .creation_flags(0x0800_0000)
            .args(["-NoProfile", "-NonInteractive", "-Command", harness])
            .env(
                "NEXUS_MIC_ROUTE_TEST_SCRIPT",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets/xiaomi/microphone-route.ps1"),
            )
            .output()
            .expect("PowerShell test harness must start");
        assert!(
            output.status.success(),
            "mocked route helper failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("PASS"));
    }

    #[cfg(target_os = "windows")]
    fn fake_route_process(body: &str) -> RouteProcess {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let script = format!("[Console]::Out.WriteLine('READY'); [Console]::Out.Flush(); {body}");
        let mut child = Command::new("powershell.exe")
            .creation_flags(0x0800_0000)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut process = RouteProcess {
            child,
            stdin,
            stdout,
        };
        assert_eq!(
            process
                .read_response(
                    &RequestControl::new(Duration::from_secs(5)),
                    &AtomicBool::new(false)
                )
                .unwrap()
                .trim(),
            "READY"
        );
        process
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn stalled_warmup_times_out_and_helper_can_be_reaped() {
        let mut process =
            fake_route_process("$null = [Console]::In.ReadLine(); Start-Sleep -Seconds 30");
        let started = Instant::now();
        let result = run_worker_request_with(
            &mut process,
            RouteAction::Warmup,
            None,
            &RequestControl::new(Duration::from_millis(80)),
            &AtomicBool::new(false),
            |_| panic!("Warmup must not reserve ownership"),
        );
        assert!(result.unwrap_err().contains("deadline expired"));
        let _ = process.child.kill();
        assert!(process.child.wait().is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn shutdown_interrupts_active_helper_response_wait() {
        let mut process =
            fake_route_process("$null = [Console]::In.ReadLine(); Start-Sleep -Seconds 30");
        let shutdown = Arc::new(AtomicBool::new(false));
        let request_shutdown = Arc::clone(&shutdown);
        let (done, completed) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = process.request(
                RouteAction::Warmup,
                None,
                &RequestControl::new(Duration::from_secs(30)),
                &request_shutdown,
            );
            drop(process);
            done.send(result).unwrap();
        });
        std::thread::sleep(Duration::from_millis(50));
        shutdown.store(true, Ordering::Release);
        assert!(completed
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap_err()
            .contains("cancelled"));
        worker.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn expired_ensure_is_not_written_to_helper() {
        let mut process = fake_route_process(
            r#"
$count = 0
while ($null -ne ($line = [Console]::In.ReadLine())) {
  $count++
  [Console]::Out.WriteLine(('{{"ok":true,"message":"{0}"}}' -f $count))
  [Console]::Out.Flush()
}
"#,
        );
        let expired = RequestControl::new(Duration::ZERO);
        let result = run_worker_request_with(
            &mut process,
            RouteAction::EnsureCable,
            None,
            &expired,
            &AtomicBool::new(false),
            |_| panic!("Expired EnsureCable must not reserve or execute"),
        );
        assert!(result.unwrap_err().contains("deadline expired"));
        let next = process
            .request(
                RouteAction::Warmup,
                None,
                &RequestControl::new(ROUTE_TIMEOUT),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(next.message, "1");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn lost_ensure_reply_keeps_prepared_recovery_target() {
        let mut process = fake_route_process(
            r#"
$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine('{"ok":true,"current_id":"original","target_id":"cable","message":"prepared"}')
[Console]::Out.Flush()
$null = [Console]::In.ReadLine()
Start-Sleep -Seconds 30
"#,
        );
        let mut recovery_target = None;
        let result = run_worker_request_with(
            &mut process,
            RouteAction::EnsureCable,
            None,
            &RequestControl::new(Duration::from_millis(150)),
            &AtomicBool::new(false),
            |snapshot| {
                recovery_target = snapshot.current_id.clone();
                Ok(recovery_target.clone())
            },
        );
        assert!(result.unwrap_err().contains("deadline expired"));
        assert_eq!(recovery_target.as_deref(), Some("original"));
    }

    #[test]
    fn exit_prevents_operation_that_was_waiting_for_the_route_lock() {
        let operation = Arc::new(Mutex::new(()));
        let exiting = Arc::new(AtomicBool::new(false));
        let guard = operation.lock().unwrap();
        let queued_operation = Arc::clone(&operation);
        let queued_exiting = Arc::clone(&exiting);
        let (ready, started) = mpsc::sync_channel(1);
        let (done, completed) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let allowed =
                lock_running_route_operation(&queued_operation, &queued_exiting).is_some();
            done.send(allowed).unwrap();
        });
        started.recv().unwrap();
        assert!(matches!(
            completed.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        exiting.store(true, Ordering::Release);
        drop(guard);
        assert!(!completed.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn shutdown_waits_for_active_helper_to_be_reaped() {
        let mut process =
            fake_route_process("$null = [Console]::In.ReadLine(); Start-Sleep -Seconds 30");
        let (requests, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let reaped = Arc::new(AtomicBool::new(false));
        let worker_reaped = Arc::clone(&reaped);
        let (ready, started) = mpsc::sync_channel(1);
        let join = std::thread::spawn(move || {
            let _receiver = receiver;
            ready.send(()).unwrap();
            let result = process.request(
                RouteAction::Warmup,
                None,
                &RequestControl::new(Duration::from_secs(30)),
                &worker_shutdown,
            );
            assert!(result.unwrap_err().contains("cancelled"));
            drop(process);
            worker_reaped.store(true, Ordering::Release);
        });
        started.recv().unwrap();
        shutdown_worker_handle(RouteWorkerHandle {
            requests,
            warmup_requested: Arc::new(AtomicBool::new(true)),
            shutdown,
            join: Arc::new(Mutex::new(Some(join))),
        });
        assert!(reaped.load(Ordering::Acquire));
    }
}
