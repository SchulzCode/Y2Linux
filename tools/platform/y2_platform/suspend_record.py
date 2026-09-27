"""Durable suspend receipts, called only before entry or after Linux returns."""
# SPDX-License-Identifier: GPL-2.0-only
from .common import Context, atomic_json
import os
import json
import socket
import sys
import time


def record(ctx, stage, result=None):
    directory = ctx.path('/data/system/platform')
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if directory.is_symlink():
        raise ValueError('unsafe_suspend_receipt_directory')
    path = directory / 'suspend-last.json'
    value = ctx.json('/data/system/platform/suspend-last.json', {})
    if stage == 'quiescing_radios':
        value = {'schema': 1, 'requested_mode': 'deep',
                 'request_boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                 'started': time.time(), 'stages': []}
    value.update(stage=stage, result=result, updated=time.time(),
                 current_boot_id=ctx.read('/proc/sys/kernel/random/boot_id'),
                 persistent_kernel=ctx.read('/sys/firmware/y2_pm/state'),
                 usb_restore=ctx.read('/run/y2/usb-state'),
                 radio_restore=ctx.read('/sys/devices/platform/18070000.connectivity/status'))
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
        if value['reborn_restore']:
            marker = ctx.path('/sys/firmware/y2_pm/stage')
            if marker.exists():
                marker.write_text('REBORN_READY\n')
        else:
            value['result'] = 1
            value['error'] = 'reborn_resume_probe_failed'
    atomic_json(path, value, durable=True)
    parent = os.open(directory.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)
    return value


def main():
    value = record(Context(), sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else None)
    if value.get('error'):
        raise SystemExit(1)
