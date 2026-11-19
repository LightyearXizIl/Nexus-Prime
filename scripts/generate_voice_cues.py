"""Generate voice cues, with optional CC0 source processing via FFmpeg."""
from argparse import ArgumentParser
from array import array
from math import exp, pi, sin
from pathlib import Path
from struct import pack
from subprocess import run
from zipfile import ZipFile
import wave

RATE = 48000
ASSETS = Path(__file__).resolve().parents[1] / "src-tauri" / "assets" / "voice-feedback"

# Each note defines onset, length, initial/final pitch, gain and harmonic mix.
PAIRS = {
    "soft": (
        [(0, 110, 560, 740, 1, (1, .14, .03))],
        [(0, 96, 630, 440, 1, (1, .10, .02))],
    ),
    "glass": (
        [(0, 118, 940, 954, 1, (1, .08, .20)), (24, 104, 1410, 1410, .52, (1, .13))],
        [(0, 94, 1150, 1140, .72, (1, .08, .16)), (22, 96, 765, 765, 1, (1, .10))],
    ),
    "pulse": (
        [(0, 68, 470, 520, .85, (1, .26, .09)), (30, 67, 680, 680, 1, (1, .19, .07))],
        [(0, 64, 615, 564, .8, (1, .25, .08)), (26, 65, 430, 400, 1, (1, .20))],
    ),
    "wood": (
        [(0, 108, 455, 448, 1, (1, .19, .13, .13)), (13, 80, 758, 758, .44, (1, .06))],
        [(0, 104, 330, 318, 1, (1, .24, .21, .10)), (9, 60, 550, 550, .35, (1, .10))],
    ),
    "air": (
        [(0, 136, 680, 925, .95, (1, .04)), (25, 113, 1010, 1180, .36, (1, .08))],
        [(0, 125, 900, 648, .95, (1, .05)), (16, 108, 1190, 850, .27, (1, .07))],
    ),
    "crystal": (
        [(0, 118, 1175, 1172, 1, (1, .25, .04)), (32, 124, 1568, 1565, .32, (1, .12))],
        [(0, 118, 988, 982, 1, (1, .22, .03)), (28, 112, 740, 737, .49, (1, .08))],
    ),
    "reply": (
        [(0, 127, 360, 506, 1, (1, .14, .02)), (35, 110, 592, 618, .45, (1, .03))],
        [(0, 130, 457, 288, 1, (1, .12, .05)), (28, 112, 353, 320, .28, (1, .03))],
    ),
}

KENNEY_CLIPS = {
    "switch-start": ("switch_004.ogg", 0, 85),
    "switch-end": ("switch_004.ogg", 96, 79),
    "notice-start": ("confirmation_001.ogg", 0, 156),
    "notice-end": ("confirmation_003.ogg", 0, 150),
}


def synth(notes, preset):
    clip = [0.0] * (max(offset + length for offset, length, *_ in notes) * 48)
    for offset, length, pitch, end_pitch, gain, harmonics in notes:
        seconds = length / 1000
        for index in range(length * 48):
            t = index / RATE
            attack = .013 if preset in ("air", "reply") else .0035
            envelope = sin(min(1, t / attack) * pi / 2) ** 2
            envelope *= exp(-t / (seconds * (.46 if preset == "wood" else .6)))
            envelope *= sin(min(1, (seconds - t) / .026) * pi / 2) ** 2
            phase = 2 * pi * (pitch*t + (end_pitch-pitch)*t*t/(2*seconds))
            signal = sum(weight * sin((harmonic + 1) * phase) for harmonic, weight in enumerate(harmonics))
            clip[offset * 48 + index] += gain * envelope * signal
    peak = max(abs(sample) for sample in clip)
    level = .54 if preset in ("glass", "pulse", "crystal", "reply") else .60
    return b"".join(pack("<hh", value, value) for value in (
        round(sample / peak * level * 32767) for sample in clip
    ))


def write_wav(path, pcm):
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as audio:
        audio.setparams((2, 2, RATE, 0, "NONE", "not compressed"))
        audio.writeframes(pcm)


def convert_kenney(archive, ffmpeg):
    with ZipFile(archive) as source:
        for output, (filename, start_ms, length_ms) in KENNEY_CLIPS.items():
            command = [
                ffmpeg, "-v", "error", "-i", "pipe:0",
                "-f", "f32le", "-ar", str(RATE), "-ac", "1", "pipe:1",
            ]
            decoded = run(command, input=source.read("Audio/" + filename), capture_output=True, check=True)
            frames = array("f", decoded.stdout)[start_ms * 48:(start_ms + length_ms) * 48]
            gain = .54 * 32767 / max(map(abs, frames))
            pcm = [
                round(frame * gain * min(1, index / 72, (len(frames) - 1 - index) / 864))
                for index, frame in enumerate(frames)
            ]
            write_wav(ASSETS / f"{output}.wav", b"".join(pack("<hh", value, value) for value in pcm))


if __name__ == "__main__":
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("--kenney", type=Path, help="Kenney Interface Sounds 1.0 ZIP (CC0)")
    parser.add_argument("--ffmpeg", default="ffmpeg", help="FFmpeg executable used to decode the CC0 sources")
    args = parser.parse_args()
    for name, pair in PAIRS.items():
        for edge, notes in zip(("start", "end"), pair):
            write_wav(ASSETS / f"{name}-{edge}.wav", synth(notes, name))
    if args.kenney:
        convert_kenney(args.kenney, args.ffmpeg)
