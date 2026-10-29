#!/usr/bin/env python3
"""Observe normal Reborn auto-sleep after a real maintenance owner releases it.
RETIRED for the player: its device side runs Python, and the player has none since test build 169
(docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.

No owner-qualified suspend helper or direct /sys/power/state write is used.
Requires host-reviewed clean cycle receipts on the exact current boot. The
normal product helper owns PM, its existing backstop and restoration. One RTC
alarm bounds the experiment; a failed automatic request is not manually retried.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import time


def require(condition, reason):
    if not condition:
        raise RuntimeError(reason)


def boottime():
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--boot-id', required=True)
    parser.add_argument('--reviewed-cycles', required=True, type=Path)
    parser.add_argument('--product-source-sha256', required=True,
                        help='reviewed live product_sleep.py hash; image identity stays unchanged')
    parser.add_argument('--suspended-seconds', type=int, choices=(0, 1800, 7200, 28800), default=0,
                        help='after reviewed shorter product success, require this actual SPM residence')
    parser.add_argument('--reviewed-product', type=Path,
                        help='same-boot successful prior product test; mandatory for endurance')
    args = parser.parse_args()
    os.umask(0o077)
    os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'
    sys.path.insert(0, '/usr/lib/y2-platform')
    from y2_platform.common import Context
    from y2_platform.product_sleep import guard_record, spm_state
    from y2_platform.sleep import request, wake_irqs
    from y2_platform.suspend_record import application_rpc
    ctx = Context()
    spec = importlib.util.spec_from_file_location('trial', '/run/y2/suspend_platform_device.py')
    trial = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(trial)
    versions = ctx.json('/etc/y2linux/versions.json', {})
    require(versions.get('build_git_commit') == '5a32683f1ae9493c6a37bd18af3d131afb04d450'
            and versions.get('reborn_source_commit') == '5a9f7586dd8d80ab5afc16c6a9a7e853e7469aeb'
            and ctx.read('/proc/sys/kernel/osrelease') == '6.18.0-y2linux-platform-v1-deepsleep-final-02',
            'different installed source')
    product_source = Path('/usr/lib/y2-platform/y2_platform/product_sleep.py')
    product_hash = hashlib.sha256(product_source.read_bytes()).hexdigest()
    require(product_hash == args.product_source_sha256, 'unreviewed product helper')
    require(ctx.read('/proc/sys/kernel/random/boot_id') == args.boot_id, 'different boot')
    require(ctx.read('/proc/sys/kernel/tainted') == '0', 'kernel tainted')
    require({line.split()[0] for line in Path('/proc/modules').read_text().splitlines()}
            == {'mediatek_drm'}, 'unexpected module')
    require(ctx.read('/sys/module/printk/parameters/console_suspend') == 'Y', 'normal console PM required')
    require(ctx.read('/sys/class/power_supply/y2-usb-presence/online') == '1', 'arm while USB connected')
    require(ctx.read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'), 'existing RTC alarm')
    require('[none]' in (ctx.read('/sys/power/pm_test') or ''), 'PM test owned')
    require(spm_state(ctx).get('system_sleep_abi') == '1', 'built-in context fix absent')
    backstop = trial.fields(ctx.read('/sys/firmware/y2_pm/backstop_s'))
    require(backstop.get('provider') == '1' and backstop.get('armed_s') == '0'
            and backstop.get('running') == '0', 'backstop unavailable or owned')
    prior = args.reviewed_cycles.resolve()
    require(prior.parent == Path('/data/logs') and prior.name.startswith('deepsleep-clean-'),
            'invalid reviewed evidence directory')
    armed = json.loads((prior / 'armed.json').read_text())
    complete = json.loads((prior / 'complete.json').read_text())
    require(armed.get('boot_id') == args.boot_id and armed.get('cycle_check') is True
            and armed.get('diagnostic_only') is False and complete.get('same_boot') is True,
            'clean cycle batch not complete on this boot')
    for name in ('rtc-1', 'rtc-2', 'rtc-3', 'power', 'rtc-5min'):
        require(json.loads((prior / (name + '-verdict.json')).read_text()).get('deep_resume') is True,
                'reviewed cycle did not pass: ' + name)
    if args.suspended_seconds:
        require(args.reviewed_product is not None, 'reviewed product trial required')
        product_prior = args.reviewed_product.resolve()
        require(product_prior.parent == Path('/data/logs')
                and product_prior.name.startswith('deepsleep-product-'), 'invalid product receipt path')
        previous_armed = json.loads((product_prior / 'armed.json').read_text())
        previous_complete = json.loads((product_prior / 'complete.json').read_text())
        previous_verdict = json.loads((product_prior / 'verdict.json').read_text())
        minimum_previous = {1800: 0, 7200: 1800, 28800: 7200}[args.suspended_seconds]
        require(previous_armed.get('boot_id') == args.boot_id
                and previous_armed.get('product_source_sha256') == product_hash
                and previous_complete.get('same_boot') is True
                and previous_verdict.get('pass') is True
                and previous_armed.get('target_suspended_seconds', 0) >= minimum_previous,
                'shorter same-source product qualification missing')
    alarm_seconds = args.suspended_seconds + 180 if args.suspended_seconds else 300
    fd = os.open('/data/system/platform/.maintenance.lock',
                 os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    locked = True
    alarm_owned = False
    root = Path('/data/logs/deepsleep-product-' + args.boot_id[:8] + '-' + str(int(boottime())))
    root.mkdir(mode=0o700)

    def record(name, value):
        trial.durable(root / name, value)

    def guard():
        require(ctx.read('/proc/sys/kernel/random/boot_id') == args.boot_id, 'boot changed')
        require(ctx.read('/proc/sys/kernel/tainted') == '0', 'kernel taint changed')
        require(ctx.read('/sys/class/power_supply/y2-usb-presence/online') == '0', 'USB returned')
        require(ctx.read('/sys/class/power_supply/BAT0/status') == 'Discharging', 'not discharging')
        require((ctx.integer('/sys/class/power_supply/BAT0/voltage_now') or 0) >= 3800000,
                'diagnostic voltage floor')

    def receipt():
        return ctx.json('/data/system/platform/suspend-last.json', {})

    def wait_until(deadline):
        while boottime() < deadline:
            time.sleep(max(0, min(15, deadline - boottime())))

    record('armed.json', {'boot_id': args.boot_id, 'identity': versions,
                         'product_source_sha256': product_hash,
                         'live_overlay': ctx.json('/data/system/platform/deepsleep-live-overlay.json', {}),
                         'runtime_policy': ctx.json('/data/system/platform/deepsleep-runtime-policy.json', {}),
                         'usb_budget_wakeup': ctx.read('/sys/class/power_supply/y2-usb-input/power/wakeup'),
                         'reviewed_cycles': str(prior), 'automatic_policy_test': True,
                         'maintenance_owner': os.getpid(), 'maintenance_hold_seconds': 75,
                         'rtc_alarm_seconds': alarm_seconds, 'direct_suspend_calls': 0,
                         'target_suspended_seconds': args.suspended_seconds,
                         'reviewed_product': str(args.reviewed_product) if args.reviewed_product else None})
    print(root, flush=True)
    try:
        record('before.json', trial.snapshot())
        retained = root / 'retained-pstore'
        retained.mkdir(mode=0o700)
        for source in Path('/sys/fs/pstore').iterdir():
            if source.is_file():
                with (retained / source.name).open('xb') as stream:
                    stream.write(source.read_bytes())
                    stream.flush()
                    os.fsync(stream.fileno())
        os.sync()
        guard_record(ctx, False)
        connected_refusal = request(ctx)
        record('usb-refusal.json', connected_refusal)
        require(connected_refusal.get('state') == 'refused' and
                connected_refusal.get('reason') in {'charger_active', 'charger_or_usb_active'},
                'connected product request not refused for USB/charger')
        state = application_rpc(ctx, 'status')
        require(state.get('playback', {}).get('state') in {'paused', 'stopped'}
                and not state.get('library', {}).get('scanning'), 'application busy')
        awake = ctx.command(['python3', '-I', '/run/y2/power-screen.py', 'on'], timeout=10,
                            failure_output=True)
        record('screen-on.json', awake)
        require(awake.get('ok'), 'could not establish awake starting state')
        # USB refusal has the normal Product-v2 screen-off fallback. Establish
        # darkness here, then make no input or explicit sleep request on battery.
        gesture = ctx.command(['python3', '-I', '/run/y2/power-screen.py', 'off'], timeout=10,
                              failure_output=True)
        record('screen-off.json', gesture)
        require(gesture.get('ok'), 'connected screen-off gesture failed')
        first = receipt()
        record('initial-refusal.json', first)
        record('ready.json', {'boottime': boottime(), 'waiting_for_unplug': True})
        deadline = boottime() + 1800
        while ctx.read('/sys/class/power_supply/y2-usb-presence/online') != '0':
            require(boottime() < deadline, 'unplug deadline')
            time.sleep(2)
        unplugged = boottime()
        record('unplugged.json', {'boottime': unplugged})
        time.sleep(8)
        guard()
        baseline = trial.snapshot()
        record('battery-before.json', baseline)
        alarm_owned = True
        alarm = ctx.command(['/usr/sbin/y2-platform', 'rtc-alarm', str(alarm_seconds)], timeout=10)
        record('alarm.json', alarm)
        require(alarm.get('ok'), 'RTC alarm did not program')
        # The held maintenance flock must now refuse the normal automatic
        # request after the screen has been dark for the product timeout.
        held_since = boottime()
        wait_until(held_since + 75)
        guard()
        later = receipt()
        record('automatic-refusal.json', {'boottime': boottime(), 'receipt': later,
                                         'spm': spm_state(ctx)})
        require(((later.get('product') or {}).get('reason') or '').startswith('maintenance')
                and later.get('started', 0) > first.get('started', 0) + 30,
                'no subsequent automatic refusal while maintenance owned')
        original_spm = trial.fields(baseline['files'].get(
            '/sys/devices/platform/10006000.power-controller/state'))
        require(spm_state(ctx).get('entries') == original_spm.get('entries'),
                'suspend entered while maintenance was held')
        record('owner-release.json', {'boottime': boottime(), 'receipt_started': later['started']})
        fcntl.flock(fd, fcntl.LOCK_UN)
        locked = False
        deadline = unplugged + alarm_seconds + 50
        while True:
            require(boottime() < deadline, 'normal product did not complete bounded RTC return')
            time.sleep(10)
            current = receipt()
            if current.get('mode') == 'product' and current.get('started', 0) > later['started']:
                state = (current.get('product') or {}).get('state')
                if state in {'restored', 'refused', 'restore_failed'}:
                    record('observed-terminal.json', current)
                    require(state == 'restored', 'normal product terminal state: ' + state + '/' +
                            str((current.get('product') or {}).get('reason')))
                    break
        record('product-receipt.json', current)
        # Wait for the product helper to release its operation lock before
        # re-establishing host-review quarantine; never race its final guard.
        deadline = boottime() + 10
        while not locked:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                locked = True
            except BlockingIOError:
                require(boottime() < deadline, 'product operation lock not released')
                time.sleep(.1)
        guard_record(ctx, True)
        guard()
        after = trial.snapshot()
        record('after.json', after)
        record('application-after.json', application_rpc(ctx, 'status'))
        final_spm = spm_state(ctx)
        product = current.get('product') or {}
        cycles = current.get('cycles', 0)
        # Kernel-return receipts can precede the safety daemon's next sample.
        # Preserve that raw receipt, but qualify SOC from a separately timed
        # post-return sample instead of treating the old percentage as current.
        power_after = json.loads(after['files']['/run/y2/power.json'])
        sampled_ns = power_after.get('monotonic_ns')
        sampled_at = sampled_ns / 1e9 if type(sampled_ns) is int else None
        sample_age = after['monotonic_s'] - sampled_at if sampled_at is not None else None
        battery_after = {'boottime_s': after['boottime_s'],
                         'monotonic_s': after['monotonic_s'],
                         'sample_age_seconds': sample_age, 'power': power_after,
                         'load_state': 'after terminal resume; application restored'}
        record('battery-after.json', battery_after)
        before_irq = wake_irqs(baseline['files'].get('/proc/interrupts'))
        after_irq = wake_irqs(after['files'].get('/proc/interrupts'))
        checks = {
            'same_boot': product.get('same_boot') is True,
            'kernel_completed': product.get('kernel_completed') is True,
            'rtc_wake': product.get('wake_reason') == 'rtc',
            'entry_count_matches': cycles >= 1 and int(final_spm['entries']) == int(original_spm['entries']) + cycles,
            'resume_count_matches': int(final_spm['resumes']) == int(original_spm['resumes']) + cycles,
            'only_expected_maintenance': current.get('maintenance_wakes', 0) == cycles - 1,
            'one_rtc_irq': after_irq.get('rtc') == before_irq.get('rtc', -2) + 1,
            'no_power_irq': after_irq.get('power') == before_irq.get('power'),
            'alarm_auto_cleared': ctx.read('/sys/class/rtc/rtc0/wakealarm') in ('', '0'),
            'reborn_restored': current.get('reborn_restore') is True,
            'fresh_post_resume_battery': (power_after.get('boot_id') == args.boot_id
                and sampled_at is not None and 0 <= sample_age <= 10
                and sampled_at > current['resume']['monotonic_s']),
            'untainted': ctx.read('/proc/sys/kernel/tainted') == '0',
            'spm_clean': final_spm.get('broken') == '0' and final_spm.get('debug') in {'0', '0x0'},
            'no_new_restore_failure': final_spm.get('suspend_restore_failures')
                == original_spm.get('suspend_restore_failures') == '0',
        }
        if args.suspended_seconds:
            checks['minimum_actual_residence'] = current.get('suspended_seconds', 0) >= args.suspended_seconds
            checks['maintenance_returns_tested'] = current.get('maintenance_wakes', 0) >= args.suspended_seconds // 600
            checks['first_endpoint_preserved'] = bool(current.get('first_entry'))
            # Keep the normal event journal as independent evidence of dark,
            # radio-off maintenance returns and the terminal RTC wake.
            events = []
            for path in (Path('/data/system/platform/power-journal.previous.jsonl'),
                         Path('/data/system/platform/power-journal.jsonl')):
                if not path.is_file():
                    continue
                for line in path.read_text().splitlines():
                    try:
                        value = json.loads(line)
                    except ValueError:
                        continue
                    if value.get('boot_id') == args.boot_id and value.get('boottime_s', 0) >= unplugged:
                        events.append(value)
            record('power-events.json', events)
            maintenance = [v for v in events if v.get('event') == 'suspend_maintenance_wake']
            checks['maintenance_evidence_complete'] = len(maintenance) == cycles - 1
            checks['maintenance_display_dark'] = all(v.get('backlight') and
                all(state == '4' for state in v['backlight'].values()) for v in maintenance)
            checks['maintenance_radios_suspended'] = all(v.get('idle_counters', {}).get(
                'runtime_pm', {}).get('18070000.connectivity') == 'suspended' for v in maintenance)
        for path, value in baseline['files'].items():
            if path.startswith('/sys/fs/ext4/') and path.endswith('/errors_count'):
                checks[path] = value == after['files'].get(path) == '0'
        record('verdict.json', {'pass': all(checks.values()), 'checks': checks})
        require(all(checks.values()), 'product return failed checks')
        record('complete.json', {'boottime': boottime(), 'same_boot': True,
                                 'automatic_request_proven': True, 'quarantined': True,
                                 'suspended_seconds': current.get('suspended_seconds'),
                                 'cycles': cycles, 'maintenance_wakes': current.get('maintenance_wakes', 0),
                                 'first_entry': current.get('first_entry'), 'resume': current.get('resume'),
                                 'battery_after': battery_after,
                                 'unexpected_wakes': 0})
    except Exception as error:
        record('error.json', {'boottime': boottime(), 'type': type(error).__name__, 'error': str(error)})
        record('failure-receipt.json', receipt())
        record('failure-snapshot.json', trial.snapshot())
        raise
    finally:
        # Exclude a new automatic request immediately, then take ownership
        # before renewing the guard. A completing helper can clear its own
        # pending flag; never race that final write or remove its RTC fallback.
        guard_record(ctx, True)
        deadline = time.monotonic() + 30
        while not locked:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                locked = True
            except BlockingIOError:
                require(time.monotonic() < deadline, 'cleanup could not acquire maintenance ownership')
                time.sleep(.1)
        if alarm_owned:
            record('alarm-cleanup.json', ctx.command(['/usr/sbin/y2-platform', 'rtc-alarm', '0'], timeout=10))
        guard_record(ctx, True)
        record('finished.json', {'boottime': boottime(), 'quarantined': True})
        os.close(fd)


if __name__ == '__main__':
    main()
