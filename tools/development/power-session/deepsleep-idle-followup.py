#!/usr/bin/env python3
"""One endpoint-only runtime window on the already documented diagnostic boot.
RETIRED for the player: its device side runs Python, and the player has none since test build 169
(docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.

No sleep request, CPU-policy change, synthetic input or periodic observation.
The matching context probe is idle outside system PM; its taint stays explicit.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.path.insert(0, '/usr/lib/y2-platform')
from y2_platform.common import Context
from y2_platform.suspend_record import application_rpc


def read(path):
    return Path(path).read_text().strip()


def main():
    os.umask(0o077)
    boot = read('/proc/sys/kernel/random/boot_id')
    root = Path('/data/logs/deepsleep-idle30-' + boot[:8] + '-' + str(int(time.monotonic())))
    root.mkdir(mode=0o700)

    def record(name, value):
        with (root / name).open('x') as stream:
            json.dump(value, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())

    def guard():
        assert read('/proc/sys/kernel/random/boot_id') == boot
        assert read('/proc/sys/kernel/tainted') == '4096'
        assert read('/sys/module/y2_pm_stack_probe/parameters/suppress_nested') == 'Y'
        assert read('/sys/module/printk/parameters/console_suspend') == 'Y'
        assert read('/sys/class/power_supply/y2-usb-presence/online') == '0'
        assert read('/sys/class/power_supply/BAT0/status') == 'Discharging'
        assert int(read('/sys/class/power_supply/BAT0/voltage_now')) >= 3650000
        value = application_rpc(Context(), 'status')
        assert value.get('screen_off') is True
        assert value.get('playback', {}).get('state') in ('paused', 'stopped')
        assert value.get('library', {}).get('scanning') is False

    saved = {radio: read('/data/' + directory + '/enabled')
             for radio, directory in [('wifi', 'network'), ('bluetooth', 'bluetooth')]}
    record('armed.json', {'boot': boot, 'taint': read('/proc/sys/kernel/tainted'),
                         'diagnostic_modules': read('/proc/modules'),
                         'settle_seconds': 120, 'window_seconds': 1800,
                         'saved_radio_intent': saved, 'system_suspend_requested': False})
    print(str(root), flush=True)
    changed = False
    try:
        deadline = time.monotonic() + 1800
        while read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            if time.monotonic() >= deadline:
                raise RuntimeError('physical USB unplug deadline')
            time.sleep(2)
        time.sleep(8)
        guard()
        changed = True
        for radio in saved:
            subprocess.run(['/usr/sbin/y2-radio', radio, 'off-runtime'], check=True)
        record('unplugged.json', {'boottime': time.clock_gettime(time.CLOCK_BOOTTIME),
                                 'monotonic': time.monotonic()})
        # subprocess.wait without a timeout blocks in waitpid; no observer polling.
        subprocess.run(['/run/y2/deepsleep-power-audit', '--seconds', '1800',
                        '--settle', '120', '--state', '04-context-diagnostic-runtime-idle',
                        '--output', str(root / 'B1800.json')], check=True)
        guard()
        record('complete.json', {'same_boot': True, 'monotonic': time.monotonic()})
    except Exception as error:
        record('error.json', {'error': str(error), 'type': type(error).__name__,
                             'monotonic': time.monotonic()})
        raise
    finally:
        restored = {}
        if changed:
            for radio, enabled in saved.items():
                restored[radio] = subprocess.run(['/usr/sbin/y2-radio', radio,
                    'on-runtime' if enabled == '1' else 'off-runtime']).returncode
        record('finished.json', {'monotonic': time.monotonic(), 'radios_restored': restored})


if __name__ == '__main__':
    main()
