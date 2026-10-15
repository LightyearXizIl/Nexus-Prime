//! 对齐 Python `audio_router.py` PCM 侧：独立进程监听 UDP，写入 VB-CABLE
//!
//! 协议：PING→PONG / CLEAR / END / 原始 int16 LE PCM（48k mono）
//! Windows：子进程挂到 Job Object（KILL_ON_JOB_CLOSE），父进程崩溃时子进程一并退出。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEFAULT_PCM_PORT: u16 = 31680;

/// The audio callbacks only publish timestamps and error flags. The UDP thread
/// formats all diagnostics, so logging never runs on the audio output thread.
struct PlaybackDiagnostics {
    epoch: Instant,
    session: AtomicU64,
    started_us: AtomicU64,
    idle_us: AtomicU64,
    first_udp_us: AtomicU64,
    first_nonzero_udp_us: AtomicU64,
    first_output_us: AtomicU64,
    first_nonzero_output_us: AtomicU64,
    peak_abs_i16: AtomicU32,
    last_output_us: AtomicU64,
    last_callback_us: AtomicU64,
    stream_errors: AtomicU64,
    stream_error_kind: AtomicU8,
}

impl PlaybackDiagnostics {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
            session: AtomicU64::new(0),
            started_us: AtomicU64::new(1),
            idle_us: AtomicU64::new(0),
            first_udp_us: AtomicU64::new(0),
            first_nonzero_udp_us: AtomicU64::new(0),
            first_output_us: AtomicU64::new(0),
            first_nonzero_output_us: AtomicU64::new(0),
            peak_abs_i16: AtomicU32::new(0),
            last_output_us: AtomicU64::new(0),
            last_callback_us: AtomicU64::new(0),
            stream_errors: AtomicU64::new(0),
            stream_error_kind: AtomicU8::new(0),
        }
    }

    fn now_us(&self) -> u64 {
        // Zero means "not observed" in the atomic timestamp fields.
        (self.epoch.elapsed().as_micros() as u64).saturating_add(1)
    }

    // Called while holding the existing PCM queue lock, serializing CLEAR with
    // a callback that is still consuming the previous session's audio.
    fn begin_session_at(&self, now_us: u64) {
        let last_output = self.last_output_us.load(Ordering::Acquire);
        self.session.fetch_add(1, Ordering::Relaxed);
        self.started_us.store(now_us, Ordering::Relaxed);
        self.idle_us.store(
            if last_output == 0 {
                0
            } else {
                now_us.saturating_sub(last_output).saturating_add(1)
            },
            Ordering::Relaxed,
        );
        self.first_udp_us.store(0, Ordering::Release);
        self.first_nonzero_udp_us.store(0, Ordering::Release);
        self.first_output_us.store(0, Ordering::Release);
        self.first_nonzero_output_us.store(0, Ordering::Release);
        self.peak_abs_i16.store(0, Ordering::Release);
    }

    fn record_udp_at(&self, now_us: u64, contains_nonzero: bool) {
        let _ = self
            .first_udp_us
            .compare_exchange(0, now_us, Ordering::Release, Ordering::Relaxed);
        if contains_nonzero {
            let _ = self.first_nonzero_udp_us.compare_exchange(
                0,
                now_us,
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }

    fn record_callback_at(&self, now_us: u64, consumed_pcm: bool, peak_abs_i16: u32) {
        self.last_callback_us.store(now_us, Ordering::Release);
        if consumed_pcm {
            let _ = self.first_output_us.compare_exchange(
                0,
                now_us,
                Ordering::Release,
                Ordering::Relaxed,
            );
            self.last_output_us.store(now_us, Ordering::Release);
            if peak_abs_i16 != 0 {
                self.peak_abs_i16.fetch_max(peak_abs_i16, Ordering::Relaxed);
                let _ = self.first_nonzero_output_us.compare_exchange(
                    0,
                    now_us,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            }
        }
    }

    fn record_stream_error(&self, error: &cpal::StreamError) {
        let kind = match error {
            cpal::StreamError::DeviceNotAvailable => 1,
            cpal::StreamError::BackendSpecific { .. } => 2,
        };
        self.stream_error_kind.store(kind, Ordering::Relaxed);
        self.stream_errors.fetch_add(1, Ordering::Release);
    }

    fn snapshot(&self) -> PlaybackSnapshot {
        let first_udp = self.first_udp_us.load(Ordering::Acquire);
        let first_output = self.first_output_us.load(Ordering::Acquire);
        let first_nonzero_udp = self.first_nonzero_udp_us.load(Ordering::Acquire);
        let first_nonzero_output = self.first_nonzero_output_us.load(Ordering::Acquire);
        let started = self.started_us.load(Ordering::Relaxed);
        let idle = self.idle_us.load(Ordering::Relaxed);
        PlaybackSnapshot {
            session: self.session.load(Ordering::Relaxed),
            idle_ms: (idle != 0).then(|| (idle - 1) / 1_000),
            first_udp_ms: (first_udp != 0).then(|| first_udp.saturating_sub(started) / 1_000),
            first_output_ms: (first_output != 0)
                .then(|| first_output.saturating_sub(started) / 1_000),
            first_nonzero_udp_ms: (first_nonzero_udp != 0)
                .then(|| first_nonzero_udp.saturating_sub(started) / 1_000),
            first_nonzero_output_ms: (first_nonzero_output != 0)
                .then(|| first_nonzero_output.saturating_sub(started) / 1_000),
            nonzero_udp_to_output_us: (first_nonzero_udp != 0 && first_nonzero_output != 0)
                .then(|| first_nonzero_output.saturating_sub(first_nonzero_udp)),
            peak_abs_i16: self.peak_abs_i16.load(Ordering::Relaxed),
            udp_to_output_us: (first_udp != 0 && first_output != 0)
                .then(|| first_output.saturating_sub(first_udp)),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PlaybackSnapshot {
    session: u64,
    idle_ms: Option<u64>,
    first_udp_ms: Option<u64>,
    first_output_ms: Option<u64>,
    first_nonzero_udp_ms: Option<u64>,
    first_nonzero_output_ms: Option<u64>,
    nonzero_udp_to_output_us: Option<u64>,
    peak_abs_i16: u32,
    udp_to_output_us: Option<u64>,
}

#[derive(Default)]
struct PlaybackReporter {
    udp_reported: bool,
    output_reported: bool,
    nonzero_udp_reported: bool,
    nonzero_output_reported: bool,
    errors_reported: u64,
}

impl PlaybackReporter {
    fn new_session(&mut self) {
        self.udp_reported = false;
        self.output_reported = false;
        self.nonzero_udp_reported = false;
        self.nonzero_output_reported = false;
    }

    fn report(&mut self, diagnostics: &PlaybackDiagnostics, ending: bool) {
        let snapshot = diagnostics.snapshot();
        if snapshot.first_udp_ms.is_some() && !self.udp_reported {
            log::info!(
                "AUDIO ROUTER LATENCY session={} phase=first_udp idle_ms={:?} first_udp_ms={:?}",
                snapshot.session,
                snapshot.idle_ms,
                snapshot.first_udp_ms
            );
            self.udp_reported = true;
        }
        if snapshot.first_output_ms.is_some() && !self.output_reported {
            log::info!(
                "AUDIO ROUTER LATENCY session={} phase=first_pcm_output idle_ms={:?} first_output_ms={:?} udp_to_output_us={:?} evidence=output_callback ime_capture=unverified",
                snapshot.session, snapshot.idle_ms, snapshot.first_output_ms, snapshot.udp_to_output_us
            );
            self.output_reported = true;
        }
        if snapshot.first_nonzero_udp_ms.is_some() && !self.nonzero_udp_reported {
            log::info!(
                "AUDIO ROUTER LATENCY session={} phase=first_nonzero_udp first_nonzero_udp_ms={:?} evidence=nonzero_samples speech=unverified ime_capture=unverified",
                snapshot.session, snapshot.first_nonzero_udp_ms
            );
            self.nonzero_udp_reported = true;
        }
        if snapshot.first_nonzero_output_ms.is_some() && !self.nonzero_output_reported {
            log::info!(
                "AUDIO ROUTER LATENCY session={} phase=first_nonzero_output first_nonzero_output_ms={:?} nonzero_udp_to_output_us={:?} peak_abs_i16={} evidence=nonzero_samples speech=unverified ime_capture=unverified",
                snapshot.session, snapshot.first_nonzero_output_ms,
                snapshot.nonzero_udp_to_output_us, snapshot.peak_abs_i16
            );
            self.nonzero_output_reported = true;
        }
        if ending {
            log::info!(
                "AUDIO ROUTER LATENCY session={} phase=end idle_ms={:?} udp_received={} pcm_output={} udp_to_output_us={:?} nonzero_udp={} nonzero_output={} first_nonzero_udp_ms={:?} first_nonzero_output_ms={:?} peak_abs_i16={} evidence=nonzero_samples speech=unverified ime_capture=unverified",
                snapshot.session, snapshot.idle_ms, snapshot.first_udp_ms.is_some(),
                snapshot.first_output_ms.is_some(), snapshot.udp_to_output_us,
                snapshot.first_nonzero_udp_ms.is_some(), snapshot.first_nonzero_output_ms.is_some(),
                snapshot.first_nonzero_udp_ms, snapshot.first_nonzero_output_ms, snapshot.peak_abs_i16
            );
        }
        let errors = diagnostics.stream_errors.load(Ordering::Acquire);
        if errors != self.errors_reported {
            let kind = match diagnostics.stream_error_kind.load(Ordering::Relaxed) {
                1 => "device_not_available",
                2 => "backend_error",
                _ => "unknown",
            };
            let callback = diagnostics.last_callback_us.load(Ordering::Acquire);
            let callback_age_ms =
                (callback != 0).then(|| diagnostics.now_us().saturating_sub(callback) / 1_000);
            log::error!(
                "AUDIO ROUTER stream_error count={errors} kind={kind} last_callback_age_ms={callback_age_ms:?}"
            );
            self.errors_reported = errors;
        }
    }
}

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn find_cable(host: &cpal::Host) -> Result<cpal::Device, String> {
    let devices = host
        .output_devices()
        .map_err(|e| format!("枚举输出失败: {e}"))?;
    for d in devices {
        let Ok(name) = d.name() else { continue };
        if name.to_ascii_lowercase().contains("cable input") {
            return Ok(d);
        }
    }
    Err("未找到 CABLE Input (VB-Audio Virtual Cable)".into())
}

/// CLI：`xiaomi-audio-router [--pcm-port N]`
pub fn run_audio_router_cli(args: &[String]) -> i32 {
    crate::logging::init_from_env();
    let mut port = pcm_port();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--pcm-port" {
            if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                port = v;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    match run_router(port) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("xiaomi-audio-router failed: {e}");
            20
        }
    }
}

fn run_router(port: u16) -> Result<(), String> {
    let host = cpal::default_host();
    let device = find_cable(&host)?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("输出配置: {e}"))?;
    let sample_format = supported.sample_format();
    let stream_config = low_latency_stream_config(&supported);
    log::info!(
        "AUDIO ROUTER device={} port={} format={:?} buffer={:?} sample_rate_hz={} channels={}",
        device.name().unwrap_or_default(),
        port,
        sample_format,
        stream_config.buffer_size,
        stream_config.sample_rate.0,
        stream_config.channels
    );

    let buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));
    let running = Arc::new(AtomicBool::new(true));
    let buffer_cb = Arc::clone(&buffer);
    let running_cb = Arc::clone(&running);
    let diagnostics = Arc::new(PlaybackDiagnostics::new());
    let mut reporter = PlaybackReporter::default();
    let channels = stream_config.channels as usize;

    let stream = match sample_format {
        cpal::SampleFormat::I16 => build_i16_stream(
            &device,
            &stream_config,
            &supported,
            buffer_cb,
            running_cb,
            channels,
            Arc::clone(&diagnostics),
        )?,
        cpal::SampleFormat::F32 => build_f32_stream(
            &device,
            &stream_config,
            &supported,
            buffer_cb,
            running_cb,
            channels,
            Arc::clone(&diagnostics),
        )?,
        other => return Err(format!("unsupported format {other:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;

    let sock = UdpSocket::bind(format!("127.0.0.1:{port}"))
        .map_err(|e| format!("bind pcm {port}: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    log::info!("AUDIO ROUTER READY pcm=127.0.0.1:{port} stream_started=true pong_evidence=udp_listener ime_capture=unverified");

    // 最多缓存约 40ms@48k，优先最新语音，避免堆积导致听写「越说越慢」。
    const MAX_BUFFER_SAMPLES: usize = 1_920;

    let mut buf = [0u8; 65536];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, peer)) => {
                let data = &buf[..n];
                if data == b"PING" {
                    let _ = sock.send_to(b"PONG", peer);
                } else if data == b"CLEAR" {
                    let mut queue = buffer.lock();
                    queue.clear();
                    diagnostics.begin_session_at(diagnostics.now_us());
                    drop(queue);
                    reporter.new_session();
                    let snapshot = diagnostics.snapshot();
                    log::info!(
                        "AUDIO ROUTER LATENCY session={} phase=begin idle_ms={:?}",
                        snapshot.session,
                        snapshot.idle_ms
                    );
                } else if data == b"END" {
                    buffer.lock().clear();
                    reporter.report(&diagnostics, true);
                } else if data == b"STOP" || data.is_empty() {
                    // ignore
                } else if n % 2 == 0 {
                    let received_us = diagnostics.now_us();
                    let mut samples = Vec::with_capacity(n / 2);
                    let mut contains_nonzero = false;
                    for chunk in data.chunks_exact(2) {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                        contains_nonzero |= sample != 0;
                        samples.push(sample);
                    }
                    let mut b = buffer.lock();
                    diagnostics.record_udp_at(received_us, contains_nonzero);
                    b.extend(samples);
                    while b.len() > MAX_BUFFER_SAMPLES {
                        b.pop_front();
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                log::error!("AUDIO ROUTER recv: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        reporter.report(&diagnostics, false);
        // 兜底：无 Job 时仍靠 parent-pid 自检
        if let Ok(pid_s) = std::env::var("REMOTE_BRIDGE_PARENT_PID") {
            if let Ok(pid) = pid_s.parse::<u32>() {
                if !parent_alive(pid) {
                    break;
                }
            }
        }
    }
    reporter.report(&diagnostics, true);
    running.store(false, Ordering::Release);
    Ok(())
}

/// 对齐 Python `latency="low"`：首选约 5ms 固定缓冲，失败则回退默认。
fn low_latency_stream_config(supported: &cpal::SupportedStreamConfig) -> cpal::StreamConfig {
    use cpal::{BufferSize, SupportedBufferSize};
    let mut cfg = supported.config();
    let rate = cfg.sample_rate.0.max(1);
    let target = ((rate as f32) * 0.005).round() as u32; // ~5ms
    let frames = match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } => target.clamp(*min, *max).max(*min),
        SupportedBufferSize::Unknown => target.clamp(256, 1024),
    };
    cfg.buffer_size = BufferSize::Fixed(frames);
    cfg
}

fn sample_peak_abs_i16(sample: i16) -> u32 {
    // unsigned_abs preserves the full magnitude of i16::MIN (32768).
    u32::from(sample.unsigned_abs())
}

fn build_i16_stream(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    supported: &cpal::SupportedStreamConfig,
    buffer_cb: Arc<Mutex<VecDeque<i16>>>,
    running_cb: Arc<AtomicBool>,
    channels: usize,
    diagnostics: Arc<PlaybackDiagnostics>,
) -> Result<cpal::Stream, String> {
    let make = |cfg: &cpal::StreamConfig,
                buffer_cb: Arc<Mutex<VecDeque<i16>>>,
                running_cb: Arc<AtomicBool>| {
        let output_diagnostics = Arc::clone(&diagnostics);
        let error_diagnostics = Arc::clone(&diagnostics);
        device.build_output_stream(
            cfg,
            move |data: &mut [i16], _| {
                if !running_cb.load(Ordering::Acquire) {
                    data.fill(0);
                    return;
                }
                let mut buf = buffer_cb.lock();
                let consumed_pcm = !buf.is_empty() && !data.is_empty();
                let mut peak_abs_i16 = 0;
                for frame in data.chunks_mut(channels.max(1)) {
                    let s = buf.pop_front().unwrap_or(0);
                    peak_abs_i16 = peak_abs_i16.max(sample_peak_abs_i16(s));
                    for o in frame.iter_mut() {
                        *o = s;
                    }
                }
                output_diagnostics.record_callback_at(
                    output_diagnostics.now_us(),
                    consumed_pcm,
                    peak_abs_i16,
                );
            },
            move |e| error_diagnostics.record_stream_error(&e),
            None,
        )
    };
    match make(preferred, Arc::clone(&buffer_cb), Arc::clone(&running_cb)) {
        Ok(s) => Ok(s),
        Err(e) => {
            log::warn!("AUDIO ROUTER low-latency buffer rejected ({e}); fallback default");
            let mut fallback = supported.config();
            fallback.buffer_size = cpal::BufferSize::Default;
            make(&fallback, buffer_cb, running_cb).map_err(|e| e.to_string())
        }
    }
}

fn build_f32_stream(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    supported: &cpal::SupportedStreamConfig,
    buffer_cb: Arc<Mutex<VecDeque<i16>>>,
    running_cb: Arc<AtomicBool>,
    channels: usize,
    diagnostics: Arc<PlaybackDiagnostics>,
) -> Result<cpal::Stream, String> {
    let make = |cfg: &cpal::StreamConfig,
                buffer_cb: Arc<Mutex<VecDeque<i16>>>,
                running_cb: Arc<AtomicBool>| {
        let output_diagnostics = Arc::clone(&diagnostics);
        let error_diagnostics = Arc::clone(&diagnostics);
        device.build_output_stream(
            cfg,
            move |data: &mut [f32], _| {
                if !running_cb.load(Ordering::Acquire) {
                    data.fill(0.0);
                    return;
                }
                let mut buf = buffer_cb.lock();
                let consumed_pcm = !buf.is_empty() && !data.is_empty();
                let mut peak_abs_i16 = 0;
                for frame in data.chunks_mut(channels.max(1)) {
                    let s = buf.pop_front().unwrap_or(0);
                    peak_abs_i16 = peak_abs_i16.max(sample_peak_abs_i16(s));
                    let f = s as f32 / 32768.0;
                    for o in frame.iter_mut() {
                        *o = f;
                    }
                }
                output_diagnostics.record_callback_at(
                    output_diagnostics.now_us(),
                    consumed_pcm,
                    peak_abs_i16,
                );
            },
            move |e| error_diagnostics.record_stream_error(&e),
            None,
        )
    };
    match make(preferred, Arc::clone(&buffer_cb), Arc::clone(&running_cb)) {
        Ok(s) => Ok(s),
        Err(e) => {
            log::warn!("AUDIO ROUTER low-latency buffer rejected ({e}); fallback default");
            let mut fallback = supported.config();
            fallback.buffer_size = cpal::BufferSize::Default;
            make(&fallback, buffer_cb, running_cb).map_err(|e| e.to_string())
        }
    }
}

#[cfg(target_os = "windows")]
fn parent_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn parent_alive(_pid: u32) -> bool {
    true
}

struct AudioChild {
    child: std::process::Child,
    #[cfg(target_os = "windows")]
    job: Option<windows::Win32::Foundation::HANDLE>,
}

// Job HANDLE 仅由本模块串行持有
unsafe impl Send for AudioChild {}

impl Drop for AudioChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(target_os = "windows")]
        {
            if let Some(job) = self.job.take() {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(job);
                }
            }
        }
    }
}

static AUDIO_CHILD: Mutex<Option<AudioChild>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn create_kill_on_close_job() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    unsafe {
        let job =
            CreateJobObjectW(None, PCWSTR::null()).map_err(|e| format!("CreateJobObjectW: {e}"))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(e) = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of_val(&info) as u32,
        ) {
            let _ = CloseHandle(job);
            return Err(format!("SetInformationJobObject: {e}"));
        }
        Ok(job)
    }
}

/// 主进程：拉起 audio router 子进程（对齐 Python XiaomiWorkers audio 角色）
pub fn spawn_audio_router_process() -> Result<(), String> {
    stop_audio_router_process();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let port = pcm_port();
    let parent = std::process::id().to_string();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("xiaomi-audio-router")
        .arg("--pcm-port")
        .arg(port.to_string())
        .env("REMOTE_BRIDGE_PCM_PORT", port.to_string())
        .env("REMOTE_BRIDGE_PARENT_PID", &parent)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Ok(log_path) = std::env::var("REMOTE_BRIDGE_LOG_PATH") {
        cmd.env("REMOTE_BRIDGE_LOG_PATH", log_path);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // 隐藏控制台：无窗口 + 分离，避免启动时闪黑框
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("spawn audio router: {e}"))?;

    #[cfg(target_os = "windows")]
    let job = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;

        match create_kill_on_close_job() {
            Ok(job) => {
                let proc = HANDLE(child.as_raw_handle());
                if let Err(e) = unsafe { AssignProcessToJobObject(job, proc) } {
                    log::warn!("AssignProcessToJobObject failed: {e}; fallback parent-pid watch");
                    let _ = unsafe { CloseHandle(job) };
                    None
                } else {
                    log::info!("AUDIO ROUTER assigned to JobObject (KILL_ON_JOB_CLOSE)");
                    Some(job)
                }
            }
            Err(e) => {
                log::warn!("CreateJobObject failed: {e}; fallback parent-pid watch");
                None
            }
        }
    };

    log::info!(
        "XIAOMI AUDIO ROUTER spawned pid={} pcm_port={}",
        child.id(),
        port
    );
    *AUDIO_CHILD.lock() = Some(AudioChild {
        child,
        #[cfg(target_os = "windows")]
        job,
    });
    Ok(())
}

pub fn stop_audio_router_process() {
    // Drop 会 kill + CloseHandle(job)
    let _ = AUDIO_CHILD.lock().take();
}

/// 语音路由子进程 PID（冲突扫描时排除，避免把自己当成占用方）
pub fn audio_router_child_pid() -> Option<u32> {
    let mut guard = AUDIO_CHILD.lock();
    match guard.as_mut() {
        Some(ac) => match ac.child.try_wait() {
            Ok(None) => Some(ac.child.id()),
            Ok(Some(_)) => {
                *guard = None;
                None
            }
            Err(_) => None,
        },
        None => None,
    }
}

/// 进程是否仍在跑（poll）
pub fn audio_router_process_alive() -> bool {
    let mut guard = AUDIO_CHILD.lock();
    match guard.as_mut() {
        Some(ac) => match ac.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                *guard = None;
                false
            }
            Err(_) => false,
        },
        None => false,
    }
}

/// UDP PING/PONG：语音路由就绪（对齐 Python audio status）
pub fn audio_router_ready() -> bool {
    let port = pcm_port();
    let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
    if sock.send_to(b"PING", ("127.0.0.1", port)).is_err() {
        return false;
    }
    let mut buf = [0u8; 32];
    match sock.recv_from(&mut buf) {
        Ok((n, _)) => &buf[..n] == b"PONG",
        Err(_) => false,
    }
}

/// 带缓存的就绪探测：每次调用都新建 socket + PING 往返，前端状态页每秒问一次。
/// 冲突检测/重启桥接等需要新鲜结果的路径请用无缓存的 `audio_router_ready()`。
pub fn audio_router_ready_cached() -> bool {
    // ponytail: TTL 5s；router 挂掉后状态页最多延迟 5s 变红
    static CACHE: std::sync::Mutex<Option<(Instant, bool)>> = std::sync::Mutex::new(None);
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, ok)) = *g {
        if at.elapsed() < Duration::from_secs(5) {
            return ok;
        }
    }
    let ok = audio_router_ready();
    *g = Some((Instant::now(), ok));
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_callbacks_do_not_claim_pcm_was_output() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(1_000);
        diagnostics.record_callback_at(2_000, false, 0);
        diagnostics.record_udp_at(3_000, false);
        diagnostics.record_callback_at(4_000, false, 0);

        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.first_udp_ms, Some(2));
        assert_eq!(snapshot.first_output_ms, None);
        assert_eq!(snapshot.udp_to_output_us, None);
    }

    #[test]
    fn first_pcm_output_latency_is_not_overwritten_by_later_packets() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(10_000);
        diagnostics.record_udp_at(20_000, false);
        diagnostics.record_callback_at(25_000, true, 0);
        diagnostics.record_udp_at(30_000, false);
        diagnostics.record_callback_at(35_000, true, 0);

        assert_eq!(
            diagnostics.snapshot(),
            PlaybackSnapshot {
                session: 1,
                idle_ms: None,
                first_udp_ms: Some(10),
                first_output_ms: Some(15),
                first_nonzero_udp_ms: None,
                first_nonzero_output_ms: None,
                nonzero_udp_to_output_us: None,
                peak_abs_i16: 0,
                udp_to_output_us: Some(5_000),
            }
        );
    }

    #[test]
    fn new_session_resets_first_frame_and_keeps_actual_output_idle_time() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(10_000);
        diagnostics.record_udp_at(20_000, true);
        diagnostics.record_callback_at(25_000, true, 120);
        // Continuous silence output is not fresh remote microphone activity.
        diagnostics.record_callback_at(3_000_000, false, 0);
        diagnostics.begin_session_at(5_025_000);

        assert_eq!(
            diagnostics.snapshot(),
            PlaybackSnapshot {
                session: 2,
                idle_ms: Some(5_000),
                first_udp_ms: None,
                first_output_ms: None,
                first_nonzero_udp_ms: None,
                first_nonzero_output_ms: None,
                nonzero_udp_to_output_us: None,
                peak_abs_i16: 0,
                udp_to_output_us: None,
            }
        );
        diagnostics.record_udp_at(5_026_000, false);
        diagnostics.record_callback_at(5_028_000, true, 0);
        assert_eq!(diagnostics.snapshot().udp_to_output_us, Some(2_000));
    }

    #[test]
    fn silence_pcm_is_output_without_claiming_nonzero_audio() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(1_000);
        diagnostics.record_udp_at(2_000, false);
        diagnostics.record_callback_at(3_000, true, 0);
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.first_output_ms, Some(2));
        assert_eq!(snapshot.first_nonzero_udp_ms, None);
        assert_eq!(snapshot.first_nonzero_output_ms, None);
        assert_eq!(snapshot.nonzero_udp_to_output_us, None);
        assert_eq!(snapshot.peak_abs_i16, 0);
    }

    #[test]
    fn nonzero_audio_has_separate_first_packet_and_first_output_times() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(1_000);
        diagnostics.record_udp_at(2_000, false);
        diagnostics.record_callback_at(3_000, true, 0);
        diagnostics.record_udp_at(3_002_000, true);
        diagnostics.record_callback_at(3_007_000, true, 120);
        diagnostics.record_udp_at(3_012_000, true);
        diagnostics.record_callback_at(3_017_000, true, 200);
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.first_udp_ms, Some(1));
        assert_eq!(snapshot.first_output_ms, Some(2));
        assert_eq!(snapshot.first_nonzero_udp_ms, Some(3_001));
        assert_eq!(snapshot.first_nonzero_output_ms, Some(3_006));
        assert_eq!(snapshot.nonzero_udp_to_output_us, Some(5_000));
        assert_eq!(snapshot.peak_abs_i16, 200);

        diagnostics.begin_session_at(4_000_000);
        let reset = diagnostics.snapshot();
        assert_eq!(reset.first_nonzero_udp_ms, None);
        assert_eq!(reset.first_nonzero_output_ms, None);
        assert_eq!(reset.peak_abs_i16, 0);
        diagnostics.record_udp_at(4_001_000, true);
        diagnostics.record_callback_at(4_003_000, true, 1);
        assert_eq!(diagnostics.snapshot().first_nonzero_output_ms, Some(3));
        assert_eq!(diagnostics.snapshot().peak_abs_i16, 1);
    }

    #[test]
    fn negative_full_scale_pcm_peak_does_not_overflow() {
        assert_eq!(sample_peak_abs_i16(i16::MIN), 32_768);
        assert_eq!(sample_peak_abs_i16(i16::MAX), 32_767);
        assert_eq!(sample_peak_abs_i16(0), 0);
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.begin_session_at(1_000);
        diagnostics.record_udp_at(2_000, true);
        diagnostics.record_callback_at(3_000, true, sample_peak_abs_i16(i16::MIN));
        diagnostics.record_callback_at(4_000, true, sample_peak_abs_i16(i16::MAX));
        assert_eq!(diagnostics.snapshot().peak_abs_i16, 32_768);
    }

    #[test]
    fn stream_errors_are_available_to_the_non_realtime_reporter() {
        let diagnostics = PlaybackDiagnostics::new();
        diagnostics.record_stream_error(&cpal::StreamError::DeviceNotAvailable);
        assert_eq!(diagnostics.stream_errors.load(Ordering::Acquire), 1);
        assert_eq!(diagnostics.stream_error_kind.load(Ordering::Relaxed), 1);

        let mut reporter = PlaybackReporter::default();
        reporter.report(&diagnostics, false);
        assert_eq!(reporter.errors_reported, 1);
        reporter.new_session();
        assert_eq!(reporter.errors_reported, 1);
    }
}
