//! 对齐 Python `UdpPcmOutput`：16k→48k 后 UDP 送到独立 audio_router 进程

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use std::collections::VecDeque;

use crate::audio::pcm_router::DEFAULT_PCM_PORT;

struct Client {
    sock: UdpSocket,
    peer: SocketAddr,
    prev: i16,
    have_prev: bool,
    source_rate_hz: Option<u32>,
    sent: AtomicU64,
    dropped: AtomicU64,
}

static CLIENT: Mutex<Option<Client>> = Mutex::new(None);
/// 热路径快速判断，避免每帧进 ensure_started / 抢锁探测
static READY: AtomicBool = AtomicBool::new(false);
static STARTING: AtomicBool = AtomicBool::new(false);
static INPUT_GATE_CLOSED: AtomicBool = AtomicBool::new(false);
static SEND_LOCK: Mutex<()> = Mutex::new(());
static FIRST_SEND_AT: Mutex<Option<Instant>> = Mutex::new(None);

const MAX_PRE_ROLL_MS: u64 = 250;

struct PendingFrame {
    samples: Vec<i16>,
    source_rate_hz: u32,
    duration_ms: u64,
}

#[derive(Default)]
struct PendingFrames {
    frames: VecDeque<PendingFrame>,
    duration_ms: u64,
    peak_duration_ms: u64,
    dropped_ms: u64,
}

static PENDING: Mutex<PendingFrames> = Mutex::new(PendingFrames {
    frames: VecDeque::new(),
    duration_ms: 0,
    peak_duration_ms: 0,
    dropped_ms: 0,
});

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn peer_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], pcm_port()))
}

/// 等待 router PONG（对齐 Python 最多 ~4s）
pub fn ensure_started() -> Result<(), String> {
    if READY.load(Ordering::Acquire) {
        return Ok(());
    }
    if STARTING.swap(true, Ordering::AcqRel) {
        return Err("audio router warmup already in progress".into());
    }
    let result = ensure_started_inner();
    STARTING.store(false, Ordering::Release);
    if result.is_ok() {
        flush_pending();
    }
    result
}

fn ensure_started_inner() -> Result<(), String> {
    {
        let g = CLIENT.lock();
        if g.is_some() {
            READY.store(true, Ordering::Release);
            return Ok(());
        }
    }
    let peer = peer_addr();
    let sock = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(150)))
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut ok = false;
    while Instant::now() < deadline {
        let _ = sock.send_to(b"PING", peer);
        let mut buf = [0u8; 64];
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if &buf[..n] == b"PONG" {
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !ok {
        return Err(format!("audio router not ready at {peer}"));
    }
    *CLIENT.lock() = Some(Client {
        sock,
        peer,
        prev: 0,
        have_prev: false,
        source_rate_hz: None,
        sent: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    });
    READY.store(true, Ordering::Release);
    log::info!("XIAOMI VOICE PCM UDP ready peer={peer}");
    Ok(())
}

/// 后台预热：应用启动 / 连上遥控后尽早 PING，避免首句说话才建连
pub fn warmup_async() {
    if READY.load(Ordering::Acquire) || STARTING.load(Ordering::Acquire) {
        return;
    }
    std::thread::Builder::new()
        .name("xiaomi-pcm-warmup".into())
        .spawn(|| {
            for attempt in 1..=8 {
                match ensure_started() {
                    Ok(()) => {
                        log::info!("XIAOMI VOICE PCM warmup ok attempt={attempt}");
                        return;
                    }
                    Err(e) => {
                        log::debug!("XIAOMI VOICE PCM warmup attempt={attempt}: {e}");
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            log::warn!("XIAOMI VOICE PCM warmup gave up; will retry on first push");
        })
        .ok();
}

pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// A new remote voice press begins with the audio gate closed.  Audio is
/// decoded immediately but held briefly until the input-method shortcut has
/// actually been delivered.
pub fn begin_session() {
    INPUT_GATE_CLOSED.store(true, Ordering::Release);
    let mut pending = PENDING.lock();
    pending.frames.clear();
    pending.duration_ms = 0;
    pending.peak_duration_ms = 0;
    pending.dropped_ms = 0;
    drop(pending);
    *FIRST_SEND_AT.lock() = None;
    clear();
}

/// Open the session's input gate after the shortcut path is ready, then flush
/// the earliest complete audio frames in FIFO order.
pub fn release_input_gate() {
    INPUT_GATE_CLOSED.store(false, Ordering::Release);
    flush_pending();
}

pub fn discard_pending() {
    let mut pending = PENDING.lock();
    pending.frames.clear();
    pending.duration_ms = 0;
}

pub fn pre_roll_stats() -> (u64, u64) {
    let pending = PENDING.lock();
    (pending.peak_duration_ms, pending.dropped_ms)
}

pub fn first_send_at() -> Option<Instant> {
    *FIRST_SEND_AT.lock()
}

pub fn clear() {
    if let Some(c) = CLIENT.lock().as_ref() {
        let _ = c.sock.send_to(b"CLEAR", c.peer);
    }
    if let Some(c) = CLIENT.lock().as_mut() {
        c.have_prev = false;
        c.source_rate_hz = None;
    }
}

pub fn end_session() {
    if let Some(c) = CLIENT.lock().as_ref() {
        let _ = c.sock.send_to(b"END", c.peer);
    }
}

/// 将 ATVV 单声道 PCM 重采样到 router 所需的 48kHz。
/// 目前协议支持 8kHz 和 16kHz；切换采样率时丢弃插值历史，避免跨速率产生爆音。
pub fn push_pcm(samples: &[i16], source_rate_hz: u32) {
    if samples.is_empty() {
        return;
    }
    let ratio = match source_rate_hz {
        8_000 => 6,
        16_000 => 3,
        other => {
            log::warn!("XIAOMI VOICE PCM unsupported source_rate={other}Hz; drop frame");
            return;
        }
    };
    if INPUT_GATE_CLOSED.load(Ordering::Acquire) || !READY.load(Ordering::Acquire) {
        enqueue_pending(samples, source_rate_hz);
        if !READY.load(Ordering::Acquire) {
            warmup_async();
        }
        return;
    }
    let _send = SEND_LOCK.lock();
    flush_pending_locked();
    send_pcm_now(samples, source_rate_hz, ratio);
}

fn enqueue_pending(samples: &[i16], source_rate_hz: u32) {
    let duration_ms = ((samples.len() as u64) * 1000 / source_rate_hz.max(1) as u64).max(1);
    let mut pending = PENDING.lock();
    while pending.duration_ms + duration_ms > MAX_PRE_ROLL_MS {
        let Some(old) = pending.frames.pop_front() else { break };
        pending.duration_ms = pending.duration_ms.saturating_sub(old.duration_ms);
        pending.dropped_ms += old.duration_ms;
    }
    pending.duration_ms += duration_ms;
    pending.peak_duration_ms = pending.peak_duration_ms.max(pending.duration_ms);
    pending.frames.push_back(PendingFrame {
        samples: samples.to_vec(),
        source_rate_hz,
        duration_ms,
    });
}

fn flush_pending() {
    if INPUT_GATE_CLOSED.load(Ordering::Acquire) || !READY.load(Ordering::Acquire) {
        return;
    }
    let _send = SEND_LOCK.lock();
    flush_pending_locked();
}

fn flush_pending_locked() {
    if INPUT_GATE_CLOSED.load(Ordering::Acquire) || !READY.load(Ordering::Acquire) {
        return;
    }
    let frames = {
        let mut pending = PENDING.lock();
        pending.duration_ms = 0;
        pending.frames.drain(..).collect::<Vec<_>>()
    };
    for frame in frames {
        let ratio = match frame.source_rate_hz {
            8_000 => 6,
            16_000 => 3,
            _ => continue,
        };
        send_pcm_now(&frame.samples, frame.source_rate_hz, ratio);
    }
}

fn send_pcm_now(samples: &[i16], source_rate_hz: u32, ratio: usize) {
    let mut guard = CLIENT.lock();
    let Some(c) = guard.as_mut() else {
        READY.store(false, Ordering::Release);
        enqueue_pending(samples, source_rate_hz);
        return;
    };
    if c.source_rate_hz != Some(source_rate_hz) {
        c.have_prev = false;
        c.source_rate_hz = Some(source_rate_hz);
        log::info!("XIAOMI VOICE PCM source_rate={source_rate_hz}Hz -> 48000Hz ratio={ratio}");
    }
    let mut previous = if c.have_prev { c.prev } else { samples[0] };
    let mut out = Vec::with_capacity(samples.len() * ratio * 2);
    for &current in samples {
        let delta = current as i32 - previous as i32;
        for step in 1..=ratio {
            let s = if step == ratio {
                current
            } else {
                (previous as i32 + delta * step as i32 / ratio as i32) as i16
            };
            out.extend_from_slice(&s.to_le_bytes());
        }
        previous = current;
    }
    c.prev = samples[samples.len() - 1];
    c.have_prev = true;
    let peer = c.peer;
    let udp_ok = match c.sock.send_to(&out, peer) {
        Ok(_) => {
            c.sent.fetch_add(1, Ordering::Relaxed);
            FIRST_SEND_AT.lock().get_or_insert_with(Instant::now);
            true
        }
        Err(_) => {
            c.dropped.fetch_add(1, Ordering::Relaxed);
            false
        }
    };
    drop(guard);
    crate::bridges::xiaomi::voice_meter::on_pcm(samples, udp_ok);
}

/// 保留旧调用点兼容性；新代码应明确传入 ATVV 协商出的采样率。
pub fn push_16k(samples: &[i16]) {
    push_pcm(samples, 16_000);
}

pub fn stop() {
    READY.store(false, Ordering::Release);
    STARTING.store(false, Ordering::Release);
    INPUT_GATE_CLOSED.store(false, Ordering::Release);
    discard_pending();
    if let Some(c) = CLIENT.lock().take() {
        let _ = c.sock.send_to(b"CLEAR", c.peer);
    }
    crate::bridges::xiaomi::voice_meter::set_session(false);
}

pub fn stats() -> (u64, u64) {
    match CLIENT.lock().as_ref() {
        Some(c) => (
            c.sent.load(Ordering::Relaxed),
            c.dropped.load(Ordering::Relaxed),
        ),
        None => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_rates_have_expected_48k_ratios() {
        assert_eq!(48_000 / 8_000, 6);
        assert_eq!(48_000 / 16_000, 3);
    }

    #[test]
    fn pre_roll_is_bounded_and_drops_oldest_audio() {
        begin_session();
        // 1600 samples @ 16 kHz = 100 ms.  The third frame must evict the
        // oldest one because a voice shortcut may only hold 250 ms of audio.
        enqueue_pending(&vec![0; 1600], 16_000);
        enqueue_pending(&vec![0; 1600], 16_000);
        enqueue_pending(&vec![0; 1600], 16_000);
        let (queued_ms, dropped_ms) = pre_roll_stats();
        assert!(queued_ms <= MAX_PRE_ROLL_MS);
        assert_eq!(queued_ms, 200);
        assert_eq!(dropped_ms, 100);
        discard_pending();
        INPUT_GATE_CLOSED.store(false, Ordering::Release);
    }
}
