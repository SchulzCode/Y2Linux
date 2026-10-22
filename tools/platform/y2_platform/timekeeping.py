"""Reasonable-clock/TLS readiness; standard BusyBox NTP and kernel CSPRNG."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import math
import os
from pathlib import Path
import time
from .boot import private_directory
from .common import atomic_json

# The durable floor (time-last.json) is written at most once an hour.
RETAIN_S = 3600


def plausible(epoch, floor):
    return (isinstance(epoch, (int, float)) and math.isfinite(epoch) and
            isinstance(floor, int) and floor > 0 and floor - 86400 <= epoch <= floor + 5 * 366 * 86400)


def entropy_ready():
    try:
        return len(os.getrandom(1, os.GRND_NONBLOCK)) == 1
    except (OSError, AttributeError):
        return False


def boottime():
    """Elapsed time including suspend, like the wall clock."""
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def drift(anchor, now, monotonic, elapsed):
    """How far the wall clock moved apart from the anchor since it was written."""
    stamp, wall, since = anchor.get('monotonic_s'), anchor.get('epoch'), anchor.get('boottime_s')
    # The wall clock keeps running in suspend, CLOCK_MONOTONIC does not: after
    # any sleep over 60 s the monotonic comparison called a correct clock
    # incoherent and every update check failed until ntpd's next callback
    # (Beta 4: "Connect to Wi-Fi" after sleep). Compare with CLOCK_BOOTTIME.
    return (now - wall - (elapsed - since) if isinstance(since, (int, float)) and isinstance(wall, (int, float))
            else now - wall - (monotonic - stamp) if isinstance(stamp, (int, float)) and isinstance(wall, (int, float))
            else math.inf)


def status(ctx, now=None, monotonic=None, elapsed=None):
    now = time.time() if now is None else now
    monotonic = time.monotonic() if monotonic is None else monotonic
    elapsed = boottime() if elapsed is None else elapsed
    floor = ctx.integer('/etc/y2linux/build-epoch')
    valid = plausible(now, floor)
    anchor = ctx.json('/run/y2/time.json', {})
    stamp, wall = anchor.get('monotonic_s'), anchor.get('epoch')
    coherent = (anchor.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
                isinstance(stamp, (int, float)) and isinstance(wall, (int, float)) and
                0 <= monotonic - stamp <= 86400 and abs(drift(anchor, now, monotonic, elapsed)) <= 60)
    ready = valid and coherent and anchor.get('source') in ('ntp', 'retained_rtc')
    return {'schema': 1, 'clock_sane': valid, 'tls_ready': bool(ready),
            'reason': 'reasonable_clock' if ready else 'clock_not_established',
            'source': anchor.get('source'), 'ntp_authenticated': False,
            'epoch': now, 'build_epoch': floor, 'anchor': anchor,
            'entropy_ready': entropy_ready() if ctx.root == Path('/') else None,
            'rtc_retention_qualified': False}


def bootstrap(ctx, now=None):
    now = time.time() if now is None else now
    floor = ctx.integer('/etc/y2linux/build-epoch')
    runtime = private_directory(ctx.path('/run/y2'))
    previous = ctx.json('/data/system/platform/time-last.json', {})
    retained = previous.get('epoch')
    plausible_retained = plausible(retained, floor)
    rtc_date = ctx.read('/sys/class/rtc/rtc0/date')
    # The owner's own date and time (#53) carries over a restart as itself:
    # a lower bound like the build date, never TLS readiness.
    manual = previous.get('source') == 'manual'
    # Kernel hctosys supplied wall time. Only a plausible RTC following an
    # earlier network clock establishes retained-clock readiness; a build date
    # alone is merely a lower bound and cannot authorize TLS downloads.
    if plausible(now, floor) and plausible_retained and now >= retained - 60 and rtc_date:
        source = 'manual' if manual else 'retained_rtc'
    else:
        source = 'build_floor'
        target = max(floor or 0, retained if plausible_retained else 0)
        if not plausible(now, floor) and target:
            result = ctx.command(['/bin/busybox', 'date', '-u', '-s', '@' + str(int(target))])
            if result['ok']:
                now = target
                source = 'manual' if manual and target == retained else source
    value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
             'epoch': now, 'monotonic_s': time.monotonic(), 'boottime_s': boottime(), 'source': source}
    atomic_json(runtime / 'time.json', value)
    return value


def ntp_event(ctx, event, env, now=None):
    if event not in ('step', 'stratum', 'periodic', 'unsync'):
        raise ValueError('unknown_ntp_event')
    now = time.time() if now is None else now
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'time.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        # BusyBox explicitly permits overlapping step/stratum callbacks.
        fcntl.flock(fd, fcntl.LOCK_EX)
        stratum = int(env.get('stratum', '16'))
        if event == 'unsync' or not 1 <= stratum < 16 or not plausible(now, ctx.integer('/etc/y2linux/build-epoch')):
            return {'state': 'Unavailable', 'reason': 'ntp_unsynchronized_or_implausible'}
        value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'epoch': now, 'monotonic_s': time.monotonic(), 'boottime_s': boottime(), 'source': 'ntp',
                 'stratum': stratum, 'authenticated': False, 'event': event}
        atomic_json(runtime / 'time.json', value)
        # Durable floor at most once an hour, plus the first sync (also the
        # first after a time set by hand, #53). No seeds or private
        # credentials are present in this small record.
        previous = ctx.json('/data/system/platform/time-last.json', {})
        previous_time = previous.get('epoch', 0)
        if (not isinstance(previous_time, (int, float)) or abs(now - previous_time) >= RETAIN_S or
                previous.get('source') != 'ntp'):
            directory = private_directory(ctx.path('/data/system/platform'))
            atomic_json(directory / 'time-last.json', value, durable=True)
        rtc_write(ctx, value)
        return value
    finally:
        os.close(fd)


def rtc_write(ctx, value):
    # Source-backed default or an owner receipt admits normal NTP -> RTC
    # synchronization. Mutable owner policy still takes precedence.
    rtc = ctx.json('/data/system/platform/rtc-policy.json', ctx.json('/etc/y2linux/rtc-policy.json', {}))
    if rtc.get('write_enabled') is True and (rtc.get('qualification_reference') or rtc.get('source')):
        value['rtc_write'] = ctx.command(['/bin/busybox', 'hwclock', '-w', '-u'], timeout=3)['ok']


def set_manual(ctx, epoch):
    """The owner's own date and time (#53), for a player without Wi-Fi.

    Within the build-date range only; it never makes the clock TLS ready (an
    owner's guess is not an authenticated time), and the next network time
    replaces it. Remembered for the next boot, which starts from it instead
    of the build date."""
    floor = ctx.integer('/etc/y2linux/build-epoch')
    if type(epoch) is not int or not plausible(epoch, floor):
        raise ValueError('date_out_of_range')
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'time.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        # Network time this boot: the clock is right, and the running time
        # server would keep a hand-set time for minutes while its periodic
        # callback calls it network time (8.22 test build: 2 h off, "ntp").
        anchor = ctx.json('/run/y2/time.json', {})
        if anchor.get('source') == 'ntp' and anchor.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id'):
            raise ValueError('network_time_active')
        if not ctx.command(['/bin/busybox', 'date', '-u', '-s', '@' + str(epoch)], timeout=3)['ok']:
            raise ValueError('date_not_set')
        value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'epoch': epoch, 'monotonic_s': time.monotonic(), 'boottime_s': boottime(), 'source': 'manual'}
        atomic_json(runtime / 'time.json', value)
        # Readers of the status (scrobbling) see the untrusted clock at once.
        atomic_json(runtime / 'time-status.json', status(ctx, now=epoch))
        directory = private_directory(ctx.path('/data/system/platform'))
        atomic_json(directory / 'time-last.json', value, durable=True)
        rtc_write(ctx, value)
        return value
    finally:
        os.close(fd)


def remember(ctx, now=None, monotonic=None, elapsed=None, every=RETAIN_S):
    """Keep the durable floor near the running clock, at most once per `every`.

    Only for a clock set from the network or by hand this boot that nobody
    stepped since: a restart without Wi-Fi then resumes about where it was
    rather than from the last sync or the build date."""
    now = time.time() if now is None else now
    monotonic = time.monotonic() if monotonic is None else monotonic
    elapsed = boottime() if elapsed is None else elapsed
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'time.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        # Never over a record that a sync or the owner wrote meanwhile.
        fcntl.flock(fd, fcntl.LOCK_EX)
        anchor = ctx.json('/run/y2/time.json', {})
        if (anchor.get('source') not in ('ntp', 'manual') or
                anchor.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id') or
                not plausible(now, ctx.integer('/etc/y2linux/build-epoch')) or
                abs(drift(anchor, now, monotonic, elapsed)) > 60):
            return None
        previous = ctx.json('/data/system/platform/time-last.json', {})
        last = previous.get('epoch')
        if (isinstance(last, (int, float)) and 0 <= now - last < every and
                previous.get('source') == anchor['source']):
            return None
        value = dict(anchor, epoch=now, monotonic_s=monotonic, boottime_s=elapsed)
        directory = private_directory(ctx.path('/data/system/platform'))
        atomic_json(directory / 'time-last.json', value, durable=True)
        return value
    finally:
        os.close(fd)


def alarm(ctx, seconds):
    """Kernel RTC alarm plumbing; arming an alarm does not enter suspend."""
    if type(seconds) is not int or not 0 <= seconds <= 604800:
        raise ValueError('rtc_alarm_seconds_out_of_range')
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'rtc-alarm.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        now = ctx.integer('/sys/class/rtc/rtc0/since_epoch')
        if now is None or now <= 0:
            raise ValueError('rtc_time_unavailable')
        path = ctx.path('/sys/class/rtc/rtc0/wakealarm')
        # Linux requires disabling an existing alarm before replacement.
        path.write_text('0\n')
        target = now + seconds if seconds else 0
        if target:
            path.write_text(str(target) + '\n')
        observed = ctx.integer('/sys/class/rtc/rtc0/wakealarm')
        valid = observed == target or (target == 0 and observed is None)
        if not valid:
            path.write_text('0\n')
            raise ValueError('rtc_alarm_readback_mismatch')
        value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'alarm_epoch': target or None, 'state': 'Armed' if target else 'Disabled',
                 'qualified': False, 'reason': 'same_boot_alarm_wake_PHYSICAL_PENDING'}
        atomic_json(runtime / 'rtc-alarm.json', value)
        return value
    finally:
        os.close(fd)
