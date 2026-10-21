"""Validated codec quality preferences, separated from the active daemon contract."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import os
from .common import atomic_json

# AAC "high": FDK afterburner and up to the peer's maximum rate where a frame
# still fits one packet (AirPods: 256 kbit/s); "standard" is BlueALSA's tested
# 220 kbit/s. Adaptive rate steps CBR AAC down while the link backs up, and
# the SBC bit-pool (53 -> 45 -> 35) the same way (BlueALSA patch 0003).
DEFAULTS = {'sbc_quality': 'high', 'sbc_abr': True, 'ldac_quality': 'standard', 'ldac_abr': True,
            'aac_quality': 'high', 'aac_abr': True}


def requested(ctx):
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    policy = ctx.json('/data/bluetooth/codec-policy.json', {})
    default = inventory.get('private_integration_enabled') is True
    if policy and policy.get('schema') != 1:
        raise ValueError('invalid_codec_policy')
    return {'schema': 1, 'experimental': policy.get('experimental', default),
            **{key: policy.get(key, value) for key, value in DEFAULTS.items()}}


def validate(value):
    if any(type(value.get(key)) is not bool for key in ('experimental', 'sbc_abr', 'ldac_abr', 'aac_abr')):
        raise ValueError('invalid_codec_boolean')
    # SBC XQ is an owner choice (some SBC-only headphones sound much better,
    # a few refuse it); the default stays "high".
    if value.get('sbc_quality') not in ('high', 'xq', 'xq+'):
        raise ValueError('invalid_sbc_quality')
    if value.get('aac_quality') not in ('standard', 'high'):
        raise ValueError('invalid_aac_quality')
    if value.get('ldac_quality') not in ('mobile', 'standard', 'high'):
        raise ValueError('invalid_ldac_quality')
    return value


def status(ctx):
    policy = validate(requested(ctx))
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    applied = ctx.json('/run/y2/codec-runtime.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    # This receipt records startup arguments, never endpoint/negotiation success.
    pid = ctx.integer('/run/y2/bluealsa.pid')
    command = ctx.read(f'/proc/{pid}/cmdline', 8192) if pid and pid > 1 else None
    argv = command.split('\0') if command else []
    live = bool(argv and argv[0].rsplit('/', 1)[-1] == 'bluealsad'
                and all(arg in argv for arg in applied.get('arguments', [])))
    effective = applied.get('settings') if live and applied.get('boot_id') == boot and boot else None
    return {'schema': 'org.y2linux.codec-settings/v1', 'requested': policy,
            'effective': effective, 'effective_scope': 'daemon_start_arguments',
            'pending_restart': effective != policy,
            'apply_action': 'restart_player',
            'supported': {'sbc_quality': ['high', 'xq', 'xq+'], 'sbc_abr': True,
                          'aac_quality': ['standard', 'high'] if inventory.get('codecs', {}).get('AAC', {}).get('compiled_locally') else [],
                          'aac_abr': inventory.get('codecs', {}).get('AAC', {}).get('compiled_locally') is True,
                          'ldac_quality': ['mobile', 'standard', 'high'] if inventory.get('codecs', {}).get('LDAC', {}).get('compiled_locally') else [],
                          'ldac_abr': inventory.get('codecs', {}).get('LDAC', {}).get('compiled_locally') is True}}


def configure(ctx, **changes):
    changes = {k: v for k, v in changes.items() if v is not None}
    if set(changes) - set(DEFAULTS):
        raise ValueError('unknown_codec_setting')
    if not changes:
        return status(ctx)
    lock = os.open(ctx.path('/run/y2/codec-settings.lock'), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        value = validate({**requested(ctx), **changes})
        caps = status(ctx)['supported']
        if any(key.startswith('ldac_') for key in changes) and not caps['ldac_quality']:
            raise ValueError('ldac_encoder_not_compiled')
        if any(key.startswith('aac_') for key in changes) and not caps['aac_quality']:
            raise ValueError('aac_encoder_not_compiled')
        atomic_json(ctx.path('/data/bluetooth/codec-policy.json'), value, durable=True)
    finally:
        os.close(lock)
    return status(ctx)
