"""Durable suspend receipts, called only before entry or after Linux returns."""
# SPDX-License-Identifier: GPL-2.0-only
from .common import Context, atomic_json, read
from .boot import private_directory
from .sleep import transition, wake_irqs
import os
import re
import json
import socket
import sys
import time

# POWER-FINAL-14: stages that also write a persistent sleep-diagnostics row.
DIAGNOSTIC_STAGES = {'kernel_suspend', 'kernel_resumed', 'restoring_radios', 'product_failure'}


def record(ctx, stage, result=None, reason=None, reborn_ack=None, mode=None, restore_ack=None, dark=False):
    directory = private_directory(ctx.path('/data/system/platform'))
    path = directory / 'suspend-last.json'
    value = ctx.json('/data/system/platform/suspend-last.json', {})
    if not isinstance(value, dict):
        value = {}
    if stage == 'requested' or (stage == 'quiescing_radios' and not value.get('reborn_prepared')) or stage.startswith('refused_'):
        value = {'schema': 1, 'requested_mode': 'deep',
                 'request_boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'started': time.time(), 'stages': [],
                 'pm_test': next(iter(re.findall(r'\[(\w+)\]', ctx.read('/sys/power/pm_test') or '')), None)}
    if mode is not None:
        value['mode'] = mode
    if stage == 'product_failure':
        value['error'] = reason
    value.update(stage=stage, result=result, updated=time.time(),
                 current_boot_id=ctx.read('/proc/sys/kernel/random/boot_id'),
                 persistent_kernel=ctx.read('/sys/firmware/y2_pm/state'),
                 usb_restore={'controllers': {p.parent.name: read(p, limit=8192)
                     for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/status')},
                     'udcs': {p.name: read(p/'state') for p in ctx.glob('/sys/class/udc/*')}},
                 radio_restore=ctx.read('/sys/devices/platform/18070000.connectivity/status'))
    if stage == 'kernel_suspend':
        value['wake_irqs_before'] = wake_irqs(ctx.read('/proc/interrupts'))
        value['entry'] = {'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME),
                          'monotonic_s': time.monotonic(),
                          'voltage_uv': ctx.integer('/sys/class/power_supply/BAT0/voltage_now'),
                          'battery': ctx.json('/run/y2/power.json', {}).get('battery')}
        # Maintenance returns update entry for the next segment. Retain the
        # actual first pre-suspend ADC/timestamp for the whole sleep interval.
        value.setdefault('first_entry', value['entry'])
    elif stage in {'kernel_resumed', 'restoring_radios'}:
        value['wake_irqs_after'] = wake_irqs(ctx.read('/proc/interrupts'))
    if stage == 'kernel_resumed':
        from .product_sleep import spm_state
        spm = spm_state(ctx)
        value['resume'] = {'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME),
                           'monotonic_s': time.monotonic(), 'spm': spm,
                           'voltage_uv': ctx.integer('/sys/class/power_supply/BAT0/voltage_now'),
                           'battery': ctx.json('/run/y2/power.json', {}).get('battery')}
        value['cycles'] = value.get('cycles', 0) + 1
        # Only returned reset-path PCM596 entry counts, never a PM test/abort.
        if transition(value, stage, result, value['current_boot_id']).get('kernel_completed'):
            ticks = int(spm.get('ticks32k', '0'))
            value['suspended_seconds'] = value.get('suspended_seconds', 0) + ticks / 32768
    if stage == 'maintenance_wake':
        value['maintenance_wakes'] = value.get('maintenance_wakes', 0) + 1
    if reborn_ack is not None:
        value['reborn_prepared'] = reborn_ack
    value.setdefault('stages', []).append({'stage': stage, 'time': time.time(), 'result': result})
    value['stages'] = value['stages'][-32:]
    if stage.startswith('complete_result_'):
        try:
            # An earlier successful restore (screen before radios) is reused;
            # asking again would redraw and reset Reborn's wake gesture.
            # A dark restore (refused sleep, F47) keeps the screen off.
            acknowledgement = restore_ack or application_rpc(
                ctx, 'sleep_restore', **({'dark': True} if dark else {}))
            value['reborn_restore'] = acknowledgement.get('sleep_ready') is True and acknowledgement.get('screen_off') is dark
        except (OSError, ValueError):
            value['reborn_restore'] = False
        if not value['reborn_restore']:
            value['result'] = value['result'] or 1
            value['error'] = 'reborn_resume_probe_failed'
    value['product'] = transition(value, stage, value['result'], value['current_boot_id'], reason)
    if stage == 'complete_result_0' and value['product']['state'] not in {'restored', 'idle'}:
        value['result'] = 1
        value['error'] = value['product']['reason']
    if value['product']['state'] == 'restored':
        marker = ctx.path('/sys/firmware/y2_pm/stage')
        if marker.exists():
            marker.write_text('REBORN_READY\n')
    atomic_json(path, value, durable=True)
    from .power_journal import PowerJournal
    PowerJournal(ctx).observe('Suspend', ctx.json('/run/y2/power.json', {}).get('battery', {}),
                              event='suspend_' + stage)
    if stage in DIAGNOSTIC_STAGES or stage.startswith('complete_result_'):
        from .sleep_diagnostics import record as diagnostics
        diagnostics(ctx, stage)
    parent = os.open(directory.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)
    return value


def progress(ctx, stage):
    """Append one stage row to the receipt, durably, reading no sysfs and
    changing no product state: locates a stall between two full records."""
    directory = private_directory(ctx.path('/data/system/platform'))
    value = ctx.json('/data/system/platform/suspend-last.json', {})
    if not isinstance(value, dict):
        return
    value.setdefault('stages', []).append({'stage': stage, 'time': time.time(),
                                           'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME)})
    value['stages'] = value['stages'][-32:]
    atomic_json(directory / 'suspend-last.json', value, durable=True)


def application_rpc(ctx, operation, **fields):
    """Bounded explicit application acknowledgement, never just process liveness."""
    request = {'version': 1, 'id': 6582, 'command': {'op': operation, **fields}}
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(5)
        client.connect(str(ctx.path('/run/reborn/control.sock')))
        client.sendall((json.dumps(request) + '\n').encode())
        response = b''
        while b'\n' not in response and len(response) <= 131072:
            block = client.recv(4096)
            if not block:
                break
            response += block
    if len(response) > 131072:
        raise ValueError('reborn_sleep_response_too_large')
    answer = json.loads(response.split(b'\n')[0])
    if not isinstance(answer, dict) or answer.get('id') != 6582:
        raise ValueError('reborn_sleep_acknowledgement_failed')
    if answer.get('ok') is not True:
        error = answer.get('error')
        raise ValueError(error if isinstance(error, str) and len(error) <= 256 else 'reborn_sleep_acknowledgement_failed')
    value = answer.get('result')
    if not isinstance(value, dict):
        raise ValueError('reborn_sleep_acknowledgement_invalid')
    return value


def main():
    value = record(Context(), sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else None,
                   sys.argv[3] if len(sys.argv) > 3 else None)
    if value.get('error'):
        raise SystemExit(1)
