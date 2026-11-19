const SAMPLE_RATE = 48_000;
export const MIN_CUE_MS = 50;
export const MAX_CUE_MS = 30_000;
export const DEFAULT_CUE_MS = 450;

export function formatVoiceTime(ms: number) {
  const minutes = Math.floor(ms / 60000);
  const seconds = ((ms % 60000) / 1000).toFixed(3).padStart(6, "0");
  return minutes + ":" + seconds;
}

export function parseVoiceTime(text: string) {
  const match = /^(?:(\d+):)?(\d+)(?:[.,](\d{1,3}))?$/.exec(text.trim());
  if (!match) return undefined;
  const [, minutes, seconds, fraction = ""] = match;
  if (minutes && Number(seconds) >= 60) return undefined;
  const ms = (Number(minutes ?? 0) * 60 + Number(seconds)) * 1000 + Number(fraction.padEnd(3, "0"));
  return Number.isSafeInteger(ms) ? ms : undefined;
}

export async function decodeVoiceSource(bytes: ArrayBuffer) {
  const context = new OfflineAudioContext(2, 1, SAMPLE_RATE);
  const decoded = await context.decodeAudioData(bytes);
  if (decoded.duration * 1000 < MIN_CUE_MS) throw new Error("audio is shorter than 50 milliseconds");
  return decoded;
}

export function initialVoiceRange(audio: AudioBuffer, edge: "start" | "end" = "start") {
  const totalMs = Math.floor(audio.length / 48);
  const durationMs = Math.min(DEFAULT_CUE_MS, totalMs);
  const channels = Array.from({ length: audio.numberOfChannels }, (_, channel) => audio.getChannelData(channel));
  // Scan for the first attack while ignoring silence and very faint noise.
  const stride = 12;
  const firstFrame = edge === "start" ? 0 : audio.length - 1;
  const direction = edge === "start" ? 1 : -1;
  for (let frame = firstFrame; frame >= 0 && frame < audio.length; frame += stride * direction) {
    if (channels.some(samples => Math.abs(samples[frame]) >= 0.018)) {
      const atMs = Math.floor(frame / 48);
      const onset = edge === "start" ? atMs - 12 : atMs + 12 - durationMs;
      const startMs = Math.min(totalMs - durationMs, Math.max(0, onset));
      return { startMs, durationMs };
    }
  }
  return { startMs: edge === "start" ? 0 : totalMs - durationMs, durationMs };
}

export function voiceWaveform(audio: AudioBuffer, startMs: number, endMs: number, count: number): number[] {
  const first = Math.floor(startMs * audio.sampleRate / 1000);
  const last = Math.min(audio.length, Math.ceil(endMs * audio.sampleRate / 1000));
  const channels = Array.from({ length: audio.numberOfChannels }, (_, index) => audio.getChannelData(index));
  const peaks = Array.from({ length: count }, (_, bin) => {
    const from = first + Math.floor((last - first) * bin / count);
    const to = first + Math.floor((last - first) * (bin + 1) / count);
    const step = Math.max(1, Math.floor((to - from) / 192));
    let peak = 0;
    for (let sample = from; sample < to; sample += step) {
      for (const channel of channels) peak = Math.max(peak, Math.abs(channel[sample]));
    }
    return peak;
  });
  const smoothed = peaks.map((peak, index) => (
    (peaks[index - 2] ?? peak) + 2 * (peaks[index - 1] ?? peak)
    + 3 * peak + 2 * (peaks[index + 1] ?? peak) + (peaks[index + 2] ?? peak)
  ) / 9);
  const ceiling = Math.max(.08, ...smoothed);
  return smoothed.map(peak => Math.pow(peak / ceiling, .75));
}

// Pass a small cropped PCM file to the native renderer. The original is retained
// separately so the range can be moved again after reopening settings.
export async function encodeVoiceFeedback(decoded: AudioBuffer, startMs: number, durationMs: number): Promise<Uint8Array> {
  const frames = durationMs * 48;

  const mix = new OfflineAudioContext(2, frames, SAMPLE_RATE);
  const source = mix.createBufferSource();
  source.buffer = decoded;
  source.connect(mix.destination);
  source.start(0, startMs / 1000);
  const stereo = await mix.startRendering();
  const left = stereo.getChannelData(0);
  const right = stereo.getChannelData(1);

  const bytes = new Uint8Array(44 + frames * 4);
  const view = new DataView(bytes.buffer);
  const text = new TextEncoder();
  bytes.set(text.encode("RIFF"), 0);
  view.setUint32(4, bytes.length - 8, true);
  bytes.set(text.encode("WAVEfmt "), 8);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 2, true);
  view.setUint32(24, SAMPLE_RATE, true);
  view.setUint32(28, SAMPLE_RATE * 4, true);
  view.setUint16(32, 4, true);
  view.setUint16(34, 16, true);
  bytes.set(text.encode("data"), 36);
  view.setUint32(40, frames * 4, true);
  const pcm16 = (sample: number) => Math.round(Math.max(-1, Math.min(1, sample)) * 32767);
  for (let index = 0; index < frames; index++) {
    view.setInt16(44 + index * 4, pcm16(left[index]), true);
    view.setInt16(46 + index * 4, pcm16(right[index]), true);
  }
  return bytes;
}
