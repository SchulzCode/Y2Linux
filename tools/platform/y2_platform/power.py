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
    value = ctx.json(POLICY, ctx.json('/etc/y2linux/power-policy.json', {}))
    # No derived or fallback voltage thresholds. A reference records who/what
    # qualified the policy; software cannot certify that physical evidence.
    if value.get('schema') not in (1, 2) or value.get('enabled') is not True:
        return None, 'thresholds_disabled_PHYSICAL_GATE'
    keys = ('critical_uv', 'low_uv', 'recover_uv', 'critical_samples', 'grace_seconds')
    if any(type(value.get(k)) is not int for k in keys):
        return None, 'invalid_policy'
    if not (0 < value['critical_uv'] < value['low_uv'] < value['recover_uv'] and
            1 <= value['critical_samples'] <= 60 and 3 <= value['grace_seconds'] <= 30 and
            isinstance(value.get('qualification_reference') or value.get('source'), str) and
            1 <= len(value.get('qualification_reference') or value.get('source')) <= 256):
        return None, 'invalid_policy'
    if value['schema'] == 2 and not (
            all(type(value.get(k)) is int for k in ('warning_soc', 'critical_soc', 'shutdown_soc')) and
            0 <= value['shutdown_soc'] < value['critical_soc'] < value['warning_soc'] <= 50):
        return None, 'invalid_policy'
    return value, None


class LowBattery:
    def __init__(self):
        self.state, self.samples = 'Unavailable', 0

    def observe(self, policy, voltage, present, soc=None):
        if policy is None:
            self.state, self.samples = 'Unavailable', 0
            return self.state
        if present != 1 or voltage is None or voltage <= 0:
            self.state, self.samples = 'Unavailable', 0
            return self.state
        # USB presence does not prove positive net pack current. The same
        # qualified voltage guard applies while connected to a source.
        soc = soc or {}
        percent = soc.get('percent')
        valid_soc = type(percent) is int and 0 <= percent <= 100
        trustworthy = valid_soc and soc.get('confidence') in ('hardware', 'calibrated_estimate')
        # F36: on a charger the shown value never falls (battery.Soc.track), so a
        # low one is a start value from before the charge; the voltage guard
        # alone covers a source weaker than the load.
        charging = soc.get('charging') is True or soc.get('status') == 'Charging'
        soc_shutdown = (policy.get('schema') == 2 and trustworthy and not charging and
                        percent <= policy['shutdown_soc'])
        if voltage <= policy['critical_uv'] or soc_shutdown:
            self.samples += 1
            self.state = 'Critical'
            if self.samples >= policy['critical_samples']:
                self.state = 'ShutdownPending'
        else:
            self.samples = 0
            if policy.get('schema') == 2 and valid_soc and percent <= policy['critical_soc']:
                self.state = 'Critical'
            elif voltage <= policy['low_uv'] or (policy.get('schema') == 2 and valid_soc and percent <= policy['warning_soc']):
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
        self.watchdog = None
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

    def wait_seconds(self, next_sample_ns):
        # Socket activity wakes accept immediately. Only observation and a
        # pending shutdown need timed wakes; polling four times per second
        # provides neither faster requests nor better battery observations.
        deadline = next_sample_ns
        if self.intent and self.intent['state'] == 'ShutdownPending':
            deadline = min(deadline, self.intent['deadline_monotonic_ns'])
        return max(0, deadline - self.clock()) / 10**9

    def execute(self):
        # Only the actual daemon entry point calls this; fixture contexts can
        # never signal a host process or execute host power commands.
        self.intent['state'] = 'Executing'
        self.publish()
        shutdown_trail(self.ctx, 'execute %s %s' % (self.intent['action'], self.intent['reason']))
        results = {'watchdog': self.arm_watchdog()}
        shutdown_trail(self.ctx, 'watchdog', results['watchdog'])
        results['long_press_reset'] = self.long_press_reset_off()
        shutdown_trail(self.ctx, 'long_press_reset', results['long_press_reset'])
        results['journal_pending'] = self.step(
            'journal_pending', ['/usr/sbin/y2-platform', 'boot-stage', 'shutdown_pending'], STAGE_SECONDS)
        # Reborn normally ends on its dark frame with the backlight already off
        # before it acknowledges. If it never did (hung, crashed, deadline
        # reached) the light goes out before the kill: dropping DRM master would
        # otherwise restore the stale fbdev/console buffer on a lit panel.
        acknowledgement = self.intent.get('acknowledgement') or {}
        if acknowledgement.get('outcome') != 'Ready':
            results['backlight_early'] = backlight_off(self.ctx)
        results['S90reborn'] = self.step('S90reborn', ['/etc/init.d/S90reborn', 'stop'], 3)
        # Stop/reap failure cannot extend the battery grace indefinitely.
        self.begin('kill')
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
        # Reborn presents its shutdown transition, ends on a black frame and
        # turns the backlight off before acknowledging. If it crashed, hung or
        # was killed, the DRM last-close restores the fbdev buffer (stale,
        # console or white). Never hand init a lit panel.
        self.begin('backlight')
        results['backlight'] = backlight_off(self.ctx)
        # #53: a clock set by hand (or from the network) survives a restart
        # to the minute instead of to the last hourly record.
        results['time_remember'] = self.step(
            'time_remember', ['/usr/sbin/y2-platform', 'time-remember'], 2)
        results['sync'] = self.step('sync', ['/bin/busybox', 'sync'], 5)
        # F39: 5 shutdowns stalled the whole system in or right after the
        # radio stop (Wi-Fi connected, Bluetooth paging). It now runs after the
        # sync, one radio per step; a stall there leaves shutdown_complete
        # unwritten and ends in the watchdog reset.
        for name in RADIO_STOP:
            results['S41_' + name] = self.step(
                'S41_' + name, ['/etc/init.d/S41y2-connectivity', 'stop-' + name], 3)
            if name == 'wifi':
                # Blocking y2-wifi removes the card in a kernel worker after
                # the write returned; wait for it so a stall names this step.
                results['S41_wifi_removed'] = self.step('S41_wifi_removed', None, 2)
        if results['sync']['ok']:
            results['journal_complete'] = self.step(
                'journal_complete', ['/usr/sbin/y2-platform', 'boot-stage', 'shutdown_complete'], STAGE_SECONDS)
        self.intent['stop_results'] = {k: {'ok': v['ok'], 'reason': v['reason']} for k, v in results.items()}
        self.publish()
        # BusyBox init owns rcK, unmount and final reboot(2). Never use -f.
        result = self.step('init_' + self.intent['action'], ['/bin/busybox', self.intent['action']], 2)
        if not result['ok']:
            self.intent.update(state='Failed', failure='init_shutdown_request_failed')
            self.publish()
        # The watchdog stays armed and is fed no more (this daemon's exit at
        # rcK closes it without 'V': one last ping, still running). rcK,
        # unmount and reboot(2) take ~5 s and reboot(2) stops it (mtk_wdt
        # stop_on_reboot); a stall there, or a failed request, ends in its reset.
        return result

    def begin(self, name):
        shutdown_trail(self.ctx, 'begin ' + name)
        if self.watchdog is not None:
            try:
                os.write(self.watchdog, b'.')
            except OSError:
                pass

    def step(self, name, argv, timeout):
        """One bounded shutdown step between pmsg 'begin' and result lines."""
        self.begin(name)
        result = (self.ctx.command(argv, timeout=timeout) if argv else
                  wait_gone(self.ctx, '/sys/class/net/wlan0', timeout))
        shutdown_trail(self.ctx, name, result)
        return result

    def arm_watchdog(self):
        """F39: arm the SoC watchdog (31 s) for the whole shutdown.

        Its reset completes the requested transition whatever stalls: a restart
        (update: LK still finds MISC boot-recovery, written by `prepare` before
        the request, as after the F34 recovery watchdog), or for a power-off the
        loader's reason 3: charging mode on a charger, otherwise off."""
        if self.watchdog is not None:
            return {'ok': True, 'reason': None}
        failure = None
        if self.intent['action'] == 'poweroff':
            try:
                with open(self.ctx.path(WATCHDOG_POWERS_OFF), 'w') as f:
                    f.write('1\n')
            except OSError:
                # Old kernel or no RGU mapping: the reset restarts instead.
                failure = 'powers_off_unavailable'
        try:
            self.watchdog = os.open(self.ctx.path('/dev/watchdog'),
                                    os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        except OSError:
            return {'ok': False, 'reason': 'watchdog_unavailable'}
        return {'ok': failure is None, 'reason': failure}

    def long_press_reset_off(self):
        """F57 (496f54ad): Power still held through the 8-14 s power-off let
        the PMIC long-press reset cut it ~3 s after Reborn's ack. With the
        watchdog armed as the backstop, turn that reset off until the next
        start (the preloader sets it again). Without the watchdog it stays the
        only escape from a stalled shutdown. Never fails the shutdown."""
        if self.watchdog is None:
            return {'ok': False, 'reason': 'kept_without_watchdog'}
        try:
            fd = os.open(self.ctx.path(LONG_PRESS_RESET), os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        except OSError:
            return {'ok': False, 'reason': 'long_press_reset_unavailable'}   # older kernel
        try:
            os.write(fd, b'0\n')
        except OSError:
            return {'ok': False, 'reason': 'long_press_reset_failed'}
        finally:
            os.close(fd)
        return {'ok': True, 'reason': None}


# F39: the radio stop, one step per sub-command of the S41 script.
RADIO_STOP = ('wifi', 'bluetooth', 'daemon')
WATCHDOG_POWERS_OFF = '/sys/class/power_supply/BAT0/watchdog_powers_off'
LONG_PRESS_RESET = '/sys/firmware/y2_pmic/long_press_reset'


def wait_gone(ctx, name, timeout):
    deadline = time.monotonic() + timeout
    while ctx.path(name).exists():
        if time.monotonic() >= deadline:
            return {'ok': False, 'reason': 'still_present', 'output': None}
        time.sleep(.1)
    return {'ok': True, 'reason': None, 'output': None}


# F39 (471831b6, c14a3860): 2 of 55 update restarts lost these journal writes
# and one never restarted. A stage write is a Python start and an fsync, which a
# busy shutdown stretches past 1 s. Every step also goes to pstore pmsg (RAM, it
# cannot stall); the next boot's retained capture or the recovery keeps it.
STAGE_SECONDS = 3


def shutdown_trail(ctx, step, result=None):
    """One pmsg line per shutdown step; best effort, never raises."""
    line = '%.2f shutdown %s' % (time.monotonic(), step)
    if result is not None:
        line += ' ok' if result.get('ok') else ' ' + str(result.get('reason'))
    try:
        fd = os.open(ctx.path('/dev/pmsg0'), os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_CLOEXEC)
    except OSError:
        return
    try:
        os.write(fd, (line + '\n').encode())
    except OSError:
        pass
    finally:
        os.close(fd)


def backlight_off(ctx):
    """Power down every panel backlight; bounded, never raises."""
    nodes = [d / 'bl_power' for d in ctx.glob('/sys/class/backlight/*')]
    nodes = [n for n in nodes if n.exists()]
    if not nodes:
        return {'ok': False, 'reason': 'no_backlight'}
    failed = 0
    for node in nodes:
        try:
            with open(node, 'w') as f:
                f.write('4\n')
        except OSError:
            failed += 1
    return {'ok': not failed, 'reason': 'write_failed' if failed else None}


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
    from .power_journal import PowerJournal
    journal = PowerJournal(ctx)
    battery = LowBattery()
    from .battery import UI_STATE, Soc, load_profile, sample
    soc = Soc(load_profile(ctx), ctx.json(UI_STATE, None))
    ui_saved = None
    policy, reason = load_policy(ctx)
    running = [True]
    signal.signal(signal.SIGTERM, lambda *_: running.__setitem__(0, False))
    signal.signal(signal.SIGINT, lambda *_: running.__setitem__(0, False))
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
        server.bind(str(address))
        os.chmod(address, 0o600)
        server.listen(4)
        next_sample = 0
        while running[0]:
            if coordinator.clock() >= next_sample:
                next_sample = coordinator.clock() + 10**9
                # SOC uses physical elapsed time including suspend. Keep the
                # shutdown grace/sampling scheduler on its existing clock.
                battery_status = sample(ctx, soc)
                voltage = battery_status['voltage_uv']
                # 28: keep the displayed percentage across a restart (stock
                # RTC spare register); written only when it changes.
                ui_state = soc.state()
                if ui_state is not None and ui_state != ui_saved:
                    try:
                        atomic_json(ctx.path(UI_STATE), ui_state, durable=True)
                        ui_saved = ui_state
                    except OSError:
                        pass
                state = battery.observe(policy, voltage, ctx.integer('/sys/class/power_supply/BAT0/present'), battery_status)
                if state == 'ShutdownPending' and not coordinator.intent:
                    coordinator.request('poweroff', 'low_battery', policy['grace_seconds'])
                journal.observe(state, battery_status, coordinator.intent)
                atomic_json(runtime / 'power.json', {'schema': 1, 'boot_id': coordinator.boot_id,
                            'monotonic_ns': time.monotonic_ns(), 'state': state, 'voltage_uv': voltage,
                            'reason': reason, 'thresholds_enabled': policy is not None,
                            'battery': battery_status,
                            'qualification_reference': policy.get('qualification_reference') if policy else None,
                            'shutdown': coordinator.intent})
            if coordinator.due():
                journal.observe(state, battery_status, coordinator.intent, event='shutdown_execute')
                coordinator.execute()
            wait = coordinator.wait_seconds(next_sample)
            if not wait:
                continue
            server.settimeout(wait)
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
