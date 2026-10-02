"""Validated codec quality preferences, separated from the active daemon contract."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import os
from .common import atomic_json

DEFAULTS = {'sbc_quality': 'high', 'ldac_quality': 'standard', 'ldac_abr': True}


def requested(ctx):
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    policy = ctx.json('/data/bluetooth/codec-policy.json', {})
    default = inventory.get('private_integration_enabled') is True
    if policy and policy.get('schema') != 1:
        raise ValueError('invalid_codec_policy')
    return {'schema': 1, 'experimental': policy.get('experimental', default),
            **{key: policy.get(key, value) for key, value in DEFAULTS.items()}}


def validate(value):
    if type(value.get('experimental')) is not bool or type(value.get('ldac_abr')) is not bool:
        raise ValueError('invalid_codec_boolean')
    if value.get('sbc_quality') not in ('high', 'xq', 'xq+'):
        raise ValueError('invalid_sbc_quality')
    if value['sbc_quality'] != 'high' and value['experimental'] is not True:
        raise ValueError('sbc_quality_requires_experimental_gate')
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
            'supported': {'sbc_quality': ['high', 'xq', 'xq+'] if policy['experimental'] else ['high'],
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
        atomic_json(ctx.path('/data/bluetooth/codec-policy.json'), value, durable=True)
    finally:
        os.close(lock)
    return status(ctx)
