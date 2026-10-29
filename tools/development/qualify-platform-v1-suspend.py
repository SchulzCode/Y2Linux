#!/usr/bin/env python3
"""Locked one-shot observer for the installed Y2 full-suspend path.

Inspect first, then explicitly request each pm_test stage or full trial.
A lost SSH launch acknowledgement is never permission to repeat entry.
Physical Power gestures and repeated-cycle admission remain owner controlled.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time
import uuid
import sys



def wake_irqs(text):
    """Serviced PMIC child IRQs in /proc/interrupts (as the platform's sleep receipts count them).

    Both RTC and Power feed SPM EINT. That shared SPM wake bit is deliberately
    insufficient; only a same-boot child-IRQ delta attributes their source.
    """
    counts = {}
    for line in (text or '').splitlines():
        fields = line.split()
        if not fields or not fields[0].endswith(':'):
            continue
        for label, name in (('mt6397-rtc', 'rtc'), ('mtk-pmic-keys', 'power')):
            if label not in fields:
                continue
            total = 0
            for field in fields[1:]:
                if not field.isdecimal():
                    break
                total += int(field)
            counts[name] = counts.get(name, 0) + total
    return counts


DEVICE_LOCK = '/tmp/y2-platform-v1-device.lock'


def device_lock(inherited_fd=None):
    """Retain a coordinator's lease across separately reviewed one-shot trials."""
    if inherited_fd is None:
        return open(DEVICE_LOCK, 'a')
    lock = os.fdopen(os.dup(inherited_fd), 'r')
    actual, expected = os.fstat(lock.fileno()), os.stat(DEVICE_LOCK)
    if (actual.st_dev, actual.st_ino) != (expected.st_dev, expected.st_ino):
        lock.close()
        raise ValueError('inherited device lease does not name the shared lock')
    return lock


def fields(text):
    return dict(re.findall(r'(\w+)=(\S+)', text or ''))


def verdict(result):
    before, after = result['before']['files'], result['after']['files']
    same = before.get('/proc/sys/kernel/random/boot_id') == after.get('/proc/sys/kernel/random/boot_id')
    kernel = fields(after.get('/sys/firmware/y2_pm/state'))
    spm_before = fields(before.get('/sys/devices/platform/10006000.power-controller/state'))
    spm_after = fields(after.get('/sys/devices/platform/10006000.power-controller/state'))
    def delta(key):
        try:
            return int(spm_after[key], 0) - int(spm_before[key], 0)
        except (KeyError, ValueError):
            return None
    deep = result['mode'] == 'none'
    irq_before = wake_irqs(before.get('/proc/interrupts'))
    irq_after = wake_irqs(after.get('/proc/interrupts'))
    wakes = [name for name in ('rtc', 'power') if name in irq_before and name in irq_after
             and irq_after[name] > irq_before[name]]
    checks = {'same_boot': same, 'untainted': after.get('/proc/sys/kernel/tainted') == '0',
              'helper_returned': result.get('helper', {}).get('code') == 0,
              'kernel_exit': kernel.get('valid') == '1' and kernel.get('stage') in {'EXIT', 'COMPLETE'} and kernel.get('error') == '0',
              'pm_test_restored': '[none]' in (after.get('/sys/power/pm_test') or '')}
    if deep:
        checks.update(reset_return=kernel.get('reset_resume_entry') == '0x59325253',
                      suspend_pcm=kernel.get('pcm_length') == '596',
                      real_entry=delta('entries') == 1, real_resume=delta('resumes') == 1,
                      positive_residence=result.get('sleep_proxy_s', 0) > 1,
                      wake_source=result.get('expected_wake') in wakes,
                      alarm_cleared=not result.get('alarm_seconds') or (result.get('alarm_clear', {}).get('code') == 0 and after.get('/sys/class/rtc/rtc0/wakealarm') in {'', '0'}),
                      pcm_clean=spm_after.get('debug') in {'0', '0x0'} and spm_after.get('broken') == '0')
        if result.get('expected_wake') == 'rtc' and 'alarm_before_cleanup' in result:
            checks['rtc_auto_disarmed'] = result['alarm_before_cleanup'].get('wakealarm') in {'', '0'}
    else:
        checks['no_deep_entry'] = delta('entries') == 0 and delta('resumes') == 0
    for path, value in before.items():
        if path.startswith('/sys/fs/ext4/') and path.endswith('/errors_count'):
            checks['filesystem:' + path] = value == after.get(path) == '0'
    return {'wake_sources': wakes, 'pass': all(checks.values()), 'deep_resume': deep and all(checks.values()), 'checks': checks}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--host', default='root@10.42.0.1')
    p.add_argument('--ssh-port', type=int)
    p.add_argument('--ssh-config', type=Path, help='existing pinned owner SSH configuration')
    p.add_argument('--host-key-alias', help='verify a temporary Wi-Fi listener against the known USB host key')
    p.add_argument('--device-lock-fd', type=int, help='inherited shared-lock descriptor from a retained coordinator lease')
    p.add_argument('--key', type=Path, default=Path('/home/luca/.ssh/y2linux_ed25519'))
    p.add_argument('--stage', choices=['inspect', 'freezer', 'devices', 'platform', 'processors', 'core', 'none'], default='inspect')
    p.add_argument('--alarm-seconds', type=int, default=0)
    p.add_argument('--wake-source', choices=['rtc', 'power'], default='rtc')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    if a.ssh_port is not None and not 1 <= a.ssh_port <= 65535:
        p.error('--ssh-port must be 1..65535')
    if a.ssh_config and not a.ssh_config.is_file():
        p.error('--ssh-config must already exist')
    # Hostnames and key paths are passed as argv, never interpolated into shell.
    with device_lock(a.device_lock_fd) as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit('DEVICE_BUSY: no device access performed')
        a.output.mkdir(parents=True, exist_ok=False)
        ssh = ['ssh', '-T', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5', '-i', str(a.key)]
        if a.ssh_config:
            ssh += ['-F', str(a.ssh_config)]
        if a.ssh_port is not None:
            ssh += ['-p', str(a.ssh_port)]
        if a.host_key_alias:
            ssh += ['-oHostKeyAlias=' + a.host_key_alias]
        ssh.append(a.host)
        def remote(code, timeout=15):
            return subprocess.run(ssh + [code], capture_output=True, text=True, timeout=timeout)
        inspect = remote("uname -a; cat /etc/os-release; cat /proc/sys/kernel/random/boot_id /proc/sys/kernel/tainted; ip -4 addr show wlan0; cat /sys/firmware/y2_pm/state /sys/firmware/y2_pm/previous /sys/firmware/y2_pm/backstop_s; cat /sys/class/power_supply/BAT0/status; y2-platform status system")
        (a.output / 'inspect.txt').write_text(inspect.stdout + inspect.stderr)
        if inspect.returncode or a.stage == 'inspect':
            raise SystemExit(inspect.returncode)
        ident = uuid.uuid4().hex
        directory = '/run/y2/suspend-platform-' + ident
        source = Path(__file__).with_name('suspend_platform_device.py').read_text()
        # Publish a resident observer, then launch exactly once and detach it.
        code = "import os, pathlib, subprocess; p=pathlib.Path(" + repr(directory) + "); p.mkdir(mode=0o700); (p/'observer.py').write_text(" + repr(source) + "); f=open(p/'log','wb'); x=subprocess.Popen(['python3',str(p/'observer.py'),str(p)," + repr(a.stage) + "," + repr(str(a.alarm_seconds)) + "," + repr(a.wake_source) + "],stdin=subprocess.DEVNULL,stdout=f,stderr=f,start_new_session=True); print(x.pid)"
        launch = remote('python3 -c ' + shlex.quote(code))
        (a.output / 'launch.json').write_text(json.dumps({'directory': directory, 'code': launch.returncode, 'output': launch.stdout, 'error': launch.stderr}))
        if launch.returncode:
            raise SystemExit('launch not acknowledged; do not retry')
        deadline = time.monotonic() + max(120, a.alarm_seconds + 120)
        while time.monotonic() < deadline:
            time.sleep(2)
            try:
                answer = remote('cat ' + shlex.quote(directory + '/result.json'), timeout=8)
            except subprocess.TimeoutExpired:
                continue  # observation retries only; entry is never relaunched
            if answer.returncode == 0:
                result = json.loads(answer.stdout)
                (a.output / 'result.json').write_text(json.dumps(result, indent=2))
                checked = verdict(result)
                (a.output / 'verdict.json').write_text(json.dumps(checked, indent=2))
                print(json.dumps(checked))
                raise SystemExit(0 if checked['pass'] else 1)
        recovery = remote('cat /proc/sys/kernel/random/boot_id /sys/firmware/y2_pm/previous /sys/firmware/y2_pm/devices_previous; cat ' + shlex.quote(directory + '/log'))
        (a.output / 'no-return.txt').write_text(recovery.stdout + recovery.stderr)
        raise SystemExit('no checked return; stop further entry trials')


if __name__ == '__main__':
    main()
