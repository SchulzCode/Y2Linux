"""USB-only owner SSH, started only after the kernel CRNG is initialized."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import json
import os
import signal
import subprocess
import time
from .boot import private_directory
from .common import atomic_json


def entropy_ready(getrandom=os.getrandom):
    try:
        return len(getrandom(1, os.GRND_NONBLOCK)) == 1
    except (BlockingIOError, OSError):
        return False


def prerequisites(ctx, entropy):
    if not ctx.read('/root/.ssh/authorized_keys'):
        return 'Unavailable', 'no_owner_authorized_key'
    if not entropy:
        return 'Starting', 'kernel_crng_not_ready'
    result = ctx.command(['ip', '-j', '-4', 'addr', 'show', 'dev', 'usb0'], timeout=1)
    try:
        interfaces = json.loads(result['output'] or '')
        if not any(a.get('local') == '10.42.0.1' and a.get('prefixlen') == 24
                   for i in interfaces if i.get('ifname') == 'usb0' for a in i.get('addr_info', [])):
            return 'Starting', 'usb_address_not_ready'
    except (ValueError, TypeError, AttributeError):
        return 'Starting', 'usb_interface_not_ready'
    if ctx.read('/data/.y2data-schema') != '1':
        return 'Failed', 'data_schema_unavailable'
    return 'Ready', 'usb_address_key_and_crng_ready'


def serve(ctx):
    runtime = private_directory(ctx.path('/run/y2'))
    lock = os.open(runtime/'ssh.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    running = [True]
    signal.signal(signal.SIGTERM, lambda *_: running.__setitem__(0, False))
    signal.signal(signal.SIGINT, lambda *_: running.__setitem__(0, False))
    child = None
    try:
        while running[0]:
            ready, reason = prerequisites(ctx, entropy_ready())
            if child is None and ready == 'Ready':
                # Fixed policy, never credentials/configuration-derived shell text.
                child = subprocess.Popen(['/usr/sbin/dropbear', '-F', '-E', '-R', '-s', '-g', '-j', '-k',
                                          '-p', '10.42.0.1:22', '-l', 'usb0', '-K', '10', '-I', '600'],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL, start_new_session=True)
                ready, reason = 'Starting', 'listener_starting'
            elif child is not None:
                if child.poll() is not None:
                    raise RuntimeError('ssh_listener_exited')
                # Child success is not readiness: require its bound listening address.
                listening = any('01002A0A:0016' in line and line.split()[3] == '0A'
                                for line in (ctx.read('/proc/net/tcp') or '').splitlines()[1:]
                                if len(line.split()) > 3)
                ready, reason = ('Ready', 'usb_bound_key_only_listener') if listening and ready == 'Ready' else ('Starting', reason)
            atomic_json(runtime/'ssh.json', {'schema': 1, 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                        'monotonic_s': time.monotonic(), 'state': ready, 'reason': reason,
                        'bind': '10.42.0.1:22', 'interface': 'usb0', 'authentication': 'owner_public_key',
                        'sftp': ctx.path('/usr/libexec/y2-sftp-server').is_file(),
                        'pid': child.pid if child else None})
            time.sleep(1)
    finally:
        if child and child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
        os.close(lock)
