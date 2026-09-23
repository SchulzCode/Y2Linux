#!/usr/bin/env python3
"""Create offline bit/channel fixtures; never configure a sink or play audio.

Packed 24-bit WAV -> FFmpeg S32 left-aligned reference, plus ALSA S24_LE
24-in-32 sign-extended memory words. Captured I2S words must be compared with
these references only after the direct hardware path supports that format.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import csv
import hashlib
import json
from pathlib import Path
import struct
import wave

PATTERN = (0,1,-1,127,-127,255,-255,256,-256,257,-257,1023,-1023,32767,-32768,0)


def generate(directory, frames=4096):
    directory=Path(directory)
    directory.mkdir(parents=True,exist_ok=False)
    words=[]; packed=bytearray(); s24=bytearray(); s32=bytearray()
    for index in range(frames):
        left=PATTERN[index%len(PATTERN)]
        right=PATTERN[(index+5)%len(PATTERN)]
        for value in (left,right):
            packed.extend((value & 0xffffff).to_bytes(3,'little'))
            s24.extend(struct.pack('<i',value))
            s32.extend(struct.pack('<i',value<<8))
        if index<64:
            words.append([index,left,right,f'{(left<<8)&0xffffffff:08x}',f'{(right<<8)&0xffffffff:08x}'])
    (directory/'reference-s24le.raw').write_bytes(s24)
    (directory/'reference-s32le.raw').write_bytes(s32)
    for rate in (44100,48000,88200,96000):
        with wave.open(str(directory/f'precision-s24-{rate}.wav'),'wb') as stream:
            stream.setparams((2,3,rate,frames,'NONE','not compressed'));stream.writeframes(packed)
    with (directory/'expected-words.csv').open('w',newline='') as stream:
        writer=csv.writer(stream);writer.writerow(['frame','left_s24','right_s24','left_s32_hex','right_s32_hex']);writer.writerows(words)
    result={'schema':'org.y2linux.audio-precision-fixture/v1','evidence_level':'IMPLEMENTED',
            'frames':frames,'channels':2,'peak_source_s24':32768,'source_valid_bits':24,
            's24le':'ALSA 24 valid low bits in 32-bit signed container',
            's32le':'24 valid high bits; source multiplied by 256; low eight bits zero',
            'meaning':'Nonzero source bits below bit 8 detect a hidden S16 truncation; file proof is not hardware proof',
            'physical_playback':False,'hardware_formats_or_rates_enabled':False,'files':{}}
    for file in sorted(directory.iterdir()):
        result['files'][file.name]={'bytes':file.stat().st_size,'sha256':hashlib.sha256(file.read_bytes()).hexdigest()}
    (directory/'manifest.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('output',type=Path)
    print(json.dumps(generate(parser.parse_args().output),indent=2))
