#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Bounded read-only Y2 Bluetooth CPU/IRQ/idle sampling under the shared lock.

Does not activate radios, discover, connect, open a PCM, change screen/CPU policy,
install code or flash. Choose the runtime scenario separately before sampling.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import subprocess

REMOTE = r'''
import json, os, time
from pathlib import Path

def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None

def sample():
    result = {'monotonic': time.monotonic(), 'boot': read('/proc/sys/kernel/random/boot_id'),
              'stat': read('/proc/stat'), 'interrupts': read('/proc/interrupts'),
              'online': read('/sys/devices/system/cpu/online'), 'processes': {}, 'idle': {},
              'core': read('/sys/devices/platform/18070000.connectivity/status'),
              'reconnect': read('/run/y2/bt-reconnect.json')}
    for process in Path('/proc').glob('[0-9]*'):
        name = read(process/'comm')
        if name not in ('reborn', 'y2-bt-reconnect', 'bluealsad', 'bluetoothd', 'dbus-daemon'):
            continue
        result['processes'][process.name] = {'name': name, 'stat': read(process/'stat'),
                                            'status': read(process/'status'), 'threads': {}}
        for thread in (process/'task').glob('*'):
            result['processes'][process.name]['threads'][thread.name] = {
                'name': read(thread/'comm'), 'stat': read(thread/'stat'),
                'status': read(thread/'status'), 'wchan': read(thread/'wchan')}
    for state in Path('/sys/devices/system/cpu').glob('cpu*/cpuidle/state*'):
        result['idle'][str(state)] = {key: read(state/key) for key in ('name', 'usage', 'time', 'disable')}
    return result

result = {'hz': os.sysconf('SC_CLK_TCK'), 'page': os.sysconf('SC_PAGE_SIZE'), 'before': sample()}
time.sleep(SECONDS)
result['after'] = sample()
print(json.dumps(result))
'''


def ticks(stat):
    fields = stat[stat.rfind(')') + 2:].split()
    return int(fields[11]) + int(fields[12]), int(fields[19])


def field(status, key):
    if status is None:
        return None
    return next((int(line.split(':', 1)[1].split()[0]) for line in status.splitlines()
                 if line.startswith(key + ':')), None)


def summary(raw):
    before, after = raw['before'], raw['after']
    seconds = after['monotonic'] - before['monotonic']
    if not before['boot'] or before['boot'] != after['boot'] or seconds <= 0:
        raise ValueError('boot or monotonic interval changed; sample is not comparable')
    result = {'seconds': seconds, 'boot': after['boot'], 'online_before': before['online'],
              'online_after': after['online'], 'processes': {}, 'idle': {},
              'battery_claim': None, 'context_switches_are_wakeup_proxies': True}
    for pid, process in before['processes'].items():
        current = after['processes'].get(pid)
        if not current or not process['stat'] or not current['stat']:
            continue
        old_ticks, old_start = ticks(process['stat'])
        new_ticks, new_start = ticks(current['stat'])
        if old_start != new_start:
            continue
        value = {'pid': pid, 'cpu_percent_one_core': 100 * (new_ticks - old_ticks) / raw['hz'] / seconds,
                 'rss_kib': field(current['status'], 'VmRSS'), 'threads': {}}
        for tid, thread in process['threads'].items():
            latest = current['threads'].get(tid)
            if not latest or not thread['stat'] or not latest['stat']:
                continue
            old_cpu, old_start = ticks(thread['stat'])
            new_cpu, new_start = ticks(latest['stat'])
            if old_start != new_start:
                continue
            counts = {}
            for key in ('voluntary_ctxt_switches', 'nonvoluntary_ctxt_switches'):
                old, new = field(thread['status'], key), field(latest['status'], key)
                counts[key + '_per_second'] = (new - old) / seconds if old is not None and new is not None else None
            value['threads'][tid] = {'name': latest['name'], **counts,
                                    'cpu_percent_one_core': 100 * (new_cpu - old_cpu) / raw['hz'] / seconds}
        result['processes'][process['name']] = value
    for key in ('ctxt', 'intr'):
        def counter(snapshot):
            return int(next(line.split()[1] for line in snapshot['stat'].splitlines()
                            if line.startswith(key + ' ')))
        result[key + '_per_second'] = (counter(after) - counter(before)) / seconds
    for path, state in after['idle'].items():
        old = before['idle'].get(path)
        if not old or any(state[k] is None or old[k] is None for k in ('usage', 'time')):
            continue
        residency = int(state['time']) - int(old['time'])
        result['idle'][path] = {'name': state['name'], 'entries': int(state['usage']) - int(old['usage']),
                               'residency_us': residency, 'residency_percent': residency / seconds / 10000}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', default='root@10.42.0.1')
    parser.add_argument('--private-key', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=20, choices=range(5, 31))
    parser.add_argument('--output', type=Path, required=True, help='new private evidence directory')
    args = parser.parse_args()
    descriptor = os.open('/tmp/y2-platform-v1-device.lock', os.O_RDWR | os.O_CREAT, 0o600)
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.exit(75, 'Y2 device lock busy; no device command sent\n')
        args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        command = ['ssh', '-T', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=4',
                   '-i', str(args.private_key), args.host, 'python3', '-']
        response = subprocess.run(command, input=REMOTE.replace('time.sleep(SECONDS)', f'time.sleep({args.seconds})'),
                                  text=True, capture_output=True, timeout=args.seconds + 12)
        (args.output/'raw.json').write_text(response.stdout)
        (args.output/'stderr.txt').write_text(response.stderr)
        if response.returncode:
            parser.exit(response.returncode, 'SSH measurement failed; receipt retained\n')
        result = summary(json.loads(response.stdout))
        (args.output/'summary.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
    finally:
        os.close(descriptor)


if __name__ == '__main__':
    main()
