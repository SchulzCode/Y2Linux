"""UI-independent shutdown policy. Electrical thresholds are owner-qualified data."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import stat
import struct
import time
import uuid
from .boot import private_directory
from .common import atomic_json

SOCKET = '/run/y2/power.sock'
POLICY = '/data/system/platform/power-policy.json'


def load_policy(ctx):
    value = ctx.json(POLICY, {})
    # No derived or fallback voltage thresholds. A reference records who/what
    # qualified the policy; software cannot certify that physical evidence.
    if value.get('schema') != 1 or value.get('enabled') is not True:
        return None, 'thresholds_disabled_PHYSICAL_GATE'
    keys = ('critical_uv', 'low_uv', 'recover_uv', 'critical_samples', 'grace_seconds')
    if any(type(value.get(k)) is not int for k in keys):
        return None, 'invalid_policy'
    if not (0 < value['critical_uv'] < value['low_uv'] < value['recover_uv'] and
            1 <= value['critical_samples'] <= 60 and 3 <= value['grace_seconds'] <= 30 and
            isinstance(value.get('qualification_reference'), str) and
            1 <= len(value['qualification_reference']) <= 256):
        return None, 'invalid_policy'
    return value, None


class LowBattery:
    def __init__(self):
        self.state, self.samples = 'Unavailable', 0

    def observe(self, policy, voltage, present):
        if policy is None:
            self.state, self.samples = 'Unavailable', 0
            return self.state
        if present != 1 or voltage is None or voltage <= 0:
            self.state, self.samples = 'Unavailable', 0
            return self.state
        # USB presence does not prove positive net pack current. The same
        # qualified voltage guard applies while connected to a source.
        if voltage <= policy['critical_uv']:
            self.samples += 1
            self.state = 'Critical'
            if self.samples >= policy['critical_samples']:
                self.state = 'ShutdownPending'
        else:
            self.samples = 0
            if voltage <= policy['low_uv']:
                self.state = 'Low'
            elif voltage >= policy['recover_uv'] or self.state == 'Unavailable':
                self.state = 'Normal'
            elif self.state in ('Critical', 'ShutdownPending'):
                self.state = 'Low'
        return self.state


def process_identity(ctx, name, expected):
    pid = ctx.integer('/run/reborn/' + name + '.pid')
    if pid is None or pid <= 1:
        return None
    try:
        if str(ctx.path(f'/proc/{pid}/exe').resolve()) != str(ctx.path(expected).resolve()):
            return None
        value = ctx.read(f'/proc/{pid}/stat')
        return {'pid': pid, 'start_ticks': int(value.rsplit(') ', 1)[1].split()[19])}
    except (OSError, ValueError, IndexError, AttributeError):
        return None


class Coordinator:
    def __init__(self, ctx, clock=time.monotonic_ns):
        self.ctx, self.clock = ctx, clock
        self.boot_id = ctx.read('/proc/sys/kernel/random/boot_id')
        previous = ctx.json('/run/y2/shutdown.json', {})
        self.intent = previous if (previous.get('boot_id') == self.boot_id and
                                  previous.get('state') in ('ShutdownPending', 'Executing', 'Failed') and
                                  previous.get('action') in ('poweroff', 'reboot') and
                                  type(previous.get('deadline_monotonic_ns')) is int and
                                  isinstance(previous.get('id'), str) and
                                  'client' in previous and 'acknowledgement' in previous) else None
        if self.intent and self.intent['state'] == 'Executing':
            # Continue the already requested transition after a daemon crash,
            # retaining its original deadline. No new boot is auto-rebooted.
            self.intent['state'] = 'ShutdownPending'

    def publish(self):
        private_directory(self.ctx.path('/run/y2'))
        atomic_json(self.ctx.path('/run/y2/shutdown.json'), self.intent)

    def request(self, action, reason, grace=10):
        if action not in ('poweroff', 'reboot') or reason not in ('user', 'low_battery', 'update', 'service'):
            raise ValueError('invalid_shutdown_request')
        if not 3 <= grace <= 30:
            raise ValueError('invalid_grace')
        if self.intent:
            if self.intent['action'] != action:
                raise ValueError('different_shutdown_already_pending')
            return self.intent
        now = self.clock()
        self.intent = {'schema': 1, 'id': str(uuid.uuid4()), 'boot_id': self.boot_id,
                       'state': 'ShutdownPending', 'action': action, 'reason': reason,
                       'requested_monotonic_ns': now, 'deadline_monotonic_ns': now + grace * 10**9,
                       'client': process_identity(self.ctx, 'process', '/usr/bin/reborn'),
                       'acknowledgement': None, 'failure': None}
        self.publish()
        return self.intent

    def ack(self, request_id, pid, outcome):
        if not self.intent or self.intent['state'] != 'ShutdownPending' or self.intent['id'] != request_id:
            raise ValueError('stale_shutdown_ack')
        current = process_identity(self.ctx, 'process', '/usr/bin/reborn')
        if not current or current != self.intent['client'] or current['pid'] != pid:
            raise ValueError('ack_client_identity_changed')
        if outcome not in ('Ready', 'Failed'):
            raise ValueError('invalid_ack_outcome')
        self.intent['acknowledgement'] = {'outcome': outcome, 'monotonic_ns': self.clock()}
        self.publish()
        return self.intent

    def due(self):
        if not self.intent or self.intent['state'] != 'ShutdownPending':
            return False
        ack = self.intent['acknowledgement']
        return (self.clock() >= self.intent['deadline_monotonic_ns'] or
                bool(ack and ack['outcome'] == 'Ready'))

    def execute(self):
        # Only the actual daemon entry point calls this; fixture contexts can
        # never signal a host process or execute host power commands.
        self.intent['state'] = 'Executing'
        self.publish()
        results = {'journal_pending': self.ctx.command(
            ['/usr/sbin/y2-platform', 'boot-stage', 'shutdown_pending'], timeout=1)}
        for name in ('S05reborn', 'S41y2-connectivity'):
            results[name] = self.ctx.command(['/etc/init.d/' + name, 'stop'], timeout=3)
        # Stop/reap failure cannot extend the battery grace indefinitely.
        for name, executable in (('process', '/usr/bin/reborn'),
                                 ('supervisor', '/usr/libexec/reborn-supervise')):
            identity = process_identity(self.ctx, name, executable)
            if identity and self.ctx.root == Path('/'):
                try:
                    fd = os.pidfd_open(identity['pid'])
                    try:
                        if process_identity(self.ctx, name, executable) == identity:
                            signal.pidfd_send_signal(fd, signal.SIGKILL)
                    finally:
                        os.close(fd)
                except (OSError, AttributeError):
                    pass
        results['sync'] = self.ctx.command(['/bin/busybox', 'sync'], timeout=5)
        if results['sync']['ok']:
            results['journal_complete'] = self.ctx.command(
                ['/usr/sbin/y2-platform', 'boot-stage', 'shutdown_complete'], timeout=1)
        self.intent['stop_results'] = {k: {'ok': v['ok'], 'reason': v['reason']} for k, v in results.items()}
        self.publish()
        # BusyBox init owns rcK, unmount and final reboot(2). Never use -f.
        result = self.ctx.command(['/bin/busybox', self.intent['action']], timeout=2)
        if not result['ok']:
            self.intent.update(state='Failed', failure='init_shutdown_request_failed')
            self.publish()
        return result


def rpc(ctx, request):
    data = (json.dumps(request) + '\n').encode()
    if len(data) > 2048:
        raise ValueError('request_too_large')
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(1)
        client.connect(str(ctx.path(SOCKET)))
        client.sendall(data)
        answer = bytearray()
        while b'\n' not in answer:
            part = client.recv(4096)
            if not part or len(answer) + len(part) > 8192:
                raise ValueError('invalid_power_response')
            answer.extend(part)
        result = json.loads(answer)
        if not result.get('ok'):
            raise ValueError(result.get('error', 'power_request_failed'))
        return result['result']


def serve(ctx):
    runtime = private_directory(ctx.path('/run/y2'))
    lock = os.open(runtime / 'power.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    address = ctx.path(SOCKET)
    if address.exists():
        if not stat.S_ISSOCK(address.lstat().st_mode):
            raise ValueError('unsafe_power_socket_path')
        address.unlink()
    coordinator = Coordinator(ctx)
    battery = LowBattery()
    policy, reason = load_policy(ctx)
    running = [True]
    signal.signal(signal.SIGTERM, lambda *_: running.__setitem__(0, False))
    signal.signal(signal.SIGINT, lambda *_: running.__setitem__(0, False))
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
        server.bind(str(address))
        os.chmod(address, 0o600)
        server.listen(4)
        server.settimeout(.25)
        next_sample = 0
        while running[0]:
            if time.monotonic() >= next_sample:
                next_sample = time.monotonic() + 1
                voltage = ctx.integer('/sys/class/power_supply/BAT0/voltage_now')
                state = battery.observe(policy, voltage, ctx.integer('/sys/class/power_supply/BAT0/present'))
                if state == 'ShutdownPending' and not coordinator.intent:
                    coordinator.request('poweroff', 'low_battery', policy['grace_seconds'])
                atomic_json(runtime / 'power.json', {'schema': 1, 'boot_id': coordinator.boot_id,
                            'monotonic_ns': time.monotonic_ns(), 'state': state, 'voltage_uv': voltage,
                            'reason': reason, 'thresholds_enabled': policy is not None,
                            'qualification_reference': policy.get('qualification_reference') if policy else None,
                            'shutdown': coordinator.intent})
            if coordinator.due():
                coordinator.execute()
            try:
                client, _ = server.accept()
            except socket.timeout:
                continue
            with client:
                client.settimeout(.2)
                try:
                    pid, uid, _ = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                    if uid != 0:
                        raise ValueError('owner_privilege_required')
                    data = bytearray()
                    while b'\n' not in data:
                        part = client.recv(2049)
                        if not part or len(data) + len(part) > 2048:
                            raise ValueError('invalid_power_request')
                        data.extend(part)
                    request = json.loads(data)
                    if request.get('operation') == 'ack':
                        value = coordinator.ack(request['id'], pid, request['outcome'])
                    else:
                        value = coordinator.request(request.get('action'), request.get('reason', 'user'))
                    result = {'ok': True, 'result': value}
                except (ValueError, OSError, KeyError, TypeError, AttributeError) as error:
                    result = {'ok': False, 'error': str(error)}
                try:
                    client.sendall((json.dumps(result) + '\n').encode())
                except OSError:
                    pass
    address.unlink(missing_ok=True)
    os.close(lock)
