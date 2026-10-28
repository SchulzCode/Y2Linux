"""Owned product suspend; no polling while asleep and no editable bypass on 04.

The established PCM wakes every 30 minutes for a battery safety check. Only
an unambiguous timer return can re-enter without restoring the screen/radios.
All other wakes restore the application. Critical operations use real flock
ownership and bounded kernel leases, never the mere existence of a PID file.
"""
# SPDX-License-Identifier: GPL-2.0-only
import contextlib
import errno
import fcntl
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

from .boot import private_directory
from .common import Context, atomic_json, kernel_trusted

GUARD = '/data/system/platform/product-sleep-guard.json'
# A kernel refusal before any task froze leaves nothing unproven; the next
# request waits this long instead of the image being quarantined.
RETRY = '/run/y2/product-sleep-retry.json'
PREPARE_RETRY_SECONDS = 120
# Stage marks of a kernel attempt that ended before freezing tasks: only the
# PM_SUSPEND_PREPARE notifiers and the filesystem sync ran, no device callback.
PRE_FREEZE_MARKS = frozenset(('HELPER_REQUEST', 'SUSPEND_REQUEST', 'BACKSTOP_STARTED',
                              'FILESYSTEM_SYNCED', 'EXIT'))
# Report 4fed1b6a (8.12): a sleep entered 8 s after Reborn started, before
# `application_ready` armed the power features, hung right after resume. A
# sleep requested that early now waits (screen already off) for the stage,
# at most this long, then goes on as before.
APPLICATION_READY_WAIT_S = 60
OPERATION_LOCKS = (
    ('activity_operation', '/run/y2/activity.lock'),
    ('firmware_update', '/data/updates/.stage.lock'),
    ('maintenance', '/data/system/platform/.maintenance.lock'),
)


class Refused(ValueError):
    pass


def spm_state(ctx):
    paths = ctx.glob('/sys/bus/platform/devices/10006000*/state')
    if len(paths) != 1:
        return {}
    return dict(re.findall(r'(\w+)=(\S+)', ctx.read(paths[0].relative_to(ctx.root)) or ''))


def identity(ctx):
    versions = ctx.json('/etc/y2linux/versions.json', {})
    return {key: versions.get(key) for key in ('build_id', 'build_git_commit', 'reborn_source_commit')} | {
        'kernel': ctx.read('/proc/sys/kernel/osrelease')}


def admission(ctx):
    if ctx.read('/sys/class/power_supply/BAT0/status') == 'Charging':
        raise Refused('charger_active')
    spm = spm_state(ctx)
    if spm.get('system_sleep_abi') != '1':
        raise Refused('physical_qualification_required')
    if not kernel_trusted(ctx) or spm.get('broken') != '0':
        raise Refused('kernel_sleep_unavailable')
    if 'mem' not in (ctx.read('/sys/power/state') or '').split():
        raise Refused('kernel_sleep_unavailable')
    if '[none]' not in (ctx.read('/sys/power/pm_test') or ''):
        raise Refused('pm_test_in_progress')
    backstop = dict(re.findall(r'(\w+)=(\S+)', ctx.read('/sys/firmware/y2_pm/backstop_s') or ''))
    if backstop.get('provider') != '1':
        raise Refused('kernel_sleep_unavailable')
    if backstop.get('armed_s') != '0' or backstop.get('running') != '0':
        raise Refused('pm_test_in_progress')
    retry = ctx.json(RETRY, {})
    if (retry.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id')
            and type(retry.get('not_before_ns')) is int
            and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < retry['not_before_ns']):
        raise Refused('kernel_prepare_retry_wait')
    prior = ctx.json(GUARD, {})
    # Only an attempt that reached the kernel can leave an unproven sleep. A
    # pending record written before kernel entry (radio quiesce, Reborn) is a
    # helper failure, not a kernel quarantine; 12 kept those across reboots.
    if (prior.get('identity') == identity(ctx) and prior.get('pending')
            and prior.get('kernel_entry', True)):
        # A clean, explicit owner trial on this image can recover a quarantine.
        receipt = ctx.json('/data/system/platform/suspend-last.json', {})
        p = receipt.get('product') or {}
        recovered = (receipt.get('mode') != 'product' and p.get('state') == 'restored'
                     and p.get('kernel_completed') is True and p.get('same_boot') is True
                     and receipt.get('current_boot_id') == ctx.read('/proc/sys/kernel/random/boot_id')
                     and receipt.get('updated', 0) > prior.get('updated', 0))
        if not recovered:
            raise Refused('sleep_recovery_required')
        guard_record(ctx, False)


def usb_host_configured(ctx):
    """True while any host has configured the gadget (SSH/MTP in use)."""
    return any(ctx.read(path.relative_to(ctx.root)) == 'configured'
               for path in ctx.glob('/sys/class/udc/*/state'))


def battery_and_cable(ctx):
    from .power import load_policy
    status = ctx.read('/sys/class/power_supply/BAT0/status')
    if ctx.read('/sys/class/power_supply/y2-usb-presence/online') != '0':
        # POWER-FINAL-16 Defect C (archive 109): VBUS from a powered-off or
        # sleeping PC with nothing charging must not hold the Y2 awake. Sleep
        # while the cable is in only when no host uses the gadget and the
        # charger is idle (Full, or inhibited/paused).
        if status not in ('Full', 'Not charging') or usb_host_configured(ctx):
            raise Refused('charger_or_usb_active')
    elif status != 'Discharging':
        raise Refused('battery_state_unavailable')
    policy, error = load_policy(ctx)
    voltage = ctx.integer('/sys/class/power_supply/BAT0/voltage_now')
    state = ctx.json('/run/y2/power.json', {})
    sampled = state.get('monotonic_ns')
    age = time.monotonic_ns() - sampled if type(sampled) is int else -1
    # F55: Low (20 %) and Critical (10 %) may sleep; asleep drains ~8 mA, awake
    # and dark far more. Every sleep returns within the 30-min SPM maintenance
    # timer and admits again here, so a sleeping player still reaches the
    # low-battery power-off (shutdown_soc / critical_uv) awake.
    if (state.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id') or
            not 0 <= age <= 10 * 10**9 or state.get('state') not in ('Normal', 'Low', 'Critical')):
        raise Refused('battery_safety_owner')
    # F55 (189640af): one loaded sample at 3.56-3.58 V (37 %) refused every
    # sleep below recover_uv. Judge the filtered voltage and refuse only where
    # the low-battery owner is about to power off (power.LowBattery).
    battery = state.get('battery') if isinstance(state.get('battery'), dict) else {}
    filtered = battery.get('filtered_voltage_uv')
    level = filtered if type(filtered) is int and filtered > 0 else voltage
    percent = battery.get('percent')
    empty = (type(percent) is int and policy is not None and policy.get('schema') == 2 and
             battery.get('confidence') in ('hardware', 'calibrated_estimate') and
             percent <= policy['shutdown_soc'])
    if error or voltage is None or level is None or level <= policy['critical_uv'] or empty:
        raise Refused('battery_reserve')
    shutdown = ctx.json('/run/y2/shutdown.json', {})
    if shutdown.get('state') in ('ShutdownPending', 'Executing', 'Failed'):
        raise Refused('shutdown_in_progress')


def operation_state(ctx, workloads=True):
    from .update import status, TERMINAL
    if status(ctx).get('state') not in TERMINAL:
        raise Refused('firmware_update')
    if ctx.path('/data/system/platform/maintenance-pending').exists():
        raise Refused('maintenance_recovery')
    if not workloads:
        return
    leases = ctx.read('/sys/module/workload/parameters/leases')
    if leases is None:
        raise Refused('workload_owner_unavailable')
    for line in leases.splitlines():
        match = re.fullmatch(r'(\w+) remaining_ms=(\d+) min_khz=-?\d+', line)
        if not match:
            raise Refused('workload_owner_unavailable')
        if match[1] != 'Idle' and int(match[2]) > 0:
            raise Refused('workload_' + match[1])


@contextlib.contextmanager
def operation_locks(ctx):
    held = {}
    with contextlib.ExitStack() as stack:
        for owner, name in OPERATION_LOCKS:
            path = ctx.path(name)
            private_directory(path.parent)
            fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
            stack.callback(os.close, fd)
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                holders = lock_holders(ctx, os.fstat(fd))
                raise Refused(owner + (':' + ','.join(holders) if holders else '')) from error
            held[owner] = fd
        yield held


def release_activity(held):
    """Let playback start once the player is awake for good (F24)."""
    fd = held.get('activity_operation')
    if fd is not None:
        fcntl.flock(fd, fcntl.LOCK_UN)


def lock_holders(ctx, meta):
    """Name actual live flock holders; never infer ownership from old files."""
    holders = []
    for line in (ctx.read('/proc/locks') or '').splitlines():
        fields = line.split()
        if len(fields) < 6 or fields[1] != 'FLOCK' or not fields[4].isdecimal():
            continue
        try:
            major, minor, inode = fields[5].split(':')
            matches = (int(major, 16), int(minor, 16), int(inode)) == (
                os.major(meta.st_dev), os.minor(meta.st_dev), meta.st_ino)
        except ValueError:
            continue
        if matches:
            pid = fields[4]
            name = re.sub(r'[^\w.-]', '_', ctx.read('/proc/' + pid + '/comm') or 'unknown')[:32]
            holders.append(pid + '/' + name)
    return holders[:8]


def launch(ctx, fd):
    if ctx.root != Path('/'):
        raise Refused('fixture_launcher_unavailable')
    # The inherited open-file description retains the request's flock across
    # parent exit, so another request cannot pass between acknowledgement/entry.
    subprocess.Popen(['/usr/bin/python3', '-I', '/usr/lib/y2-platform/product-sleep.py', str(fd)],
                     pass_fds=(fd,), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, start_new_session=True,
                     env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'})


def guard_record(ctx, pending, kernel_entry=False):
    private_directory(ctx.path(GUARD).parent)
    atomic_json(ctx.path(GUARD), {'identity': identity(ctx), 'pending': pending,
                'kernel_entry': kernel_entry,
                'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                'updated': time.time()}, durable=True)


def failed_before_freeze(ctx):
    """True when the kernel returned an error before freezing any task.

    A PM_SUSPEND_PREPARE notifier refusal (c43e8711 report: system-idle
    bringing a parked CPU back after hotplug was disabled, -EPERM) returns
    from the write before a single device callback ran. The current cycle's
    ring then holds only these marks, ending in a failed EXIT. Anything else,
    including a wrapped or unreadable ring, keeps the quarantine.
    """
    lines = (ctx.read('/sys/firmware/y2_pm/devices', limit=16384) or '').splitlines()
    if not lines or not lines[0].startswith('ring=valid '):
        return False
    marks = []
    for line in lines[1:]:
        match = re.fullmatch(r'\d+ phase=\d+ mark result=(-?\d+) ms=\d+ stage=(\w+)', line)
        if not match:
            return False
        marks.append((match[2], int(match[1])))
    kernel = dict(re.findall(r'(\w+)=(\S+)', ctx.read('/sys/firmware/y2_pm/state') or ''))
    return (bool(marks) and marks[-1][0] == 'EXIT' and marks[-1][1] < 0
            and ('SUSPEND_REQUEST', 0) in marks
            and all(stage in PRE_FREEZE_MARKS for stage, _ in marks)
            and kernel.get('failed_stage') == 'EXIT')


def retry_later(ctx):
    private_directory(ctx.path(RETRY).parent)
    atomic_json(ctx.path(RETRY), {'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                'not_before_ns': time.clock_gettime_ns(time.CLOCK_BOOTTIME)
                + PREPARE_RETRY_SECONDS * 10**9})


def command(ctx, argv):
    result = ctx.command(argv, timeout=20)
    if not result.get('ok'):
        raise RuntimeError('sleep_service_failed:' + Path(argv[0]).name)


RADIO_QUIESCE_SECONDS = 25
# Fresh wakeup_count handshakes after an event that was neither Power, RTC nor
# a cable change, before the entry is refused as a wake in progress.
STALE_COUNT_RETRIES = 2


def quiesce_radio(ctx, radio):
    """Runtime-off one radio, waiting out a platform Bluetooth operation.

    After a Power wake the reconnect owner holds bt-operation.lock while it
    reconnects the headset, and `bluetooth-power off` refuses at once. Retry
    within a bounded window instead of failing the whole sleep request.
    """
    argv = ['/usr/sbin/y2-radio', radio, 'off-runtime']
    deadline = time.monotonic() + RADIO_QUIESCE_SECONDS
    attempts = 0
    while True:
        attempts += 1
        result = ctx.command(argv, timeout=20)
        if result.get('ok'):
            return attempts
        if time.monotonic() >= deadline:
            raise RuntimeError('sleep_service_failed:y2-radio:%s:attempts=%d' % (radio, attempts))
        time.sleep(1)


def application_starting(ctx):
    """True only while this boot's journal says the UI has not reached
    `application_ready` yet; unknown or another boot is not "starting"."""
    journal = ctx.json('/data/system/platform/boot.json', {})
    return (isinstance(journal, dict) and journal.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id')
            and journal.get('last_stage') in ('platform_start', 'services_started'))


def wait_for_application(ctx, previous_irqs, wait_s=APPLICATION_READY_WAIT_S):
    """Defer kernel entry until `application_ready`, bounded. A Power or RTC
    wake during the wait cancels the sleep like any wake before entry."""
    from .sleep import wake_irqs
    from .suspend_record import record
    if not application_starting(ctx):
        return
    record(ctx, 'waiting_application_ready', 0)
    deadline = time.monotonic() + wait_s
    while application_starting(ctx) and time.monotonic() < deadline:
        time.sleep(.25)
        if previous_irqs is not None and wake_irqs(ctx.read('/proc/interrupts')) != previous_irqs:
            raise Refused('wake_event_in_progress')


def screen_first(ctx, dark=False):
    """Ask Reborn to restore before the radios are; `dark` keeps the screen
    off (a refused sleep nobody woke, F47)."""
    from .suspend_record import application_rpc
    try:
        return application_rpc(ctx, 'sleep_restore', **({'dark': True} if dark else {}))
    except (OSError, ValueError):
        return None


def progress(ctx, stage):
    """One durable row in the sleep receipt without the sysfs reads of a full
    record (a stalled driver could block those too). Never raises."""
    from .suspend_record import progress as row
    with contextlib.suppress(OSError, ValueError, TypeError):
        row(ctx, stage)


def marker(ctx, stage):
    control_write(ctx, '/sys/firmware/y2_pm/stage', stage + '\n')


def control_write(ctx, name, value):
    """Issue one sysfs command; buffered close can replay a rejected write.

    TextIOWrapper/BufferedWriter retry a failed flush during close. For PM
    that can suspend after EBUSY, yet still report the original refusal to
    the caller. FileIO has no pending buffer to retry on close. A short write
    is also a failure, never a reason to issue the remaining command again.
    """
    data = value.encode('ascii')
    with ctx.path(name).open('wb', buffering=0) as stream:
        if stream.write(data) != len(data):
            raise OSError(errno.EIO, 'short PM control write: ' + name)


def timer_only(value, spm):
    p = value.get('product') or {}
    before, after = value.get('wake_irqs_before', {}), value.get('wake_irqs_after', {})
    return (p.get('kernel_completed') is True and p.get('same_boot') is True
            and spm.get('timer_wake') == '1'
            and all(name in before and after.get(name) == before[name] for name in ('power', 'rtc')))


def kernel_sleep(ctx):
    control_write(ctx, '/sys/power/state', 'mem\n')


def wakeup_count(ctx, timeout=2):
    # sysfs wakeup_count_show calls pm_get_wakeup_count(..., true), so even
    # O_NONBLOCK cannot bound an active wake source. Interrupt that read rather
    # than leaving the product helper stuck before it reaches kernel PM.
    if signal.getitimer(signal.ITIMER_REAL)[0]:
        raise Refused('sleep_deadline_in_progress')
    def expired(signum, frame):
        raise Refused('wakeup_count_busy')
    old = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, timeout)
    try:
        return ctx.read('/sys/power/wakeup_count')
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, old)


def execute(ctx, fd, enter=kernel_sleep):
    with contextlib.ExitStack() as locks:
        return execute_owned(ctx, fd, enter, locks)


def execute_owned(ctx, fd, enter, locks):
    from .sleep import prepare, wake_irqs
    from .suspend_record import record
    # Require the actual inherited lock, never a user-supplied PID or stale file.
    if not os.path.samestat(os.fstat(fd), ctx.path('/run/y2/suspend.lock').stat()):
        raise ValueError('invalid_suspend_lock')
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    prepared = False
    radios_changed = False
    result = 0
    pending = False
    attempted = False
    restored_kernel = False
    refusal = None
    held = {}
    try:
        held = locks.enter_context(operation_locks(ctx))
        admission(ctx)
        battery_and_cable(ctx)
        operation_state(ctx, workloads=False)
        count = wakeup_count(ctx)
        previous_irqs = wake_irqs(ctx.read('/proc/interrupts'))
        guard_record(ctx, True)
        pending = True
        marker(ctx, 'HELPER_REQUEST')
        try:
            prepare(ctx)
        except ValueError as error:
            raise Refused(str(error)) from error
        except OSError as error:
            raise Refused('reborn_preparation_unavailable') from error
        prepared = True
        wait_for_application(ctx, previous_irqs)
        record(ctx, 'quiescing_radios', 0)
        radios_changed = True
        for radio in ('wifi', 'bluetooth'):
            quiesce_radio(ctx, radio)
        deadline = time.monotonic() + 15
        while not re.search(r'powered=0 functions=(?:0x0|0) ',
                            ctx.read('/sys/devices/platform/18070000.connectivity/status') or ''):
            if time.monotonic() >= deadline:
                raise Refused('radio_power_down')
            time.sleep(.1)
        stale_counts = 0
        while True:
            # Reading can block until an event finishes. The write rejects
            # any intervening Power/RTC/USB wake before the kernel entry.
            if count is None:
                count = wakeup_count(ctx)
            if previous_irqs is not None:
                if wake_irqs(ctx.read('/proc/interrupts')) != previous_irqs:
                    raise Refused('wake_event_in_progress')
            if count is None or not count.isdecimal():
                raise Refused('wakeup_count_unavailable')
            battery_and_cable(ctx)
            operation_state(ctx)
            record(ctx, 'syncing', 0)
            command(ctx, ['/bin/sync'])
            record(ctx, 'kernel_suspend', 0)
            guard_record(ctx, True, kernel_entry=True)
            pending = True
            try:
                control_write(ctx, '/sys/power/wakeup_count', count + '\n')
            except OSError as error:
                # An event completed since the count was read. Archives 155
                # (06:38 self-wake) and 156 (refused Power sleep): a routine
                # BAT0 notification during preparation. The next pass reads a
                # fresh count and repeats the Power/RTC IRQ and cable checks,
                # so a real wake still cancels entry.
                stale_counts += 1
                if stale_counts > STALE_COUNT_RETRIES:
                    raise Refused('wakeup_count_busy') from error
                count = None
                continue
            stale_counts = 0
            restored_kernel = False
            attempted = True
            try:
                # The already qualified one-shot RGU backstop pauses during
                # actual SPM residence and bounds stalls in entry/restoration.
                # A reset remains a failed sleep, quarantined by durable intent.
                control_write(ctx, '/sys/firmware/y2_pm/backstop_s', '30\n')
                try:
                    enter(ctx)
                finally:
                    control_write(ctx, '/sys/firmware/y2_pm/backstop_s', '0\n')
            except OSError as error:
                if error.errno in (errno.EBUSY, errno.EAGAIN):
                    restored_kernel = True
                    raise Refused('kernel_suspend_refused') from error
                if failed_before_freeze(ctx):
                    # The write returned and no task froze: nothing is left
                    # unproven. 8.10 kept this pending and refused every sleep
                    # on that image (237 refusals in 4.4 h on battery).
                    restored_kernel = True
                    retry_later(ctx)
                    raise Refused('kernel_prepare_refused') from error
                raise
            count = wakeup_count(ctx)
            value = record(ctx, 'kernel_resumed', 0)
            restored_kernel = value['product'].get('kernel_completed') is True
            if not restored_kernel:
                raise RuntimeError('kernel_exit_not_proven')
            if not timer_only(value, spm_state(ctx)):
                break
            # No display, input reactivation, radio restore or library scan
            # during a maintenance return. Safety daemon gets a normal tick;
            # the following admission also reads the ADC directly.
            previous_irqs = value['wake_irqs_after']
            record(ctx, 'maintenance_wake', 0)
            time.sleep(2)
            # The safety tick may publish a normal BAT0 notification. Begin a
            # new handshake after that pause; reusing the pre-tick count would
            # reject an otherwise safe next cycle. Preserve previous_irqs so
            # Power/RTC during the pause still cancel entry, and the atomic
            # count store still rejects any later event.
            count = None
    except Refused as error:
        result = 2
        refusal = str(error)
        record(ctx, 'product_refused', result, reason=refusal)
        # No kernel call followed a rejected wake-count handshake. Only
        # `wake_event_in_progress` is a Power/RTC wake; `wakeup_count_busy`
        # is a kernel wakeup source that did not settle.
        if refusal in ('wake_event_in_progress', 'wakeup_count_busy'):
            restored_kernel = True
    except (OSError, ValueError, RuntimeError) as error:
        result = 1
        record(ctx, 'product_failure', result, reason=str(error))
    finally:
        # Keep every operation lock held until radios and Reborn restore.
        if prepared:
            # POWER-FINAL-17: screen first. Wi-Fi/Bluetooth firmware, patches
            # and RF calibration take ≈1.6 s (archive 110) and the UI used to
            # wait for them; it does not depend on the radios. A failed early
            # restore is retried by the completion record, as before.
            # 4fed1b6a: nothing durable followed `kernel_resumed`; these two
            # light rows say whether a stall is in the screen restore or after it.
            progress(ctx, 'screen_restoring')
            # F47: a refusal leaves the screen dark (it went dark with the
            # request); only a wake — Power or RTC during the preparation, or
            # a real return from the kernel — lights it. Lit after every
            # refusal, a kernel that refused each sleep turned the screen on
            # every two minutes for hours. Failures still light it to explain.
            dark = result == 2 and refusal != 'wake_event_in_progress'
            screen = screen_first(ctx, dark)
            progress(ctx, 'screen_restored' if screen else 'screen_restore_failed')
            # F24: nothing sleeps any more. A Play right after waking took
            # Reborn's shared activity lease and was refused as busy (ALSA
            # -16) for the ≈1.6 s of the radio restore below; wired playback
            # does not need the radios. The other operation locks stay.
            release_activity(held)
            try:
                record(ctx, 'restoring_radios', result)
                marker(ctx, 'RADIOS_RESTORING')
                if radios_changed:
                    for radio, directory in (('wifi', 'network'), ('bluetooth', 'bluetooth')):
                        if ctx.read('/data/' + directory + '/enabled') == '1':
                            command(ctx, ['/usr/sbin/y2-radio', radio, 'on-runtime'])
            except (OSError, ValueError, RuntimeError):
                result = 1
            value = record(ctx, 'complete_result_' + str(result), result, restore_ack=screen, dark=dark)
            if value.get('result'):
                result = value['result']
            if result == 0:
                marker(ctx, 'COMPLETE')
        # Clear whenever no kernel attempt is left unproven: a refusal, a
        # proven kernel return, or any failure before kernel entry (12 left
        # result 1 pending here, refusing all sleep until a reflash).
        if pending and (restored_kernel or not attempted):
            guard_record(ctx, False)
    return result


def main():
    fd = int(sys.argv[1])
    def stop(signum, frame):
        raise RuntimeError('sleep_helper_interrupted')
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        raise SystemExit(execute(Context(), fd))
    finally:
        os.close(fd)
