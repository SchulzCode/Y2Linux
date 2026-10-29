#!/usr/bin/env python3
"""Resident clean-image PM stages and bounded, source-matched real trials.
RETIRED for the player: its device side runs Python, and the player has none since test build 169
(docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.

The host owns the device lease and supplies the freshly verified boot ID.
A live maintenance lock excludes product auto-entry during this diagnostic;
the explicit owner helper retains its own suspend/activity locks. A durable
product guard quarantines automatic entry until the host reviews the result.
Use BOOTTIME for the owner gesture schedule; never retry a failed entry.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time


EXPECTED = {
    'build_id': 'Y2LINUX-PLATFORM-V1-DEEPSLEEP-FINAL-02',
    'build_git_commit': '5a32683f1ae9493c6a37bd18af3d131afb04d450',
    'reborn_source_commit': '5a9f7586dd8d80ab5afc16c6a9a7e853e7469aeb',
    'kernel_version': '6.18.0-y2linux-platform-v1-deepsleep-final-02',
}


def boottime():
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def require(value, reason):
    if not value:
        raise RuntimeError(reason)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--boot-id', required=True)
    parser.add_argument('--stages', action='store_true')
    parser.add_argument('--wake', choices=('power', 'rtc'), required=True)
    parser.add_argument('--alarm', type=int, required=True)
    parser.add_argument('--entry-after', type=int, required=True)
    parser.add_argument('--cycle-check', action='store_true',
                        help='after a reviewed clean return: three RTC cycles, scheduled Power, RTC five minutes')
    parser.add_argument('--stack-observer', action='store_true',
                        help='explicit diagnostic-only run with the exact matching stack observer')
    args = parser.parse_args()
    require(30 <= args.alarm <= 600, 'existing RTC bound')
    require(15 <= args.entry_after <= 300, 'bounded gesture schedule')
    require(not args.stages or args.entry_after >= 180, 'stage schedule too short')
    require(args.wake != 'power' or args.alarm >= 300, 'Power needs generous RTC fallback')
    require(not args.cycle_check or (not args.stages and not args.stack_observer
            and args.wake == 'power' and args.alarm == 300 and args.entry_after >= 180),
            'cycle check requires clean Power/RTC300 and at least 180 seconds preparation')
    # label, kernel test mode, RTC fallback, expected wake, scheduled offset,
    # minimum actual SPM residence. A failed trial is never retried.
    if args.cycle_check:
        plan = [(f'rtc-{n}', 'none', 30, 'rtc', 0, 0) for n in range(1, 4)]
        plan += [('power', 'none', 300, 'power', args.entry_after, 0),
                 ('rtc-5min', 'none', 310, 'rtc', 0, 300)]
    else:
        modes = ['freezer', 'devices', 'platform', 'processors', 'core'] if args.stages else []
        plan = [(mode, mode, 0, args.wake, 0, 0) for mode in modes]
        plan += [('none', 'none', args.alarm, args.wake, args.entry_after, 0)]
    os.umask(0o077)
    os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'
    sys.path.insert(0, '/usr/lib/y2-platform')
    from y2_platform.common import Context
    from y2_platform.product_sleep import guard_record, spm_state
    ctx = Context()
    versions = ctx.json('/etc/y2linux/versions.json', {})
    require(all(versions.get(k) == v for k, v in EXPECTED.items()), 'different installed source')
    require(ctx.read('/proc/sys/kernel/osrelease') == EXPECTED['kernel_version'], 'different kernel')
    require(ctx.read('/proc/sys/kernel/random/boot_id') == args.boot_id, 'different boot')
    expected_taint = '4096' if args.stack_observer else '0'
    require(ctx.read('/proc/sys/kernel/tainted') == expected_taint, 'unexpected kernel taint')
    if args.stack_observer:
        require(Path('/sys/module/y2_pm_device_stack').is_dir(), 'stack observer absent')
        require(hashlib.sha256(Path('/run/y2/y2-pm-device-stack.ko').read_bytes()).hexdigest() ==
                '97371e07df3c24e31b0a2cf4b4b999e0cca12ffa658393020ae9e8b0f1e5c0bd',
                'different diagnostic observer')
        require({line.split()[0] for line in Path('/proc/modules').read_text().splitlines()} ==
                {'y2_pm_device_stack', 'mediatek_drm'}, 'unexpected diagnostic module')
    require(spm_state(ctx).get('system_sleep_abi') == '1', 'built-in context fix absent')
    require(ctx.read('/sys/module/printk/parameters/console_suspend') == 'Y', 'normal console PM required')
    require(ctx.read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing RTC alarm')
    require('[none]' in (ctx.read('/sys/power/pm_test') or ''), 'PM test already owned')
    maintenance = os.open('/data/system/platform/.maintenance.lock',
                          os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    fcntl.flock(maintenance, fcntl.LOCK_EX | fcntl.LOCK_NB)
    kind = 'observed' if args.stack_observer else 'clean'
    root = Path('/data/logs/deepsleep-' + kind + '-' + args.boot_id[:8] + '-' + str(int(time.monotonic())))
    root.mkdir(mode=0o700)
    spec = importlib.util.spec_from_file_location('q', '/run/y2/qualify-platform-v1-suspend.py')
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)
    backstop = q.fields(ctx.read('/sys/firmware/y2_pm/backstop_s'))
    require(backstop.get('provider') == '1' and backstop.get('armed_s') == '0'
            and backstop.get('running') == '0', 'backstop unavailable or already owned')

    def record(name, value):
        with (root / name).open('x') as stream:
            json.dump(value, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)

    def guard():
        require(ctx.read('/proc/sys/kernel/random/boot_id') == args.boot_id, 'boot changed')
        require(ctx.read('/proc/sys/kernel/tainted') == expected_taint, 'kernel taint changed')
        require(ctx.read('/sys/class/power_supply/y2-usb-presence/online') == '0', 'USB present')
        require(ctx.read('/sys/class/power_supply/BAT0/status') == 'Discharging', 'not discharging')
        require((ctx.integer('/sys/class/power_supply/BAT0/voltage_now') or 0) >= 3800000,
                'diagnostic voltage floor')
        require(ctx.read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing alarm')
        require(spm_state(ctx).get('broken') == '0', 'SPM quarantined')

    def preserve(label):
        paths = ['/proc/interrupts', '/sys/kernel/debug/wakeup_sources',
                 '/run/y2/power.json', '/proc/uptime', '/proc/sys/kernel/random/boot_id']
        paths += [str(p) for p in Path('/sys/firmware/y2_pm').iterdir() if p.is_file()]
        record(label + '-state.json', {'boottime': boottime(), 'monotonic': time.monotonic(),
                                      'files': {p: ctx.read(p) for p in paths}})
        with (root / (label + '.tar')).open('xb') as stream:
            with tarfile.open(fileobj=stream, mode='w') as archive:
                for source in [Path('/data/system/platform'), Path('/data/reborn/logs'),
                               *Path('/data/logs').glob('system.log*')]:
                    if source.exists():
                        archive.add(source, arcname=str(source).lstrip('/'))
                # sysfs size is not the readable file length; preserve actual bytes.
                for source in sorted(Path('/sys/fs/pstore').glob('*')):
                    if source.is_file():
                        data = source.read_bytes()
                        info = tarfile.TarInfo(str(source).lstrip('/'))
                        info.size, info.mode = len(data), 0o600
                        archive.addfile(info, io.BytesIO(data))
            stream.flush()
            os.fsync(stream.fileno())
        with (root / (label + '-dmesg.txt')).open('x') as stream:
            subprocess.run(['dmesg'], stdout=stream, check=True)
            stream.flush()
            os.fsync(stream.fileno())
        os.sync()

    record('armed.json', {'boot_id': args.boot_id, 'identity': EXPECTED, 'stages': args.stages,
                         'diagnostic_only': args.stack_observer, 'expected_taint': int(expected_taint),
                         'wake': args.wake, 'alarm': args.alarm, 'entry_after': args.entry_after,
                         'cycle_check': args.cycle_check, 'plan': plan,
                         'schedule_clock': 'CLOCK_BOOTTIME', 'automatic_retry': False,
                         'product_quarantine_until_host_review': True,
                         'maintenance_lock_pid': os.getpid()})
    print(root, flush=True)
    guard_record(ctx, True)
    try:
        deadline = boottime() + 1800
        while ctx.read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            require(boottime() < deadline, 'unplug deadline')
            time.sleep(2)
        unplugged = boottime()
        record('unplugged.json', {'boottime': unplugged, 'entry_target': unplugged + args.entry_after})
        time.sleep(8)
        guard()
        preserve('before-stages')
        for label, mode, alarm, wake, scheduled, minimum_residence in plan:
            guard()
            # A reset during this exact attempt must not start an automatic loop.
            guard_record(ctx, True)
            if mode == 'none':
                if scheduled:
                    require(boottime() < unplugged + scheduled - 10, 'missed real-entry schedule')
                preserve('before-' + label)
                if scheduled:
                    require(boottime() < unplugged + scheduled - 2, 'archive exceeded schedule')
                    time.sleep(max(0, unplugged + scheduled - boottime()))
                guard()
            directory = root / label
            directory.mkdir(mode=0o700)
            record(label + '-start.json', {'boottime': boottime(), 'monotonic': time.monotonic()})
            with (directory / 'log').open('x') as stream:
                result = subprocess.run(['python3', '-I', '/run/y2/suspend_platform_device.py',
                                         str(directory), mode, str(alarm), wake], stdout=stream, stderr=stream)
            require(result.returncode == 0 and (directory / 'result.json').is_file(), 'missing return receipt')
            value = json.loads((directory / 'result.json').read_text())
            verdict = q.verdict(value)
            if minimum_residence:
                state = q.fields(value['after']['files'].get(
                    '/sys/devices/platform/10006000.power-controller/state'))
                ticks = int(state.get('ticks32k', '0'), 0)
                verdict['checks']['minimum_spm_residence'] = ticks >= minimum_residence * 32768
                verdict['pass'] = all(verdict['checks'].values())
                verdict['deep_resume'] = verdict['pass']
            if args.stack_observer:
                verdict['diagnostic_checks_pass'] = all(
                    passed for check, passed in verdict['checks'].items() if check != 'untainted')
            record(label + '-verdict.json', verdict)
            require(verdict.get('diagnostic_checks_pass', verdict['pass']),
                    'failed ' + label + ' verdict; no retry')
            time.sleep(2)
        record('complete.json', {'same_boot': ctx.read('/proc/sys/kernel/random/boot_id') == args.boot_id,
                                'diagnostic_only': args.stack_observer,
                                'boottime': boottime(), 'review_required': True})
    except Exception as error:
        record('error.json', {'type': type(error).__name__, 'error': str(error), 'boottime': boottime()})
        raise
    finally:
        # The host clears this only after reviewing the actual clean return.
        guard_record(ctx, True)
        record('finished.json', {'boottime': boottime(), 'quarantined': True})
        os.close(maintenance)


if __name__ == '__main__':
    main()
