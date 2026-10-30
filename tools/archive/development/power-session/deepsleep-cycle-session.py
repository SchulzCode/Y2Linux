#!/usr/bin/env python3
"""Bounded diagnostic cycles after the first complete 04 RTC resume.

One core test, three short RTC cycles, one scheduled Power cycle, then one
five-minute RTC cycle. Stop on the first failed check; never retry a failure.
The matching context diagnostic is mandatory and external taint is retained.
Normal UART/console suspend is required. No flash, MMIO or charger changes.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def read(name):
    try:
        return Path(name).read_text().strip()
    except OSError:
        return None


def main():
    os.umask(0o077)
    os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'
    sys.path.insert(0, '/usr/lib/y2-platform')
    boot = read('/proc/sys/kernel/random/boot_id')
    root = Path('/data/logs/deepsleep-cycles-' + boot[:8])
    root.mkdir(mode=0o700)
    old_printk = read('/proc/sys/kernel/printk')
    spec = importlib.util.spec_from_file_location('q', '/run/y2/qualify-platform-v1-suspend.py')
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)

    def record(name, value):
        with (root / name).open('x') as stream:
            json.dump(value, stream)
            stream.flush()
            os.fsync(stream.fileno())
        fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)

    def guard():
        assert read('/proc/sys/kernel/random/boot_id') == boot, 'boot changed'
        assert read('/proc/sys/kernel/tainted') == '4096', 'unexpected taint'
        assert read('/sys/module/y2_pm_stack_probe/parameters/suppress_nested') == 'Y', 'context diagnostic absent'
        assert read('/sys/module/printk/parameters/console_suspend') == 'Y', 'normal console suspend required'
        assert read('/sys/class/power_supply/y2-usb-presence/online') == '0', 'USB present'
        assert read('/sys/class/power_supply/BAT0/status') == 'Discharging', 'not discharging'
        assert int(read('/sys/class/power_supply/BAT0/voltage_now')) >= 3800000, 'voltage floor'

    plan = [('core', 'core', 0, 'rtc'),
            ('rtc-1', 'none', 30, 'rtc'), ('rtc-2', 'none', 30, 'rtc'),
            ('rtc-3', 'none', 30, 'rtc'), ('power-1', 'none', 60, 'power'),
            ('rtc-5min', 'none', 310, 'rtc')]
    record('armed.json', {'boot_id': boot, 'diagnostic_only': True,
                         'plan': plan, 'power_entry_after_unplug_s': 180,
                         'owner_power_press_after_unplug_s': 200,
                         'reconnect_after_unplug_s': 600, 'automatic_retry': False})
    try:
        deadline = time.monotonic() + 1800
        while read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            if time.monotonic() > deadline:
                raise RuntimeError('unplug deadline')
            time.sleep(1)
        # Owner prompts follow elapsed wall time, including preceding suspend
        # residence. CLOCK_MONOTONIC deliberately excludes that residence.
        unplugged = time.clock_gettime(time.CLOCK_BOOTTIME)
        record('unplugged.json', {'boottime_s': unplugged,
                                  'monotonic_s': time.monotonic()})
        time.sleep(8)
        guard()
        assert read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing RTC alarm'
        Path('/proc/sys/kernel/printk').write_text('8\n')
        for name, mode, seconds, wake in plan:
            guard()
            if name == 'power-1':
                if time.clock_gettime(time.CLOCK_BOOTTIME) > unplugged + 170:
                    raise RuntimeError('missed scheduled Power entry; stop')
                time.sleep(max(0, unplugged + 180 - time.clock_gettime(time.CLOCK_BOOTTIME)))
                guard()
            directory = root / name
            directory.mkdir(mode=0o700)
            record(name + '-start.json', {'monotonic_s': time.monotonic()})
            with (directory / 'log').open('x') as log:
                result = subprocess.run(['python3', '-I', '/run/y2/suspend_platform_device.py',
                                         str(directory), mode, str(seconds), wake],
                                        stdout=log, stderr=log)
            if result.returncode or not (directory / 'result.json').exists():
                raise RuntimeError(name + ' did not return a receipt')
            result = json.loads((directory / 'result.json').read_text())
            verdict = q.verdict(result)
            record(name + '-verdict.json', verdict)
            # Retain the strict original verdict, including untainted=false.
            # Only progression within this explicitly tainted diagnostic skips it.
            if not all(value for key, value in verdict['checks'].items() if key != 'untainted'):
                raise RuntimeError(name + ' failed; no next cycle')
            if name == 'rtc-5min' and result['sleep_proxy_s'] < 300:
                raise RuntimeError('five-minute residence not achieved')
            time.sleep(2)
        record('complete.json', {'same_boot': read('/proc/sys/kernel/random/boot_id') == boot,
                                 'diagnostic_only': True, 'real_cycles': 5})
    except Exception as error:
        record('error.json', {'type': type(error).__name__, 'error': str(error)})
        raise
    finally:
        if old_printk:
            Path('/proc/sys/kernel/printk').write_text(old_printk + '\n')
        record('finished.json', {'monotonic_s': time.monotonic()})


if __name__ == '__main__':
    main()
