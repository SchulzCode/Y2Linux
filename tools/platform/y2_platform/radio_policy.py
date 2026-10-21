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
    def readback():
        answer = ctx.command(['/usr/sbin/iw', 'dev', 'wlan0', 'get', 'power_save'], timeout=3)
        actual = ({'Power save: on': True, 'Power save: off': False}.get(
                  (answer['output'] or '').strip()) if answer['ok'] else None)
        return actual, answer

    # GET checks cfg80211's current state, including interface recreation and
    # external changes. Avoid resending CMD_ID_POWER_SAVE_MODE every 30 seconds
    # when this same setting is already in effect. Readback is host state, not
    # proof of firmware sleep or electrical savings.
    actual, result = readback()
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    epoch = ctx.read('/run/y2/wifi-epoch')
    ifindex = ctx.read('/sys/class/net/wlan0/ifindex')
    previous = ctx.json('/run/y2/wifi-power.json', {})
    initialized = (previous.get('state') == 'Ready' and previous.get('boot_id') == boot and
                   previous.get('epoch') == epoch and previous.get('ifindex') == ifindex and
                   previous.get('requested') is requested)
    # Establish policy once per association/interface; cfg80211's initial flag
    # alone cannot establish what firmware chose during interface creation.
    applied = actual is not requested or not initialized
    if applied:
        result = ctx.command(['/usr/sbin/iw', 'dev', 'wlan0', 'set', 'power_save',
                              'on' if requested else 'off'], timeout=2)
        actual = readback()[0] if result['ok'] else None
    value = {'schema': 1, 'mode': mode, 'requested': requested, 'enabled': actual, 'applied': applied,
             'state': 'Ready' if actual is requested else 'Unavailable',
             'reason': None if actual is requested else result.get('reason') or 'power_save_readback_unavailable',
             'provisional': True, 'requested_driver_profile': 'FAST_PSP' if requested else 'CAM',
             'epoch': epoch, 'ifindex': ifindex,
             'monotonic_s': time.monotonic(),
             'boot_id': boot}
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


def codec_runtime(ctx, record=False):
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    from .codec_controls import requested, validate
    policy = validate(requested(ctx))
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
    if quality not in ('high', 'xq', 'xq+'):
        raise ValueError('invalid_sbc_quality')
    args.append('--sbc-quality=' + quality)
    if policy.get('sbc_abr', True) is True:
        args.append('--sbc-abr')
    if '--codec=AAC' in args:
        # The patched encoder caps this by the peer and by one packet per frame.
        if policy.get('aac_quality', 'high') == 'high':
            args += ['--aac-afterburner', '--aac-bitrate=320000']
        if policy.get('aac_abr', True) is True:
            args.append('--aac-abr')
    ldac = policy.get('ldac_quality', 'standard')
    if ldac not in ('mobile', 'standard', 'high'):
        raise ValueError('invalid_ldac_quality')
    if inventory.get('codecs', {}).get('LDAC', {}).get('compiled_locally') is True:
        args.append('--ldac-quality=' + ldac)
        if policy.get('ldac_abr', True) is True:
            args.append('--ldac-abr')
    if record:
        atomic_json(ctx.path('/run/y2/codec-runtime.json'), {
            'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
            'settings': policy, 'arguments': args})
    return args
