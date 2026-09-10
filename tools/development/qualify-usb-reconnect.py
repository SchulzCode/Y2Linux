#!/usr/bin/env python3
"""Observe two owner-operated USB reconnects, retaining host state and SSH uptime."""
import argparse
import json
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('output', type=Path)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
ssh = ['ssh', '-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=3',
       '-o', 'ServerAliveInterval=2', '-o', 'ServerAliveCountMax=2',
       '-i', '/home/luca/.ssh/y2linux_ed25519', 'root@10.42.0.1']
remote = ('cat /proc/uptime; '
          'if test -f /tmp/y2-live-qualification/boot-token; then '
          'cat /tmp/y2-live-qualification/boot-token; else echo MISSING; fi; '
          'uname -a; cat /proc/1/stat; ip addr show usb0; '
          'ls -l /dev/ttyGS0; cat /sys/class/udc/*/state')

def present():
    for dev in Path('/sys/bus/usb/devices').glob('*'):
        try:
            if ((dev/'idVendor').read_text().strip() == '0525' and
                (dev/'idProduct').read_text().strip() == 'a4aa' and
                (dev/'serial').read_text().strip() == 'Y2LINUX-DEV-01'):
                return str(dev)
        except (FileNotFoundError, NotADirectoryError):
            pass
    return None

def snapshot(name):
    result = subprocess.run(ssh + [remote], capture_output=True, text=True, timeout=15)
    (a.output/f'{name}-ssh.txt').write_text(result.stdout + result.stderr)
    host = []
    for command in (['lsusb'], ['ip', '-br', 'address'], ['ip', 'route', 'get', '10.42.0.1']):
        r = subprocess.run(command, capture_output=True, text=True)
        host.append(f'$ {command!r}\nexit={r.returncode}\n{r.stdout}{r.stderr}')
    host.append('ACM nodes: ' + repr([str(x) for x in Path('/dev').glob('ttyACM*')]))
    (a.output/f'{name}-host.txt').write_text('\n'.join(host)+'\n')
    if result.returncode:
        return None
    lines = result.stdout.splitlines()
    return float(lines[0].split()[0]), lines[1]

events = []
def record(**data):
    events.append(dict(host_monotonic=time.monotonic(), host_unix=time.time(), **data))
    (a.output/'reconnect-events.json').write_text(json.dumps(events, indent=2)+'\n')
    print(json.dumps(events[-1]), flush=True)

baseline = snapshot('reconnect-before')
if baseline is None or not present():
    raise SystemExit('Need working SSH and attached verified Y2 before test')
record(event='ready', uptime=baseline[0], token=baseline[1])
deadline = time.monotonic() + 600
for cycle in (1, 2):
    while present():
        if time.monotonic() > deadline:
            raise SystemExit('Owner disconnect not observed within test window')
        time.sleep(1)
    detached = time.monotonic()
    record(event='USB_absent', cycle=cycle)
    while not present():
        if time.monotonic() > deadline:
            raise SystemExit('Y2 did not return within test window')
        time.sleep(1)
    record(event='USB_returned', cycle=cycle, disconnected_seconds=time.monotonic()-detached)
    result = None
    for attempt in range(15):
        result = snapshot(f'reconnect-{cycle}-attempt-{attempt}')
        if result is not None:
            break
        time.sleep(2)
    if result is None:
        raise SystemExit('USB returned, but SSH did not recover')
    elapsed = time.monotonic() - events[0]['host_monotonic']
    continuous = result[1] == baseline[1] and result[0] > baseline[0]
    record(event='SSH_returned', cycle=cycle, uptime=result[0], token=result[1],
           uptime_delta=result[0]-baseline[0], host_elapsed=elapsed,
           continued_boot=continuous)
    if not continuous:
        raise SystemExit('Boot continuity failed')
record(event='PASS', cycles=2)
