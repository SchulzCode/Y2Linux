"""Durable suspend receipts, called only before entry or after Linux returns."""
# SPDX-License-Identifier: GPL-2.0-only
from .common import Context, atomic_json, read
from .boot import private_directory
from .sleep import transition, wake_irqs
import os
import json
import socket
import sys
import time


def record(ctx, stage, result=None, reason=None):
    directory = private_directory(ctx.path('/data/system/platform'))
    path = directory / 'suspend-last.json'
    value = ctx.json('/data/system/platform/suspend-last.json', {})
    if not isinstance(value, dict):
        value = {}
    if stage in ('requested', 'quiescing_radios') or stage.startswith('refused_'):
        value = {'schema': 1, 'requested_mode': 'deep',
                 'request_boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'started': time.time(), 'stages': []}
    value.update(stage=stage, result=result, updated=time.time(),
                 current_boot_id=ctx.read('/proc/sys/kernel/random/boot_id'),
                 persistent_kernel=ctx.read('/sys/firmware/y2_pm/state'),
                 usb_restore={'controllers': {p.parent.name: read(p, limit=8192)
                     for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/status')},
                     'udcs': {p.name: read(p/'state') for p in ctx.glob('/sys/class/udc/*')}},
                 radio_restore=ctx.read('/sys/devices/platform/18070000.connectivity/status'))
    if stage == 'kernel_suspend':
        value['wake_irqs_before'] = wake_irqs(ctx.read('/proc/interrupts'))
    elif stage == 'restoring_radios':
        value['wake_irqs_after'] = wake_irqs(ctx.read('/proc/interrupts'))
    value.setdefault('stages', []).append({'stage': stage, 'time': time.time(), 'result': result})
    value['stages'] = value['stages'][-32:]
    if stage == 'complete_result_0' and result == 0:
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(2)
                client.connect(str(ctx.path('/run/reborn/control.sock')))
                client.sendall(b'{"version":1,"id":6582,"command":{"op":"status"}}\n')
                response = b''
                while b'\n' not in response and len(response) < 131072:
                    block = client.recv(4096)
                    if not block:
                        break
                    response += block
                answer = json.loads(response.split(b'\n')[0])
                value['reborn_restore'] = answer.get('ok') is True and answer.get('id') == 6582
        except (OSError, ValueError):
            value['reborn_restore'] = False
        if not value['reborn_restore']:
            value['result'] = 1
            value['error'] = 'reborn_resume_probe_failed'
    value['product'] = transition(value, stage, value['result'], value['current_boot_id'], reason)
    if stage == 'complete_result_0' and value['product']['state'] != 'restored':
        value['result'] = 1
        value['error'] = value['product']['reason']
    if value['product']['state'] == 'restored':
        marker = ctx.path('/sys/firmware/y2_pm/stage')
        if marker.exists():
            marker.write_text('REBORN_READY\n')
    atomic_json(path, value, durable=True)
    parent = os.open(directory.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)
    return value


def main():
    value = record(Context(), sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else None,
                   sys.argv[3] if len(sys.argv) > 3 else None)
    if value.get('error'):
        raise SystemExit(1)
