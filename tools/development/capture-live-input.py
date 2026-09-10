#!/usr/bin/env python3
"""Capture four normal evdev streams over SSH, retaining raw ARM input_event bytes."""
import argparse
import concurrent.futures
from pathlib import Path
import subprocess
import struct

parser = argparse.ArgumentParser()
parser.add_argument('output', type=Path)
parser.add_argument('--seconds', type=int, default=300)
args = parser.parse_args()
if not 1 <= args.seconds <= 600:
    parser.error('capture must be bounded to 1..600 seconds')
args.output.mkdir(parents=True, exist_ok=True)

def capture(n):
    base = args.output / f'input-event{n}'
    command = (f'cat /dev/input/event{n} & reader=$!; '
               f'sleep {args.seconds}; kill "$reader"; wait "$reader"')
    with base.with_suffix('.bin').open('wb') as out, base.with_suffix('.stderr').open('wb') as err:
        result = subprocess.run(['ssh', '-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10',
                                 '-i', '/home/luca/.ssh/y2linux_ed25519',
                                 'root@10.42.0.1', command], stdout=out, stderr=err)
    data = base.with_suffix('.bin').read_bytes()
    with base.with_suffix('.txt').open('w') as out:
        out.write(f'event{n}: ssh_exit={result.returncode}; bytes={len(data)}; record_size=16\n')
        for offset in range(0, len(data) - 15, 16):
            sec, usec, kind, code, value = struct.unpack_from('<iiHHi', data, offset)
            out.write(f'{sec}.{usec:06d} type={kind} code={code} value={value}\n')
        if len(data) % 16:
            out.write('ERROR: partial event record\n')
    print(base, len(data), flush=True)

with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    list(pool.map(capture, range(4)))
