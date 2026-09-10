#!/usr/bin/env python3
"""Generate two 3-second stereo S16 WAVs at -30 dBFS with 50ms fades."""
import argparse, math, struct, wave
from pathlib import Path

def generate(directory):
    directory.mkdir(parents=True, exist_ok=True)
    for rate in (44100, 48000):
        data = bytearray()
        count = rate * 3
        for i in range(count):
            fade = min(1.0, i / (rate * .05), (count - 1 - i) / (rate * .05))
            # Different frequencies permit left/right identification; no DC.
            data.extend(struct.pack('<hh', *(round(32767 * 10**(-30/20) * fade *
                        math.sin(2 * math.pi * hz * i / rate)) for hz in (440, 660))))
        with wave.open(str(directory / f'headphone-{rate}.wav'), 'wb') as wav:
            wav.setparams((2, 2, rate, count, 'NONE', 'not compressed'))
            wav.writeframes(data)
    (directory / 'README.txt').write_text('3 seconds; S16_LE stereo; L=440 Hz R=660 Hz; -30 dBFS peak; 50 ms fades.\nSet Master Playback Volume to -24 dB and enable Headphone only when ready.\nDo not use speaker-test for the first physical acceptance.\n')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    generate(parser.parse_args().directory)
