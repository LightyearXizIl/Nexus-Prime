//! Native voice cues, owned by the main application.
//! No log watching, microphone capture, helper executable, or voice-PCM mixing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

const SOURCE_RATE: f64 = 48_000.0;
const CUE_ATTACK_SECONDS: f64 = 0.160;
const CUE_QUEUE_CAPACITY: usize = 6;
/// Playback gain for start/end guitar cues (1.0 = original embedded level).
const FEEDBACK_GAIN: f32 = 0.5;
static COMMANDS: OnceLock<mpsc::Sender<(Command, Instant)>> = OnceLock::new();
static OUTPUT_READY: AtomicBool = AtomicBool::new(false);
static OUTPUT_NEEDED: AtomicBool = AtomicBool::new(false);
static TOTAL_RENDERED: AtomicU64 = AtomicU64::new(0);
static COMPLETED_START: AtomicU64 = AtomicU64::new(0);
static COMPLETED_END: AtomicU64 = AtomicU64::new(0);
static START_BEGUN: AtomicU64 = AtomicU64::new(0);
static END_BEGUN: AtomicU64 = AtomicU64::new(0);
static START_LATENCY_US: AtomicU64 = AtomicU64::new(u64::MAX);
static END_LATENCY_US: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(target_os = "windows")]
static TARGET: parking_lot::Mutex<Option<Target>> = parking_lot::const_mutex(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cue {
    Start,
    End,
}

enum Command {
    Start,
    End,
    Cancel,
    Configure(Option<CuePair>),
    Preview(Cue, Clip),
}

#[derive(Default)]
struct SessionState {
    active: bool,
}

impl SessionState {
    fn accept(&mut self, command: Command) -> Option<Cue> {
        match command {
            Command::Start => {
                if self.active {
                    return None;
                }
                self.active = true;
                Some(Cue::Start)
            }
            Command::End if self.active => {
                self.active = false;
                Some(Cue::End)
            }
            Command::Cancel => {
                self.active = false;
                None
            }
            _ => None,
        }
    }
}

/// Starts one application-owned audio worker. All device I/O stays off BLE and
/// keyboard callbacks; a failure must never prevent recording or key delivery.
pub fn initialize() {
    COMMANDS.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("native-voice-feedback".into())
            .spawn(move || worker(receiver))
        {
            log::error!("VOICE FEEDBACK worker_start_failed error={error}");
        }
        sender
    });
}

fn send(command: Command) {
    if let Some(sender) = COMMANDS.get() {
        let _ = sender.send((command, Instant::now()));
    }
}

pub fn voice_started() {
    send(Command::Start);
}
pub fn voice_stopped() {
    send(Command::End);
}
pub fn cancel() {
    send(Command::Cancel);
}

pub(crate) type Clip = Arc<Vec<[f32; 2]>>;
pub(crate) type CuePair = [Clip; 2];

pub(crate) fn configure(clips: Option<CuePair>) {
    if clips.is_some() {
        initialize();
    }
    send(Command::Configure(clips));
}

pub(crate) fn preview_clip(cue: Cue, clip: Clip) {
    initialize();
    send(Command::Preview(cue, clip));
}

pub(super) fn decode_wav(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("invalid embedded WAV header".into());
    }
    let mut cursor = 12;
    let mut valid_format = false;
    while cursor + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let data_start = cursor + 8;
        let data_end = data_start.checked_add(size).ok_or("invalid WAV size")?;
        if data_end > bytes.len() {
            return Err("truncated embedded WAV".into());
        }
        if &bytes[cursor..cursor + 4] == b"fmt " && size >= 16 {
            let format = &bytes[data_start..data_end];
            valid_format = u16::from_le_bytes(format[0..2].try_into().unwrap()) == 1
                && u16::from_le_bytes(format[2..4].try_into().unwrap()) == 2
                && u32::from_le_bytes(format[4..8].try_into().unwrap()) == SOURCE_RATE as u32
                && u16::from_le_bytes(format[14..16].try_into().unwrap()) == 16;
        }
        if &bytes[cursor..cursor + 4] == b"data" {
            if !valid_format || size == 0 || size % 4 != 0 {
                return Err("embedded WAV must be stereo 48kHz PCM16".into());
            }
            let frames = bytes[data_start..data_end]
                .chunks_exact(4)
                .map(|frame| {
                    [
                        i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.0,
                        i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.0,
                    ]
                })
                .collect();
            return Ok(Arc::new(frames));
        }
        cursor = data_end + size % 2;
    }
    Err("embedded WAV data missing".into())
}

struct Playing {
    id: u64,
    cue: Cue,
    queued_at: Instant,
    began: bool,
    clip: Clip,
    position: f64,
    step: f64,
}

impl Playing {
    fn attack_has_played(&self) -> bool {
        self.position >= (SOURCE_RATE * CUE_ATTACK_SECONDS).min(self.clip.len() as f64)
    }

    fn sample(&mut self) -> Option<[f32; 2]> {
        let index = self.position as usize;
        if index >= self.clip.len() {
            return None;
        }
        let next = (index + 1).min(self.clip.len() - 1);
        let fraction = (self.position - index as f64) as f32;
        let result = [
            self.clip[index][0] * (1.0 - fraction) + self.clip[next][0] * fraction,
            self.clip[index][1] * (1.0 - fraction) + self.clip[next][1] * fraction,
        ];
        self.position += self.step;
        Some(result)
    }
}

#[derive(Default)]
struct RenderState {
    current: Option<Playing>,
    tail: Option<(Playing, usize, usize)>,
    last_callback: Option<Instant>,
    completed_id: u64,
}

impl RenderState {
    fn replace(&mut self, next: Playing, output_rate: f64) -> u64 {
        // Release replaces an unfinished start immediately with an 8ms
        // crossfade. It never waits for the previous chord or a fixed timer.
        let fade = (output_rate * 0.008).round().max(1.0) as usize;
        self.tail = self.current.take().map(|old| (old, fade, fade));
        self.current = Some(next);
        self.completed_id
    }

    fn next(&mut self) -> [f32; 2] {
        let new_gain = self
            .tail
            .as_ref()
            .map(|(_, remaining, total)| 1.0 - *remaining as f32 / *total as f32)
            .unwrap_or(1.0);
        let mut result = [0.0; 2];
        if let Some(playing) = self.current.as_mut() {
            if let Some(frame) = playing.sample() {
                result = [
                    frame[0] * new_gain * FEEDBACK_GAIN,
                    frame[1] * new_gain * FEEDBACK_GAIN,
                ];
                if !playing.began && result.iter().any(|x| x.abs() > 0.00001) {
                    playing.began = true;
                    let latency = playing
                        .queued_at
                        .elapsed()
                        .as_micros()
                        .min(u64::MAX as u128) as u64;
                    let (count, metric) = match playing.cue {
                        Cue::Start => (&START_BEGUN, &START_LATENCY_US),
                        Cue::End => (&END_BEGUN, &END_LATENCY_US),
                    };
                    metric.store(latency, Ordering::Release);
                    count.fetch_add(1, Ordering::Release);
                }
            } else {
                self.completed_id = playing.id;
                self.current = None;
            }
        }
        if let Some((old, remaining, total)) = self.tail.as_mut() {
            if *remaining > 0 {
                let gain = *remaining as f32 / *total as f32;
                if let Some(frame) = old.sample() {
                    result[0] += frame[0] * gain * FEEDBACK_GAIN;
                    result[1] += frame[1] * gain * FEEDBACK_GAIN;
                }
                *remaining -= 1;
            }
            if *remaining == 0 {
                self.tail = None;
            }
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn fxsound_output_name_from_xml(text: &str) -> Option<String> {
    use windows::{core::HSTRING, Data::Xml::Dom::XmlDocument};
    if text.len() > 128 * 1024 {
        return None;
    }
    let document = XmlDocument::new().ok()?;
    document.LoadXml(&HSTRING::from(text)).ok()?;
    let name = document
        .SelectSingleNode(&HSTRING::from(
            "/PROPERTIES/VALUE[@name='output_device_name']/@val",
        ))
        .ok()?
        .InnerText()
        .ok()?
        .to_string();
    (!name.is_empty() && safe_output_name(&name) && !name.contains("FxSound")).then_some(name)
}

#[cfg(target_os = "windows")]
fn fxsound_output_name() -> Option<String> {
    let path = std::path::PathBuf::from(std::env::var_os("APPDATA")?)
        .join("FxSound")
        .join("FxSound.settings");
    if std::fs::metadata(&path).ok()?.len() > 128 * 1024 {
        return None;
    }
    fxsound_output_name_from_xml(&std::fs::read_to_string(path).ok()?)
}

fn safe_output_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    !name.contains("cable input")
        && !name.contains("cable in 16ch")
        && !name.contains("vb-audio virtual cable")
}

#[cfg(target_os = "windows")]
struct Output {
    _stream: cpal::Stream,
    name: String,
    rate: f64,
    state: Arc<parking_lot::Mutex<RenderState>>,
    failed: Arc<AtomicBool>,
    rendered: Arc<AtomicU64>,
}

#[cfg(target_os = "windows")]
impl Output {
    fn unhealthy(&self) -> bool {
        self.failed.load(Ordering::Acquire)
            || self
                .state
                .lock()
                .last_callback
                .is_some_and(|at| at.elapsed() > Duration::from_millis(500))
    }
}

#[cfg(target_os = "windows")]
fn active_render_devices() -> Result<Vec<cpal::Device>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    // CPAL 0.15 output_devices() probes all supported formats, including
    // IsFormatSupported. Doing that every two seconds repeatedly enters the
    // Windows system-effects property store and leaks Key handles on this OS.
    // Enumerate endpoint metadata instead; negotiate only when opening a stream.
    let names = super::endpoint_inventory::active_names(false)?;
    Ok(cpal::default_host()
        .devices()
        .map_err(|e| e.to_string())?
        .filter(|device| device.name().ok().is_some_and(|name| names.contains(&name)))
        .collect())
}

#[cfg(target_os = "windows")]
fn select_device() -> Result<cpal::Device, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(device) = host.default_output_device() {
        // FxSound forwards through another application's audio session, which
        // may be ducked independently of our own. Follow its selected physical
        // output for notifications only; never edit its preset or Windows routes.
        if device
            .name()
            .ok()
            .is_some_and(|name| name.contains("FxSound"))
        {
            if let Some(preferred) = fxsound_output_name() {
                if let Some(physical) = active_render_devices().ok().and_then(|devices| {
                    let mut devices = devices.into_iter();
                    devices.find(|d| d.name().ok().as_ref() == Some(&preferred))
                }) {
                    return Ok(physical);
                }
            }
        }
        if device
            .name()
            .ok()
            .is_some_and(|name| safe_output_name(&name))
        {
            return Ok(device);
        }
    }
    // Never send notification audio into the virtual microphone. If the
    // default is unsuitable, choose an available ordinary playback endpoint.
    active_render_devices()?
        .into_iter()
        .find(|d| d.name().ok().is_some_and(|name| safe_output_name(&name)))
        .ok_or_else(|| "no safe playback endpoint; voice input remains unaffected".into())
}

#[cfg(target_os = "windows")]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<parking_lot::Mutex<RenderState>>,
    failed: Arc<AtomicBool>,
    rendered: Arc<AtomicU64>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut state = state.lock();
                state.last_callback = Some(Instant::now());
                let mut count = 0;
                for frame in data.chunks_mut(channels) {
                    let stereo = state.next();
                    if stereo[0].abs() > 0.00001 || stereo[1].abs() > 0.00001 {
                        count += 1;
                    }
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        // Stereo goes to front L/R only, including 8-channel FxSound.
                        let value = if channels == 1 {
                            (stereo[0] + stereo[1]) * 0.5
                        } else if channel < 2 {
                            stereo[channel]
                        } else {
                            0.0
                        };
                        *sample = T::from_sample(value);
                    }
                }
                rendered.fetch_add(count, Ordering::Relaxed);
                TOTAL_RENDERED.fetch_add(count, Ordering::Relaxed);
            },
            move |error| {
                failed.store(true, Ordering::Release);
                log::warn!("VOICE FEEDBACK stream_error error={error}");
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn keep_own_notification_volume() -> windows::core::Result<u32> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    // Opt out for this process's notification session only. Never change
    // Windows volume, communication preferences, FxSound or another app.
    let mut modified = 0;
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for device_index in 0..devices.GetCount()? {
            let device = devices.Item(device_index)?;
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let sessions = manager.GetSessionEnumerator()?;
            for index in 0..sessions.GetCount()? {
                let session: IAudioSessionControl2 = sessions.GetSession(index)?.cast()?;
                if session.GetProcessId()? == std::process::id() {
                    session.SetDuckingPreference(true)?;
                    modified += 1;
                }
            }
        }
    }
    Ok(modified)
}

#[cfg(target_os = "windows")]
fn open_output() -> Result<Output, String> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    let device = select_device()?;
    let name = device.name().map_err(|e| e.to_string())?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let state = Arc::new(parking_lot::Mutex::new(RenderState::default()));
    let failed = Arc::new(AtomicBool::new(false));
    let rendered = Arc::new(AtomicU64::new(0));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            state.clone(),
            failed.clone(),
            rendered.clone(),
        ),
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            state.clone(),
            failed.clone(),
            rendered.clone(),
        ),
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            state.clone(),
            failed.clone(),
            rendered.clone(),
        ),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    match keep_own_notification_volume() {
        Ok(count) => log::info!("VOICE FEEDBACK own_session_ducking_opt_out sessions={count}"),
        Err(error) => {
            log::debug!("VOICE FEEDBACK own_session_ducking_preference_unavailable error={error}")
        }
    }
    log::info!(
        "VOICE FEEDBACK output_ready device={name} rate={} channels={} embedded=true",
        config.sample_rate.0,
        config.channels
    );
    Ok(Output {
        _stream: stream,
        name,
        rate: config.sample_rate.0 as f64,
        state,
        failed,
        rendered,
    })
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct Target {
    generation: u64,
    name: Arc<str>,
    rate: f64,
    state: Arc<parking_lot::Mutex<RenderState>>,
    failed: Arc<AtomicBool>,
    rendered: Arc<AtomicU64>,
}

#[cfg(target_os = "windows")]
struct Observation {
    cue: Cue,
    clip: Clip,
    id: u64,
    before: u64,
    at: Instant,
    handed_off_at: Instant,
    began: bool,
    retried: bool,
    live: bool,
    target: Target,
}

#[cfg(target_os = "windows")]
impl Observation {
    fn protects_attack(&self) -> bool {
        self.live
            && !self.target.failed.load(Ordering::Acquire)
            && self.handed_off_at.elapsed() < Duration::from_millis(240)
            && self
                .target
                .state
                .lock()
                .current
                .as_ref()
                .is_some_and(|playing| playing.id == self.id && !playing.attack_has_played())
    }
}

#[cfg(target_os = "windows")]
struct QueuedCue {
    cue: Cue,
    clip: Clip,
    at: Instant,
    live: bool,
    retried: bool,
}

#[cfg(target_os = "windows")]
fn record_completion(observed: &Observation) {
    let frames = observed
        .target
        .rendered
        .load(Ordering::Relaxed)
        .saturating_sub(observed.before);
    log::info!("VOICE FEEDBACK cue={:?} id={} state=callback_completed nonzero_frames={frames} elapsed_ms={} device={}",
        observed.cue, observed.id, observed.at.elapsed().as_millis(), observed.target.name);
    match observed.cue {
        Cue::Start => &COMPLETED_START,
        Cue::End => &COMPLETED_END,
    }
    .fetch_add(1, Ordering::Release);
}

/// Device enumeration and stream recovery must never occupy the cue dispatcher.
#[cfg(target_os = "windows")]
fn maintain_output() {
    use cpal::traits::DeviceTrait;
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        );
    }
    let mut output: Option<Output> = None;
    let mut generation = 0;
    let mut next_check = Instant::now();
    loop {
        let now = Instant::now();
        if !OUTPUT_NEEDED.load(Ordering::Acquire) {
            if output.is_some() {
                TARGET.lock().take();
                OUTPUT_READY.store(false, Ordering::Release);
                output = None;
            }
            next_check = now;
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        if now >= next_check || output.as_ref().is_some_and(Output::unhealthy) {
            next_check = now + Duration::from_secs(2);
            let selected = select_device()
                .and_then(|d| d.name().map_err(|e| e.to_string()))
                .ok();
            if output
                .as_ref()
                .is_none_or(|o| o.unhealthy() || Some(&o.name) != selected.as_ref())
            {
                TARGET.lock().take();
                OUTPUT_READY.store(false, Ordering::Release);
                if let Some(previous) = &output {
                    previous.failed.store(true, Ordering::Release);
                }
                output = None;
                match open_output() {
                    Ok(opened) => {
                        generation += 1;
                        *TARGET.lock() = Some(Target {
                            generation,
                            name: Arc::from(opened.name.as_str()),
                            rate: opened.rate,
                            state: opened.state.clone(),
                            failed: opened.failed.clone(),
                            rendered: opened.rendered.clone(),
                        });
                        output = Some(opened);
                        OUTPUT_READY.store(true, Ordering::Release);
                    }
                    Err(error) => log::warn!(
                        "VOICE FEEDBACK output_unavailable retry_in_ms=2000 error={error}"
                    ),
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "windows")]
fn worker(receiver: mpsc::Receiver<(Command, Instant)>) {
    if let Err(error) = std::thread::Builder::new()
        .name("native-feedback-output".into())
        .spawn(maintain_output)
    {
        log::error!("VOICE FEEDBACK output_worker_failed error={error}");
        return;
    }
    let mut session = SessionState::default();
    let mut configured: Option<CuePair> = None;
    let mut session_clips: Option<CuePair> = None;
    let mut observation: Option<Observation> = None;
    let mut pending: VecDeque<QueuedCue> = VecDeque::new();
    let mut serial = 0;
    loop {
        let command = match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let mut next = None;
        let mut clear = false;
        if let Some((command, at)) = command {
            match command {
                Command::Configure(clips) => {
                    configured = clips;
                    if configured.is_none() {
                        session_clips = None;
                        clear = true;
                    }
                }
                Command::Preview(cue, clip) => {
                    if !session.active {
                        pending.clear();
                        next = Some((cue, clip, false));
                    }
                }
                Command::Cancel => {
                    session.accept(Command::Cancel);
                    session_clips = None;
                    clear = true;
                }
                edge => match session.accept(edge) {
                    Some(Cue::Start) => {
                        session_clips = configured.clone();
                        if let Some(clips) = &session_clips {
                            next = Some((Cue::Start, clips[0].clone(), true));
                        } else {
                            clear = true;
                        }
                    }
                    Some(Cue::End) => {
                        if let Some(clips) = session_clips.take() {
                            next = Some((Cue::End, clips[1].clone(), true));
                        }
                    }
                    None => {}
                },
            }
            if let Some((cue, clip, live)) = next {
                if pending.len() >= CUE_QUEUE_CAPACITY {
                    pending.pop_front();
                    log::debug!("VOICE FEEDBACK oldest_pending_cue_discarded");
                }
                pending.push_back(QueuedCue {
                    cue,
                    clip,
                    at,
                    live,
                    retried: false,
                });
            }
        }
        if clear {
            if let Some(target) = TARGET.lock().as_ref() {
                let mut state = target.state.lock();
                state.current = None;
                state.tail = None;
            }
            pending.clear();
            observation = None;
        }
        if let Some(observed) = observation.as_mut() {
            let frames = observed
                .target
                .rendered
                .load(Ordering::Relaxed)
                .saturating_sub(observed.before);
            if frames > 0 && !observed.began {
                observed.began = true;
                let latency = match observed.cue {
                    Cue::Start => &START_LATENCY_US,
                    Cue::End => &END_LATENCY_US,
                }
                .load(Ordering::Acquire);
                log::info!("VOICE FEEDBACK cue={:?} id={} state=rendered nonzero_frames={frames} event_to_first_frame_us={latency} device={}",
                    observed.cue, observed.id, observed.target.name);
            }
            if observed.target.state.lock().completed_id == observed.id {
                record_completion(observed);
                observation = None;
            } else if pending.is_empty() && observed.target.failed.load(Ordering::Acquire) {
                let current = TARGET.lock().clone();
                if let Some(target) = current {
                    if target.generation != observed.target.generation
                        && !observed.retried
                        && observed.at.elapsed() < Duration::from_secs(3)
                    {
                        pending.push_back(QueuedCue {
                            cue: observed.cue,
                            clip: observed.clip.clone(),
                            at: observed.at,
                            live: observed.live,
                            retried: true,
                        });
                    }
                }
                if observed.at.elapsed() >= Duration::from_secs(3) || observed.retried {
                    log::warn!(
                        "VOICE FEEDBACK cue={:?} id={} state=failed retry_limit=1",
                        observed.cue,
                        observed.id
                    );
                    observation = None;
                }
            } else if observed.handed_off_at.elapsed()
                > Duration::from_secs_f64(observed.clip.len() as f64 / SOURCE_RATE + 1.0)
                && pending.is_empty()
            {
                observed.target.failed.store(true, Ordering::Release);
            }
        }
        let protected = pending.front().is_some_and(|cue| cue.live)
            && observation
                .as_ref()
                .is_some_and(Observation::protects_attack);
        if !protected && !pending.is_empty() {
            // Snapshot an already-open stream. No COM, XML, enumeration, sleeps
            // or file reads exist between receiving an edge and handing it off.
            let target = TARGET.lock().clone();
            if let Some(target) = target.filter(|t| !t.failed.load(Ordering::Acquire)) {
                let Some(QueuedCue {
                    cue,
                    clip,
                    at,
                    live,
                    retried,
                }) = pending.pop_front()
                else {
                    continue;
                };
                serial += 1;
                let before = target.rendered.load(Ordering::Relaxed);
                let completed_id = target.state.lock().replace(
                    Playing {
                        id: serial,
                        cue,
                        queued_at: at,
                        began: false,
                        clip: clip.clone(),
                        position: 0.0,
                        step: SOURCE_RATE / target.rate,
                    },
                    target.rate,
                );
                if let Some(previous) = observation.take() {
                    if previous.target.generation == target.generation
                        && previous.id == completed_id
                    {
                        record_completion(&previous);
                    } else {
                        log::info!(
                            "VOICE FEEDBACK cue={:?} id={} state=interrupted_by_{cue:?}",
                            previous.cue,
                            previous.id
                        );
                    }
                }
                log::info!("VOICE FEEDBACK cue={cue:?} id={serial} state=queued origin={} retry={} event_to_queued_us={} device={}",
                    if live { "remote" } else { "preview" }, u8::from(retried), at.elapsed().as_micros(), target.name);
                observation = Some(Observation {
                    cue,
                    clip,
                    id: serial,
                    before,
                    at,
                    handed_off_at: Instant::now(),
                    began: false,
                    retried,
                    live,
                    target,
                });
            } else if pending
                .front()
                .is_some_and(|cue| cue.at.elapsed() >= Duration::from_secs(2))
            {
                if let Some(expired) = pending.pop_front() {
                    log::warn!(
                        "VOICE FEEDBACK cue={:?} state=skipped reason=no_output",
                        expired.cue
                    );
                }
            }
        }
        OUTPUT_NEEDED.store(
            configured.is_some() || observation.is_some() || !pending.is_empty(),
            Ordering::Release,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn worker(_receiver: mpsc::Receiver<(Command, Instant)>) {}

/// An explicit, finite diagnostic mode of the same executable, not an addon.
pub fn preview() -> bool {
    let Ok(start) = decode_wav(include_bytes!("../../assets/voice-feedback/start.wav")) else {
        return false;
    };
    let Ok(end) = decode_wav(include_bytes!("../../assets/voice-feedback/end.wav")) else {
        return false;
    };
    configure(Some([start, end]));
    let started = Instant::now();
    while !OUTPUT_READY.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !OUTPUT_READY.load(Ordering::Acquire) {
        return false;
    }
    let before_start = START_BEGUN.load(Ordering::Acquire);
    let before_end = END_BEGUN.load(Ordering::Acquire);
    let before_completed = COMPLETED_END.load(Ordering::Acquire);
    voice_started();
    std::thread::sleep(Duration::from_millis(120));
    voice_stopped();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if START_BEGUN.load(Ordering::Acquire) > before_start
            && END_BEGUN.load(Ordering::Acquire) > before_end
            && COMPLETED_END.load(Ordering::Acquire) > before_completed
        {
            std::thread::sleep(Duration::from_millis(350));
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub fn latency_metrics_us() -> (u64, u64) {
    (
        START_LATENCY_US.load(Ordering::Acquire),
        END_LATENCY_US.load(Ordering::Acquire),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_session_edges_are_immediate_and_deduplicated() {
        let mut state = SessionState::default();
        assert_eq!(state.accept(Command::End), None);
        assert_eq!(state.accept(Command::Start), Some(Cue::Start));
        assert_eq!(state.accept(Command::Start), None);
        assert_eq!(state.accept(Command::End), Some(Cue::End));
        assert_eq!(state.accept(Command::End), None);
        state.accept(Command::Start);
        state.accept(Command::Cancel);
        assert_eq!(state.accept(Command::End), None);
        assert_eq!(state.accept(Command::Start), Some(Cue::Start));
    }

    fn playing(id: u64, cue: Cue, value: f32) -> Playing {
        Playing {
            id,
            cue,
            queued_at: Instant::now(),
            began: false,
            clip: Arc::new(vec![[value; 2]; 48000]),
            position: 0.0,
            step: 1.0,
        }
    }

    #[test]
    fn release_crossfades_immediately_instead_of_waiting_for_start() {
        let mut state = RenderState::default();
        state.replace(playing(1, Cue::Start, 0.6), 48000.0);
        for _ in 0..100 {
            state.next();
        }
        state.replace(playing(2, Cue::End, -0.6), 48000.0);
        // The next frame begins the transition; within 8ms the release cue
        // completely replaces a start chord that still has almost 1s left.
        assert!(state.next()[0] > 0.5 * FEEDBACK_GAIN);
        for _ in 0..400 {
            state.next();
        }
        assert!(state.next()[0] < -0.5 * FEEDBACK_GAIN);
        assert!(state.tail.is_none());
        assert_eq!(state.current.as_ref().unwrap().id, 2);
    }

    #[test]
    fn quick_taps_protect_the_whole_short_cue_and_only_the_attack_of_long_cues() {
        let mut long = playing(1, Cue::Start, 0.6);
        long.position = SOURCE_RATE * 0.150;
        assert!(!long.attack_has_played());
        long.position = SOURCE_RATE * 0.160;
        assert!(long.attack_has_played());

        let mut short = playing(2, Cue::End, 0.6);
        short.clip = Arc::new(vec![[0.6; 2]; 4368]);
        short.position = 4367.0;
        assert!(!short.attack_has_played());
        short.position = 4368.0;
        assert!(short.attack_has_played());
    }

    #[test]
    fn virtual_microphone_is_never_a_notification_output() {
        assert!(!safe_output_name("CABLE Input (VB-Audio Virtual Cable)"));
        assert!(!safe_output_name("CABLE In 16ch (VB-Audio Virtual Cable)"));
        assert!(safe_output_name(
            "FxSound Speakers (FxSound Audio Enhancer)"
        ));
        assert!(safe_output_name("音箱 (SV)"));
    }

    #[test]
    fn embedded_audio_is_finite_non_silent_and_renderer_finishes() {
        for bytes in [
            include_bytes!("../../assets/voice-feedback/start.wav").as_slice(),
            include_bytes!("../../assets/voice-feedback/end.wav").as_slice(),
        ] {
            let clip = decode_wav(bytes).unwrap();
            assert!((8000..48000).contains(&clip.len()));
            assert!(clip
                .iter()
                .flatten()
                .all(|x| x.is_finite() && x.abs() <= 0.91));
            assert!(clip.iter().flatten().any(|x| x.abs() > 0.1));
            let count = clip.len();
            let mut state = RenderState {
                current: Some(Playing {
                    id: 1,
                    cue: Cue::Start,
                    queued_at: Instant::now(),
                    began: false,
                    clip,
                    position: 0.0,
                    step: SOURCE_RATE / 44100.0,
                }),
                last_callback: None,
                completed_id: 0,
                tail: None,
            };
            for _ in 0..count + 2 {
                state.next();
            }
            assert!(state.current.is_none());
            assert_eq!(state.completed_id, 1);
            assert_eq!(state.next(), [0.0; 2]);
        }
        assert!(decode_wav(b"invalid").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn fxsound_route_decodes_unicode_and_rejects_virtual_microphones() {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            );
        }
        assert_eq!(
            fxsound_output_name_from_xml(
                r#"<PROPERTIES><VALUE name="output_device_name" val="&#38899;&#31665; (SV)"/></PROPERTIES>"#
            ),
            Some("音箱 (SV)".into())
        );
        assert_eq!(
            fxsound_output_name_from_xml(
                r#"<PROPERTIES><VALUE name="output_device_name" val="CABLE Input (VB-Audio Virtual Cable)"/></PROPERTIES>"#
            ),
            None
        );
        assert_eq!(fxsound_output_name_from_xml("<invalid"), None);
        assert_eq!(fxsound_output_name_from_xml("<PROPERTIES/>"), None);
    }
}
