"""Explicit user radio action, serialized with the sole automatic reconnect owner."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import os
import tempfile
import time


def intent(ctx, operation):
    if operation not in ('connect', 'disconnect', 'pair', 'forget', 'power_on', 'power_off', 'uncertain', 'power_pending'):
        raise ValueError('invalid_bluetooth_intent')
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if not boot or len(boot) != 36:
        raise ValueError('boot_identity_unavailable')
    now = time.monotonic_ns() // 1000
    directory = ctx.path('/run/y2')
    fd, temporary = tempfile.mkstemp(dir=directory, prefix='.bt-control.')
    try:
        with os.fdopen(fd, 'w') as stream:
            stream.write(f'[intent]\nversion=1\nboot_id={boot}\nsequence={now}\n'
                         f'operation={operation}\ndeadline_us={now + 30_000_000}\n')
        os.replace(temporary, directory / 'bt-control.ini')
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def power(ctx, value):
    if value not in ('on', 'off'):
        raise ValueError('invalid_bluetooth_power')
    fd = os.open(ctx.path('/run/y2/bt-operation.lock'), os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {'ok': False, 'reason': 'platform_bluetooth_operation_in_progress'}
        intent(ctx, 'power_pending' if value == 'on' else 'power_off')
        result = ctx.command(['bluetoothctl', 'power', value], timeout=15)
        intent(ctx, 'power_' + value if result['ok'] else 'uncertain')
        return result
    finally:
        os.close(fd)
