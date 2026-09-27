"""Runtime radio policy and coexistence hooks; no invented firmware features."""
# SPDX-License-Identifier: GPL-2.0-only
import json
import time
from .common import atomic_json


def wifi_policy(ctx, activity=None):
    policy = ctx.json('/data/network/power-save.json', {'schema': 1, 'mode': 'automatic', 'automatic_standard': False})
    mode = policy.get('mode')
    if policy.get('schema') != 1 or mode not in ('off', 'standard', 'automatic'):
        return {'state': 'Failed', 'reason': 'invalid_wifi_power_policy'}
    activity = activity or {}
    requested = mode == 'standard' or (mode == 'automatic' and policy.get('automatic_standard') is True
                                       and not activity.get('wifi_heavy_transfer'))
    result = ctx.command(['/usr/sbin/iw', 'dev', 'wlan0', 'set', 'power_save', 'on' if requested else 'off'], timeout=2)
    observed = ctx.command(['/usr/sbin/iw', 'dev', 'wlan0', 'get', 'power_save'], timeout=2) if result['ok'] else None
    actual = None
    if observed and observed['ok']:
        actual = {'Power save: on': True, 'Power save: off': False}.get((observed['output'] or '').strip())
    value = {'schema': 1, 'mode': mode, 'requested': requested, 'enabled': actual,
             'state': 'Ready' if actual is requested else 'Unavailable',
             'reason': None if actual is requested else result.get('reason') or 'power_save_readback_unavailable',
             'provisional': True, 'monotonic_s': time.monotonic(),
             'boot_id': ctx.read('/proc/sys/kernel/random/boot_id')}
    atomic_json(ctx.path('/run/y2/wifi-power.json'), value)
    return value


class Coexistence:
    def __init__(self):
        self.previous = None

    def observe(self, ctx, wifi, now):
        stats = wifi.get('traffic_counters') or {}
        total = sum(stats.get(k) or 0 for k in ('rx_bytes', 'tx_bytes'))
        rate = None
        if self.previous:
            previous_time, previous_bytes = self.previous
            if now > previous_time and total >= previous_bytes:
                rate = (total - previous_bytes) / (now - previous_time)
        self.previous = now, total
        value = {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'), 'monotonic_s': now,
                 'wifi_active': wifi.get('state') == 'Online', 'wifi_bytes_per_second': rate,
                 'wifi_heavy_transfer': rate is not None and rate >= 1024**2,
                 'cpu_pressure': ctx.read('/proc/pressure/cpu'),
                 'bluetooth_link': ctx.json('/run/y2/bt-reconnect.json', {}),
                 'provisional': True, 'codec_bitrate_override': None}
        atomic_json(ctx.path('/run/y2/coexistence.json'), value)
        return value


def codec_runtime(ctx):
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    policy = ctx.json('/data/bluetooth/codec-policy.json', {'schema': 1, 'experimental': False})
    if inventory.get('schema') != 1 or policy.get('schema') != 1:
        raise ValueError('codec_policy_or_inventory_unavailable')
    experimental = policy.get('experimental') is True
    args = []
    for name in ('AAC', 'aptX', 'aptX-HD', 'LDAC'):
        cap = inventory.get('codecs', {}).get(name, {})
        eligible = cap.get('compiled_locally') is True and (
            (cap.get('distribution_approved') is True and cap.get('platform_qualified') is True) or
            (experimental and cap.get('owner_private_experiment') is True))
        if cap.get('compiled_locally') is True:
            args.append('--codec=' + ('' if eligible else '-') + name)
    quality = policy.get('sbc_quality', 'high')
    if quality not in ('high', 'xq', 'xq+') or (quality != 'high' and not experimental):
        raise ValueError('sbc_quality_requires_experimental_gate')
    args.append('--sbc-quality=' + quality)
    ldac = policy.get('ldac_quality', 'standard')
    if ldac not in ('mobile', 'standard', 'high'):
        raise ValueError('invalid_ldac_quality')
    if inventory.get('codecs', {}).get('LDAC', {}).get('compiled_locally') is True:
        args.append('--ldac-quality=' + ldac)
        if policy.get('ldac_abr', True) is True:
            args.append('--ldac-abr')
    return args
