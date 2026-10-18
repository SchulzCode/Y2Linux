#!/usr/bin/env python3
"""Write the Y2 headphone measurement signal: one 16-bit 44.1 kHz stereo file
whose sections are listed in docs/HEADPHONE-OUTPUT.md (Measurement plan).

    python3 tools/audio/test_signals.py out/audio-test-signals

writes "Y2 Headphone Test Signal.wav" (and .flac when the flac tool exists)
plus a timeline in sections.txt. Standard library only; deterministic output.
"""
import argparse
import math
import shutil
import struct
import subprocess
import wave
from pathlib import Path

RATE = 44_100
FULL = 32_767


def db(value):
    return 10 ** (value / 20)


def silence(seconds):
    return [(0, 0)] * round(seconds * RATE)


def sine(seconds, frequency, level_db, left=True, right=True):
    amplitude = db(level_db)
    count = round(seconds * RATE)
    fade = round(0.01 * RATE)  # 10 ms raised-cosine ends: no clicks
    out = []
    for n in range(count):
        edge = min(n, count - 1 - n)
        window = 1.0 if edge >= fade else 0.5 - 0.5 * math.cos(math.pi * edge / fade)
        v = round(FULL * amplitude * window * math.sin(2 * math.pi * frequency * n / RATE))
        out.append((v if left else 0, v if right else 0))
    return out


def sweep(seconds, start, stop, level_db):
    """Exponential (Farina) sine sweep: equal time per octave."""
    amplitude = db(level_db)
    count = round(seconds * RATE)
    k = math.log(stop / start)
    fade = round(0.02 * RATE)
    out = []
    for n in range(count):
        t = n / RATE
        phase = 2 * math.pi * start * seconds / k * (math.exp(t / seconds * k) - 1)
        edge = min(n, count - 1 - n)
        window = 1.0 if edge >= fade else 0.5 - 0.5 * math.cos(math.pi * edge / fade)
        v = round(FULL * amplitude * window * math.sin(phase))
        out.append((v, v))
    return out


SECTIONS = [
    ("lead-in silence", lambda: silence(2)),
    ("marker: 1 kHz -20 dBFS, both channels", lambda: sine(2, 1000, -20)),
    ("digital silence: idle noise", lambda: silence(30)),
    ("1 kHz -20 dBFS: level, THD+N, output impedance", lambda: sine(10, 1000, -20)),
    ("1 kHz -1 dBFS: highest output at this level, THD+N", lambda: sine(5, 1000, -1)),
    ("1 kHz -20 dBFS left only: crosstalk into right", lambda: sine(4, 1000, -20, right=False)),
    ("1 kHz -20 dBFS right only: crosstalk into left", lambda: sine(4, 1000, -20, left=False)),
    ("50 Hz -20 dBFS: bass level and output impedance", lambda: sine(5, 50, -20)),
    ("silence before the sweep", lambda: silence(2)),
    ("log sweep 20 Hz-20 kHz -12 dBFS: frequency response", lambda: sweep(10, 20, 20_000, -12)),
    ("silence", lambda: silence(2)),
    ("log sweep 20 Hz-20 kHz -12 dBFS (repeat)", lambda: sweep(10, 20, 20_000, -12)),
    ("tail silence", lambda: silence(2)),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    wav = args.output / "Y2 Headphone Test Signal.wav"
    timeline = []
    position = 0
    with wave.open(str(wav), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        for name, make in SECTIONS:
            frames = make()
            w.writeframes(b"".join(struct.pack("<hh", l, r) for l, r in frames))
            timeline.append(f"{position / RATE:7.2f}-{(position + len(frames)) / RATE:7.2f} s  {name}")
            position += len(frames)
    (args.output / "sections.txt").write_text(
        "Y2 Headphone Test Signal, 16-bit 44.1 kHz stereo\n" + "\n".join(timeline) + "\n")
    if shutil.which("flac"):
        subprocess.run(["flac", "--silent", "--force", "--best", "--no-seektable",
                        str(wav), "-o", str(wav.with_suffix(".flac"))], check=True)
    print("\n".join(timeline))


if __name__ == "__main__":
    main()
