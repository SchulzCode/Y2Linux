#!/usr/bin/env python3
"""Resident staged PM + ONE real Power-wake trial, with an RTC recovery alarm.

Owner gesture: unplug, wait 3 min 20 s, brief Power once; reconnect at 5 min.
The real entry is scheduled 180 s after cable removal and cancelled if staging
misses that deadline. A fallback RTC wake is not a Power-wake pass. Backstop
reboot is always failure. No retry, MMIO, flash or protection change.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time


def read(name):
    try:
        return Path(name).read_text().strip()
    except OSError:
        return None


def main():
    os.umask(0o077)
    os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'
    boot = read('/proc/sys/kernel/random/boot_id')
    root = Path('/data/logs/deepsleep-power-' + boot[:8])
    root.mkdir(mode=0o700)  # existing session means no repeat entry
    sys.path.insert(0, '/usr/lib/y2-platform')
    spec = importlib.util.spec_from_file_location('qualification', '/run/y2/qualify-platform-v1-suspend.py')
    qualification = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(qualification)

    def record(name, value):
        with (root / name).open('x') as stream:
            json.dump(value, stream)
            stream.flush()
            os.fsync(stream.fileno())

    def guard():
        assert read('/proc/sys/kernel/random/boot_id') == boot, 'boot changed'
        assert read('/proc/sys/kernel/tainted') == '0', 'kernel tainted'
        assert read('/sys/class/power_supply/y2-usb-presence/online') == '0', 'USB present'
        assert read('/sys/class/power_supply/BAT0/status') == 'Discharging', 'not discharging'
        assert int(read('/sys/class/power_supply/BAT0/voltage_now')) >= 3800000, 'voltage floor'

    def preserve(label):
        # Archive before a trial that can invoke the existing bounded backstop.
        # Never recurse into this session directory or clear pstore.
        with (root / (label + '.tar')).open('xb') as output:
            with tarfile.open(fileobj=output, mode='w') as archive:
                sources = [Path('/data/system/platform'), Path('/data/reborn/logs'),
                           Path('/sys/fs/pstore')]
                sources += list(Path('/data/logs').glob('system.log*'))
                for source in sources:
                    if source.exists():
                        archive.add(source, arcname=str(source).lstrip('/'), recursive=True)
            output.flush()
            os.fsync(output.fileno())
        with (root / (label + '-dmesg.txt')).open('x') as output:
            subprocess.run(['dmesg'], stdout=output, check=True)
            output.flush()
            os.fsync(output.fileno())
        record(label + '-pm.json', {str(path): read(path) for path in
                                  Path('/sys/firmware/y2_pm').iterdir() if path.is_file()})
        record(label + '-wakes.json', {
            'wakeup_sources': read('/sys/kernel/debug/wakeup_sources'),
            'power': read('/run/y2/power.json'), 'interrupts': read('/proc/interrupts'),
            'monotonic_s': time.monotonic(), 'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME)})
        os.sync()

    record('armed.json', {'boot_id': boot, 'plan': ['wait_unplug', 'freezer', 'devices',
           'platform', 'processors', 'core', 'at_180s_one_real_power_trial_rtc_fallback_60s'],
           'expected_owner_power_seconds_after_unplug': 200,
           'no_automatic_retry': True})
    try:
        deadline = time.monotonic() + 1800
        while read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            if time.monotonic() > deadline:
                raise RuntimeError('unplug deadline')
            time.sleep(2)
        unplugged = time.monotonic()
        record('unplugged.json', {'monotonic_s': unplugged, 'real_entry_target_s': unplugged + 180})
        time.sleep(8)
        guard()
        assert read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing RTC alarm'
        preserve('before-stages')
        for mode in ('freezer', 'devices', 'platform', 'processors', 'core', 'none'):
            guard()
            if mode == 'none':
                if time.monotonic() > unplugged + 170:
                    raise RuntimeError('stages missed Power trial deadline; no real entry')
                preserve('before-real')
                if time.monotonic() > unplugged + 178:
                    raise RuntimeError('archive missed Power trial deadline; no real entry')
                time.sleep(max(0, unplugged + 180 - time.monotonic()))
                guard()
            directory = root / mode
            directory.mkdir(mode=0o700)
            record(mode + '-start.json', {'monotonic_s': time.monotonic()})
            with (directory / 'log').open('x') as log:
                result = subprocess.run(['python3', '-I', '/run/y2/suspend_platform_device.py',
                         str(directory), mode, '60' if mode == 'none' else '0', 'power'],
                         stdout=log, stderr=log)
            if result.returncode or not (directory / 'result.json').exists():
                raise RuntimeError('stage ' + mode + ' did not return a receipt')
            verdict = qualification.verdict(json.loads((directory / 'result.json').read_text()))
            record(mode + '-verdict.json', verdict)
            if not verdict['pass']:
                raise RuntimeError('stage ' + mode + ' failed verdict')
            time.sleep(2)
        record('complete.json', {'same_boot': read('/proc/sys/kernel/random/boot_id') == boot})
    except Exception as error:
        record('error.json', {'type': type(error).__name__, 'error': str(error),
                              'monotonic_s': time.monotonic()})
        raise
    finally:
        record('finished.json', {'monotonic_s': time.monotonic()})


if __name__ == '__main__':
    main()
