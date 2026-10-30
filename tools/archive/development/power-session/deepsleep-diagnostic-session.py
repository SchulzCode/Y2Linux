#!/usr/bin/env python3
"""One resident RTC diagnostic after a reproduced 04 post-thaw failure.

Requires the matching, explicitly loaded stack probe and the host device lease.
External-module taint is recorded, never promoted into product qualification.
No retries, hardware writes, protection changes or automatic reboot commands.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def main():
    os.umask(0o077)
    os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'
    sys.path.insert(0, '/usr/lib/y2-platform')
    options = set(sys.argv[1:])
    assert options <= {'--context-repair', '--console-debug'}, 'unsupported diagnostic mode'
    repair = '--context-repair' in options
    console_debug = '--console-debug' in options
    boot = read('/proc/sys/kernel/random/boot_id')
    root = Path('/data/logs/deepsleep-diagnostic-' + boot[:8])
    root.mkdir(mode=0o700)
    old_printk = read('/proc/sys/kernel/printk')
    console_suspend = Path('/sys/module/printk/parameters/console_suspend')
    old_console_suspend = read(console_suspend)
    spec = importlib.util.spec_from_file_location('q', '/run/y2/qualify-platform-v1-suspend.py')
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)

    def record(name, data):
        with (root / name).open('x') as stream:
            json.dump(data, stream)
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
        assert Path('/sys/module/y2_pm_stack_probe').is_dir(), 'probe absent'
        if repair:
            assert read('/sys/module/y2_pm_stack_probe/parameters/suppress_nested') == 'Y', 'context repair not enabled'
        assert read('/sys/class/power_supply/y2-usb-presence/online') == '0', 'USB present'
        assert read('/sys/class/power_supply/BAT0/status') == 'Discharging', 'not discharging'
        assert int(read('/sys/class/power_supply/BAT0/voltage_now')) >= 3800000, 'voltage floor'

    record('armed.json', {'boot_id': boot, 'diagnostic_only': True,
           'context_repair': repair,
           'console_debug': console_debug, 'old_console_suspend': old_console_suspend,
           'expected_taint': 4096, 'plan': ['wait_unplug', 'core', 'one_real_rtc_30s'],
           'old_printk': old_printk, 'automatic_retry': False})
    try:
        deadline = time.monotonic() + 1800
        while read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            if time.monotonic() > deadline:
                raise RuntimeError('unplug deadline')
            time.sleep(2)
        record('unplugged.json', {'monotonic_s': time.monotonic()})
        time.sleep(8)
        guard()
        assert read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing RTC alarm'
        # Console loglevel 3 otherwise suppresses scheduler stack lines from
        # the existing ramoops console. Event-only diagnostic verbosity.
        Path('/proc/sys/kernel/printk').write_text('8\n')
        if console_debug:
            # An early device-phase wait leaves the console suspended, so
            # delayed stack prints cannot reach ramoops. Standard diagnostic
            # no_console_suspend behavior preserves that evidence. It may
            # retain the UART and is never a normal-power qualification.
            assert old_console_suspend in ('Y', 'N'), 'console parameter absent'
            console_suspend.write_text('N\n')
        for mode in ('core', 'none'):
            guard()
            directory = root / mode
            directory.mkdir(mode=0o700)
            record(mode + '-start.json', {'monotonic_s': time.monotonic()})
            with (directory / 'log').open('x') as log:
                result = subprocess.run(['python3', '-I', '/run/y2/suspend_platform_device.py',
                    str(directory), mode, '30' if mode == 'none' else '0', 'rtc'],
                    stdout=log, stderr=log)
            if result.returncode or not (directory / 'result.json').exists():
                raise RuntimeError(mode + ' did not return a receipt')
            verdict = q.verdict(json.loads((directory / 'result.json').read_text()))
            record(mode + '-verdict.json', verdict)
            if mode == 'core' and not all(v for k, v in verdict['checks'].items() if k != 'untainted'):
                raise RuntimeError('instrumented core stage failed; no real entry')
            time.sleep(2)
        record('complete.json', {'same_boot': read('/proc/sys/kernel/random/boot_id') == boot,
                                 'diagnostic_only': True})
    except Exception as error:
        record('error.json', {'type': type(error).__name__, 'error': str(error)})
        raise
    finally:
        if console_debug and old_console_suspend:
            console_suspend.write_text(old_console_suspend + '\n')
        if old_printk:
            Path('/proc/sys/kernel/printk').write_text(old_printk + '\n')
        record('finished.json', {'monotonic_s': time.monotonic()})


if __name__ == '__main__':
    main()
