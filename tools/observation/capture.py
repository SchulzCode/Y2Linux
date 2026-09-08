#!/usr/bin/env python3
"""Bounded Linux serial receiver. Wiring, never O_RDONLY alone, prevents TX drive."""
import argparse
import copy
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import select
import stat
import sys
import termios
import time


def utc():
    return datetime.now(timezone.utc).isoformat()


def capture(device, output, setup, *, baud=921600, seconds=60, max_bytes=16*1024*1024):
    if baud not in (115200, 921600):
        raise ValueError('unsupported capture baud; no automatic baud sweep')
    if not math.isfinite(seconds) or not 0 < seconds <= 600:
        raise ValueError('duration must be >0 and <=600 seconds')
    if not 0 < max_bytes <= 64*1024*1024:
        raise ValueError('byte bound must be >0 and <=64 MiB')
    device = Path(device)
    if not device.is_absolute() or not stat.S_ISCHR(device.stat().st_mode):
        raise ValueError('an explicit absolute character-device path is required')
    setup_data = Path(setup).read_bytes()
    if not setup_data or len(setup_data) > 65536:
        raise ValueError('supply a nonempty setup/wiring record of at most 64 KiB')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    (output/'setup.txt').write_bytes(setup_data)
    report = dict(start_utc=utc(), device=str(device), resolved_device=str(device.resolve()),
                  baud=baud, format='8N1', flow_control='none', host=platform.platform(),
                  python=sys.version, seconds=seconds, max_bytes=max_bytes,
                  setup_sha256=hashlib.sha256(setup_data).hexdigest(),
                  tool_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  status='starting', hardware_proof=False,
                  note='Raw bytes require independent review; setup record is operator supplied.')
    fd = None
    saved = None
    size = 0
    digest = hashlib.sha256()
    result = 2
    try:
        with (output/'raw.bin').open('xb') as raw, (output/'chunks.jsonl').open('x') as chunks:
            fd = os.open(device, os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
            fcntl.ioctl(fd, termios.TIOCEXCL)
            saved = termios.tcgetattr(fd)
            attrs = copy.deepcopy(saved)
            attrs[0] = attrs[1] = attrs[3] = 0  # no translation, echo, canonical or XON/XOFF
            attrs[2] &= ~(termios.CSIZE | termios.PARENB | termios.PARODD |
                          termios.CSTOPB | termios.CRTSCTS | termios.HUPCL)
            attrs[2] |= termios.CS8 | termios.CREAD | termios.CLOCAL
            attrs[4] = attrs[5] = getattr(termios, 'B'+str(baud))
            attrs[6][termios.VMIN] = attrs[6][termios.VTIME] = 0
            termios.tcsetattr(fd, termios.TCSANOW, attrs)  # no flush; no data or modem writes
            applied = termios.tcgetattr(fd)
            # Linux normalizes encoded baud bits in c_cflag; Python versions
            # differ in speed constant representation. Compare actual speeds too.
            baud_bits = termios.CBAUD | termios.CIBAUD
            if (applied[0:2] != attrs[0:2] or applied[3:6] != attrs[3:6]
                    or (applied[2] & ~baud_bits) != (attrs[2] & ~baud_bits)):
                raise ValueError('driver did not retain requested raw serial settings')
            report['termios_flags_and_speeds'] = applied[:6]
            report['status'] = 'capturing'
            start = time.monotonic_ns()
            report['capture_start_utc'] = utc()
            (output/'capture.json').write_text(json.dumps(report, indent=2)+'\n')
            deadline = start + int(seconds*1e9)
            while True:
                remaining = (deadline-time.monotonic_ns())/1e9
                if remaining <= 0:
                    report['status'] = 'data-captured' if size else 'empty'
                    result = 0 if size else 2
                    break
                if not select.select([fd], [], [], min(remaining, .25))[0]:
                    continue
                try:
                    data = os.read(fd, min(65536, max_bytes-size))
                except BlockingIOError:
                    continue
                if not data:
                    raise OSError('serial device disconnected/end of stream')
                raw.write(data)
                raw.flush()
                digest.update(data)
                chunks.write(json.dumps(dict(offset=size, bytes=len(data),
                             elapsed_ns=time.monotonic_ns()-start))+'\n')
                chunks.flush()
                size += len(data)
                if size == max_bytes:
                    report['status'] = 'limit-reached-incomplete'
                    break
    except (OSError, ValueError, termios.error, KeyboardInterrupt) as exc:
        report['status'] = 'interrupted' if isinstance(exc, KeyboardInterrupt) else 'error'
        report['error'] = type(exc).__name__+': '+str(exc)
    finally:
        if fd is not None:
            try:
                if saved is not None:
                    termios.tcsetattr(fd, termios.TCSANOW, saved)
            except (OSError, termios.error) as exc:
                report['restore_error'] = str(exc)
                result = 2
            finally:
                os.close(fd)
        report.update(end_utc=utc(), bytes=size, sha256=digest.hexdigest(), exit_code=result)
        (output/'capture.json').write_text(json.dumps(report, indent=2)+'\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True, help='explicit /dev/serial/by-id/... path')
    parser.add_argument('--output', required=True, help='new evidence directory')
    parser.add_argument('--setup', required=True, help='verified wiring/adapter/voltage setup record')
    parser.add_argument('--baud', type=int, default=921600, choices=(115200,921600))
    parser.add_argument('--seconds', type=float, default=60)
    parser.add_argument('--max-bytes', type=int, default=16*1024*1024)
    args = parser.parse_args()
    try:
        report = capture(**vars(args))
    except (OSError, ValueError) as exc:
        parser.exit(2, str(exc)+'\n')
    print(json.dumps({k: report[k] for k in ('status','bytes','sha256','hardware_proof')}))
    return report['exit_code']


if __name__ == '__main__':
    sys.exit(main())
