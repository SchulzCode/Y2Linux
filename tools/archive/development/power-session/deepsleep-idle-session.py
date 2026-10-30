#!/usr/bin/env python3
"""Battery-only normal-policy observation; no manual C3 or suspend entry.

Run detached from /run/y2 after archiving retained evidence. Uses the installed
endpoint-only audit and stores each completed window on Y2DATA. The host must
hold the shared device lease. Existing battery protection remains authoritative.
"""
import json
import os
from pathlib import Path
import subprocess
import time


def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def main():
    os.umask(0o077)
    boot = read('/proc/sys/kernel/random/boot_id')
    root = Path('/data/logs/deepsleep-final-' + boot[:8])
    root.mkdir(mode=0o700, exist_ok=False)

    def record(name, value):
        with (root / name).open('x') as stream:
            json.dump(value, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())

    def guard():
        if read('/proc/sys/kernel/random/boot_id') != boot:
            raise RuntimeError('boot changed')
        if read('/proc/sys/kernel/tainted') != '0':
            raise RuntimeError('kernel tainted')
        if read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            raise RuntimeError('USB attached')
        if read('/sys/class/power_supply/BAT0/status') != 'Discharging':
            raise RuntimeError('not discharging')
        if int(read('/sys/class/power_supply/BAT0/voltage_now')) < 3650000:
            raise RuntimeError('observation voltage floor')

    saved = {radio: read('/data/' + folder + '/enabled')
             for radio, folder in [('wifi', 'network'), ('bluetooth', 'bluetooth')]}
    record('armed.json', {'boot': boot, 'identity': read('/etc/y2linux/versions.json'),
                         'saved_radio_intent': saved,
                         'normal_idle_policy': read('/run/y2/cpu-idle-policy.json'),
                         'plan_seconds': [60, 300, 1800], 'suspend_trials': 0})
    changed = False
    try:
        deadline = time.monotonic() + 1800
        while read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            if time.monotonic() >= deadline:
                raise RuntimeError('physical USB unplug deadline')
            time.sleep(2)
        time.sleep(8)
        guard()
        record('unplugged.json', {'monotonic': time.monotonic(),
                                 'voltage_uv': read('/sys/class/power_supply/BAT0/voltage_now')})
        changed = True
        for radio in saved:
            subprocess.run(['/usr/sbin/y2-radio', radio, 'off-runtime'], check=True)
        subprocess.run(['python3', '/run/y2/power-screen.py', 'off'], check=True)
        for seconds in (60, 300, 1800):
            guard()
            record(f'B{seconds}-begin.json', {'monotonic': time.monotonic()})
            # No timeout= here: Python's timed wait would poll during the window.
            subprocess.run(['/usr/sbin/y2-power-audit', '--seconds', str(seconds),
                            '--settle', '30', '--state', '04-battery-dark-radios-off',
                            '--output', str(root / f'B{seconds}.json')], check=True)
            guard()
            record(f'B{seconds}-complete.json', {'monotonic': time.monotonic()})
        with (root / 'dac.txt').open('x') as stream:
            subprocess.run(['sh', '/run/y2/dac-readback.sh'], stdout=stream,
                           stderr=subprocess.STDOUT, check=True)
        record('complete.json', {'same_boot': read('/proc/sys/kernel/random/boot_id') == boot,
                                'monotonic': time.monotonic()})
    except Exception as error:
        record('error.json', {'type': type(error).__name__, 'error': str(error),
                              'monotonic': time.monotonic()})
        raise
    finally:
        restoration = {}
        if changed:
            for radio, enabled in saved.items():
                answer = subprocess.run(['/usr/sbin/y2-radio', radio,
                                         'on-runtime' if enabled == '1' else 'off-runtime'])
                restoration[radio] = answer.returncode
        record('finished.json', {'radios_restored': restoration,
                                 'monotonic': time.monotonic(), 'screen_left_off': True})


if __name__ == '__main__':
    main()
