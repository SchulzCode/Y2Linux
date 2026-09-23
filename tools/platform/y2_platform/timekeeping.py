"""Reasonable-clock/TLS readiness; standard BusyBox NTP and kernel CSPRNG."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import math
import os
from pathlib import Path
import time
from .boot import private_directory
from .common import atomic_json


def plausible(epoch, floor):
    return (isinstance(epoch, (int, float)) and math.isfinite(epoch) and
            isinstance(floor, int) and floor > 0 and floor - 86400 <= epoch <= floor + 5 * 366 * 86400)


def entropy_ready():
    try:
        return len(os.getrandom(1, os.GRND_NONBLOCK)) == 1
    except (OSError, AttributeError):
        return False


def status(ctx, now=None, monotonic=None):
    now = time.time() if now is None else now
    monotonic = time.monotonic() if monotonic is None else monotonic
    floor = ctx.integer('/etc/y2linux/build-epoch')
    valid = plausible(now, floor)
    anchor = ctx.json('/run/y2/time.json', {})
    stamp, wall = anchor.get('monotonic_s'), anchor.get('epoch')
    coherent = (anchor.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
                isinstance(stamp, (int, float)) and isinstance(wall, (int, float)) and
                0 <= monotonic - stamp <= 86400 and abs(now - wall - (monotonic - stamp)) <= 60)
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
    # Kernel hctosys supplied wall time. Only a plausible RTC following an
    # earlier network clock establishes retained-clock readiness; a build date
    # alone is merely a lower bound and cannot authorize TLS downloads.
    if plausible(now, floor) and plausible_retained and now >= retained - 60 and rtc_date:
        source = 'retained_rtc'
    else:
        source = 'build_floor'
        target = max(floor or 0, retained if plausible_retained else 0)
        if not plausible(now, floor) and target:
            result = ctx.command(['/bin/busybox', 'date', '-u', '-s', '@' + str(int(target))])
            if result['ok']:
                now = target
    value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
             'epoch': now, 'monotonic_s': time.monotonic(), 'source': source}
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
                 'epoch': now, 'monotonic_s': time.monotonic(), 'source': 'ntp',
                 'stratum': stratum, 'authenticated': False, 'event': event}
        atomic_json(runtime / 'time.json', value)
        # Durable floor at most once an hour, plus the first sync. No seeds or
        # private credentials are present in this small record.
        previous = ctx.json('/data/system/platform/time-last.json', {})
        previous_time = previous.get('epoch', 0)
        if not isinstance(previous_time, (int, float)) or abs(now - previous_time) >= 3600:
            directory = private_directory(ctx.path('/data/system/platform'))
            atomic_json(directory / 'time-last.json', value, durable=True)
        # RTC writes require an explicit qualification receipt; wrong retained
        # dates must not silently become a new RTC programming experiment.
        rtc = ctx.json('/data/system/platform/rtc-policy.json', {})
        if rtc.get('write_enabled') is True and rtc.get('qualification_reference'):
            value['rtc_write'] = ctx.command(['/bin/busybox', 'hwclock', '-w', '-u'], timeout=3)['ok']
        return value
    finally:
        os.close(fd)
