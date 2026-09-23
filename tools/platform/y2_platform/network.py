"""One DHCP lease lifecycle and truthful readiness above wpa_supplicant."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import ipaddress
import json
import os
import re
import socket
import time
import uuid
from .boot import private_directory
from .common import atomic_json
from .observe import wifi


def publish(ctx, name, value):
    private_directory(ctx.path('/run/y2'))
    value.update(schema=1, boot_id=ctx.read('/proc/sys/kernel/random/boot_id'), monotonic_s=time.monotonic())
    atomic_json(ctx.path('/run/y2/' + name + '.json'), value)
    return value


def lease_event(ctx, event):
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'dhcp.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        return _lease_event(ctx, event)
    finally:
        os.close(fd)


def _lease_event(ctx, event):
    if event not in ('CONNECTED', 'DISCONNECTED', 'SERVICE_RESTART'):
        raise ValueError('invalid_wifi_event')
    epoch = str(uuid.uuid4())
    runtime = private_directory(ctx.path('/run/y2'))
    (runtime / 'wifi-epoch').write_text(epoch + '\n')
    (runtime / 'dns.json').unlink(missing_ok=True)
    old = ctx.json('/run/y2/wifi-events.json', {})
    count = old.get(event.lower(), 0)
    count = count if isinstance(count, int) and count >= 0 else 0
    old[event.lower()] = count + 1
    old['last_event'] = event
    publish(ctx, 'wifi-events', old)
    # Leases never survive a new association/service epoch. USB's fixed address
    # and routes are outside this interface-scoped lifecycle.
    for argv in (['ip', '-4', 'address', 'flush', 'dev', 'wlan0', 'scope', 'global'],
                 ['ip', '-4', 'route', 'flush', 'dev', 'wlan0']):
        ctx.command(argv)
    resolv(ctx, [])
    return publish(ctx, 'dhcp', {'epoch': epoch, 'state': 'AcquiringIP' if event == 'CONNECTED' else 'Unavailable',
                    'error': None, 'address': None})


def resolv(ctx, servers):
    # /etc/resolv.conf is a production-owned symlink into /run; only Wi-Fi
    # supplies DNS in this product. Never import search strings from DHCP.
    path = ctx.path('/run/y2/resolv.conf')
    temporary = path.with_name('.resolv.conf.' + uuid.uuid4().hex)
    temporary.write_text(''.join(f'nameserver {server}\n' for server in servers) +
                         'options timeout:2 attempts:1\n')
    os.chmod(temporary, 0o644)
    os.replace(temporary, path)


def dhcp_event(ctx, event, env):
    if env.get('interface') != 'wlan0':
        raise ValueError('dhcp_interface_not_owned')
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'dhcp.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        epoch = ctx.read('/run/y2/wifi-epoch')
        if not epoch or env.get('Y2_DHCP_GENERATION') != epoch:
            raise ValueError('stale_dhcp_epoch')
        (runtime / 'dns.json').unlink(missing_ok=True)
        state = {'epoch': epoch, 'state': 'AcquiringIP', 'error': None, 'address': None}
        if event in ('deconfig', 'nak', 'leasefail'):
            for argv in (['ip', '-4', 'address', 'flush', 'dev', 'wlan0', 'scope', 'global'],
                         ['ip', '-4', 'route', 'flush', 'dev', 'wlan0']):
                ctx.command(argv)
            resolv(ctx, [])
            if event != 'deconfig':
                state.update(state='Failed', error='dhcp_timeout' if event == 'leasefail' else 'dhcp_nak')
        elif event in ('bound', 'renew'):
            address = ipaddress.IPv4Address(env.get('ip', ''))
            interface = ipaddress.IPv4Interface(str(address) + '/' + env.get('subnet', ''))
            if address.is_unspecified or address.is_multicast or address.is_loopback:
                raise ValueError('invalid_dhcp_address')
            servers = [str(ipaddress.IPv4Address(s)) for s in env.get('dns', '').split()[:3]]
            gateways = [str(ipaddress.IPv4Address(s)) for s in env.get('router', '').split()[:3]]
            routes = []
            # RFC3442: classless routes supersede the router option. Bound the
            # list and restrict syntax to parsed addresses, never shell text.
            static = env.get('staticroutes', '').split()
            if static:
                if len(static) > 32 or len(static) % 2:
                    raise ValueError('invalid_classless_routes')
                routes = [(str(ipaddress.IPv4Network(static[i], strict=False)),
                           str(ipaddress.IPv4Address(static[i + 1]))) for i in range(0, len(static), 2)]
            else:
                routes = [('default', gateway) for gateway in gateways[:1]]
            commands = [['ip', '-4', 'address', 'flush', 'dev', 'wlan0', 'scope', 'global'],
                        ['ip', '-4', 'route', 'flush', 'dev', 'wlan0'],
                        ['ip', 'link', 'set', 'dev', 'wlan0', 'up'],
                        ['ip', '-4', 'address', 'add', str(interface), 'dev', 'wlan0']]
            commands += [['ip', '-4', 'route', 'replace', destination] +
                         ([] if gateway == '0.0.0.0' else ['via', gateway]) +
                         ['dev', 'wlan0', 'metric', '100'] for destination, gateway in routes]
            for argv in commands:
                if not ctx.command(argv)['ok']:
                    state.update(state='Failed', error='lease_configuration_failed')
                    publish(ctx, 'dhcp', state)
                    raise ValueError('lease_configuration_failed')
            resolv(ctx, servers)
            state.update(state='Authenticated', address=str(address), dns_servers=servers,
                         error=None if servers else 'dns_unavailable', lease_seconds=env.get('lease'))
        else:
            raise ValueError('unknown_dhcp_event')
        return publish(ctx, 'dhcp', state)
    finally:
        os.close(fd)


def probe_dns(ctx, observed):
    if not observed['ip_addresses'] or not observed['default_route']:
        return None
    config = ctx.json('/etc/y2linux/network-policy.json', {})
    hostname = config.get('dns_probe_host', 'pool.ntp.org')
    if not isinstance(hostname, str) or not re.fullmatch(r'[A-Za-z0-9.-]{1,253}', hostname):
        raise ValueError('invalid_dns_probe_host')
    epoch = ctx.read('/run/y2/wifi-epoch')
    # Standard libc resolver in a bounded subprocess; no custom DNS protocol.
    answer = ctx.command(['/usr/bin/python3', '-I', '-c',
                          'import socket,sys; print(len(socket.getaddrinfo(sys.argv[1],None,socket.AF_INET)))',
                          hostname], timeout=3)
    after = wifi(ctx)
    if (ctx.read('/run/y2/wifi-epoch') != epoch or
            after.get('association_id') != observed.get('association_id') or
            after['ip_addresses'] != observed['ip_addresses']):
        return None
    ok = answer['ok'] and (answer['output'] or '').isdigit() and int(answer['output']) > 0
    return publish(ctx, 'dns', {'epoch': epoch, 'ok': ok, 'address': observed['ip_addresses'][0],
                   'bssid': observed.get('association_id'), 'probe_host': hostname,
                   'reason': None if ok else 'dns_unavailable'})


class EventMonitor:
    """Passive supplicant events. Never issues CONNECT or owns reconnect."""
    def __init__(self, ctx):
        self.ctx, self.sock, self.path = ctx, None, None
        self.generation = None

    def close(self):
        if self.sock:
            self.sock.close()
        if self.path:
            self.path.unlink(missing_ok=True)
        self.sock = self.path = None
        self.generation = None

    def poll(self):
        remote = self.ctx.path('/run/wpa_supplicant/wlan0')
        if not remote.exists():
            self.close()
            return
        try:
            meta = remote.stat()
            generation = (meta.st_ino, meta.st_ctime_ns)
            if self.generation is not None and generation != self.generation:
                self.close()
            if self.sock is None:
                self.path = self.ctx.path('/run/y2/wpa-events.sock')
                self.path.unlink(missing_ok=True)
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
                self.sock.settimeout(.2)
                self.sock.bind(str(self.path))
                self.sock.connect(str(remote))
                self.sock.send(b'ATTACH')
                if self.sock.recv(1024).strip() != b'OK':
                    raise ValueError('monitor_attach_failed')
                self.sock.setblocking(False)
                self.generation = generation
            for _ in range(16):
                try:
                    raw = self.sock.recv(4096).decode(errors='replace')
                except BlockingIOError:
                    break
                self.event(raw)
        except (OSError, ValueError):
            self.close()

    def event(self, text):
        error = None
        if 'CTRL-EVENT-SSID-TEMP-DISABLED' in text:
            error = 'wrong_credentials' if 'reason=WRONG_KEY' in text else 'authentication_failed'
        elif 'CTRL-EVENT-AUTH-REJECT' in text:
            error = 'authentication_rejected'
        elif 'CTRL-EVENT-NETWORK-NOT-FOUND' in text:
            error = 'ap_unavailable'
        elif 'CTRL-EVENT-CONNECTED' not in text:
            return
        old = self.ctx.json('/run/y2/wifi-failure.json', {})
        failures = old.get('failures', 0)
        publish(self.ctx, 'wifi-failure', {'error': error,
                'epoch': self.ctx.read('/run/y2/wifi-epoch'),
                'failures': (failures if isinstance(failures, int) else 0) + bool(error)})
