"""One DHCP lease lifecycle and truthful readiness above wpa_supplicant."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import ipaddress
import json
import os
import re
import select
import socket
import time
import uuid
from .boot import private_directory
from .common import atomic_json
from .observe import wifi


# Only a password verdict is kept until a connect. A rejected or failed
# association (CONN_FAILED, AUTH-REJECT) is no password on WPA2-PSK.
CREDENTIAL_ERRORS = ('wrong_credentials',)
LAST_FAILURE_KEYS = ('error', 'reason', 'epoch', 'failures', 'scan', 'scan_monotonic_s', 'boot_id', 'monotonic_s')
# F52 (aa96e00a): wpa_supplicant says WRONG_KEY for any 4-way handshake that
# ends early (events.c could_be_psk_mismatch), also one this player cut short
# (a Join, the sleep's radio off). A password is wrong only when one network
# fails this often with no connect between; a single failure is
# `handshake_failed`, one after our own disconnect is not counted.
WRONG_KEY_REPEAT = 2
# Durable Wi-Fi self-heal rows (F52/F53); never a network name or address.
JOURNAL = '/data/system/platform/wifi-journal.jsonl'
JOURNAL_LIMIT = 262144
# Supplicant events counted for reports: what it did while it sat
# DISCONNECTED (F52). Kinds and times only, never the event text.
EVENT_KINDS = ('CONNECTED', 'DISCONNECTED', 'SSID-TEMP-DISABLED', 'SSID-REENABLED', 'ASSOC-REJECT',
               'AUTH-REJECT', 'NETWORK-NOT-FOUND', 'SCAN-STARTED', 'SCAN-RESULTS', 'SCAN-FAILED',
               'AUTH-TIMEOUT')


def publish(ctx, name, value):
    private_directory(ctx.path('/run/y2'))
    value.update(schema=1, boot_id=ctx.read('/proc/sys/kernel/random/boot_id'), monotonic_s=time.monotonic())
    atomic_json(ctx.path('/run/y2/' + name + '.json'), value)
    return value


def journal(ctx, event, **fields):
    """Append one durable row (bounded, rotated once); False when /data is not ours."""
    from .power_journal import append_bounded
    from .sleep_diagnostics import data_ready
    try:
        if not data_ready(ctx):
            return False
        row = {'schema': 1, 'event': event, 'wall_time': time.time(), 'monotonic_s': time.monotonic(),
               'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'), **fields}
        append_bounded(ctx, JOURNAL, (json.dumps(row, separators=(',', ':')) + '\n').encode(),
                       JOURNAL_LIMIT, durable=True)
        return True
    except (OSError, ValueError, TypeError):
        return False


def wlan_switched_off(ctx):
    """True while Wi-Fi is soft-blocked: sleep or the owner turned it off."""
    for entry in sorted(ctx.path('/sys/class/rfkill').glob('rfkill*')):
        name = '/sys/class/rfkill/' + entry.name
        if ctx.read(name + '/type') == 'wlan' and ctx.read(name + '/soft') == '1':
            return True
    return False


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
    if old.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id'):
        old = {}
    active = old.get('association_active', old.get('last_event') == 'CONNECTED') is True
    # Action calls include supervisory cleanup as well as native link events.
    # Keep their historical raw counts, but count a real loss only once after
    # a known connection. Unknown queries never enter this lifecycle.
    old.setdefault('link_counts_since_s', time.monotonic())
    for name in ('real_disconnects', 'reconnects', 'radio_off_disconnects'):
        value = old.get(name, 0)
        old[name] = value if isinstance(value, int) and value >= 0 else 0
    if event == 'DISCONNECTED' and active:
        # Sleep switches the radio off: that is not a lost link (Beta 1
        # counted every sleep as a real disconnect).
        old['radio_off_disconnects' if wlan_switched_off(ctx) else 'real_disconnects'] += 1
    if event == 'CONNECTED' and not active and old.get('connected', 0):
        old['reconnects'] += 1
    old['association_active'] = event == 'CONNECTED'
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
    after = wifi(ctx, signal_cache=observed)
    if (ctx.read('/run/y2/wifi-epoch') != epoch or
            after.get('association_id') != observed.get('association_id') or
            after['ip_addresses'] != observed['ip_addresses'] or
            after.get('default_route') != observed.get('default_route')):
        return None
    ok = answer['ok'] and (answer['output'] or '').isdigit() and int(answer['output']) > 0
    return publish(ctx, 'dns', {'epoch': epoch, 'ok': ok, 'address': observed['ip_addresses'][0],
                   'bssid': observed.get('association_id'), 'probe_host': hostname,
                   'reason': None if ok else 'dns_unavailable'})


class DnsProbeSchedule:
    """Probe newly usable leases immediately; bound retries on the same lease.

    A confirmed lease is re-checked every five minutes, not every 30 s: each
    probe starts a Python resolver (≈260 ms CPU on the Y2) and a radio
    exchange. observe.DNS_FRESH_S keeps Online across one late re-check.
    """
    RETRY_S = 30
    CONFIRMED_S = 300

    def __init__(self):
        self.key = None
        self.deadline = 0

    def confirmed(self, now):
        self.deadline = now + self.CONFIRMED_S

    def due(self, observed, epoch, now):
        if (observed.get('association') != 'COMPLETED' or not epoch or
                not observed.get('ip_addresses') or not observed.get('default_route')):
            return False
        key = (epoch, observed.get('association_id'), tuple(observed['ip_addresses']))
        if key != self.key:
            self.key, self.deadline = key, 0
        if now < self.deadline:
            return False
        self.deadline = now + self.RETRY_S
        return True


def _channel(frequency):
    if 2412 <= frequency <= 2472:
        return (frequency - 2407) // 5
    if frequency == 2484:
        return 14
    if 5000 <= frequency <= 5900:
        return (frequency - 5000) // 5
    return None


def scan_evidence(ctx):
    """Why the saved network was "not found" (F13: Wi-Fi did not come back after
    a wake on one player, cause unproven): how many networks the last scan saw,
    whether a saved one was among them, its best signal and channel, and the
    regulatory country (country=00 scans channels 12/13 passively). Never a
    network name or address."""
    scan = ctx.command(['wpa_cli', '-i', 'wlan0', 'scan_results'], timeout=2)
    saved = ctx.command(['wpa_cli', '-i', 'wlan0', 'list_networks'], timeout=2)
    if not scan['ok'] or not saved['ok']:
        return {'available': False}
    names = {row[1] for row in (line.split('\t') for line in (saved['output'] or '').splitlines()[1:])
             if len(row) >= 2 and row[1]}
    rows = [row for row in (line.split('\t') for line in (scan['output'] or '').splitlines()[1:])
            if len(row) >= 3 and row[1].isdigit() and re.fullmatch(r'-?\d+', row[2])]
    result = {'available': True, 'networks': len(rows), 'saved_networks': len(names),
              'band_2ghz': sum(int(row[1]) < 3000 for row in rows),
              'band_5ghz': sum(int(row[1]) >= 5000 for row in rows)}
    hits = [row for row in rows if len(row) >= 5 and row[4] in names]
    result['saved_seen'] = bool(hits)
    if hits:
        best = max(hits, key=lambda row: int(row[2]))
        result.update(saved_signal_dbm=int(best[2]), saved_frequency_mhz=int(best[1]),
                      saved_channel=_channel(int(best[1])), saved_access_points=len(hits))
        # F13 (cb904ffb): "not found" with the saved network listed at -66 dBm.
        # scan_results is the BSS table (entries up to 180 s old): how old the
        # entry is, and whether its security matches the saved network's.
        result['saved_flags'] = ''.join(re.findall(r'\[[A-Z0-9+_-]{1,24}\]', best[3] if len(best) > 3 else ''))
        bss = ctx.command(['wpa_cli', '-i', 'wlan0', 'bss', best[0]], timeout=2)
        age = re.search(r'(?m)^age=(\d+)$', bss.get('output') or '') if bss['ok'] else None
        if age:
            result['saved_age_s'] = int(age.group(1))
        ids = [row[0] for row in (line.split('\t') for line in (saved['output'] or '').splitlines()[1:])
               if len(row) >= 2 and row[1] == best[4] and row[0].isdigit()]
        managements = set()
        for network in ids[:4]:
            answer = ctx.command(['wpa_cli', '-i', 'wlan0', 'get_network', network, 'key_mgmt'], timeout=2)
            value = (answer.get('output') or '').strip()
            if answer['ok'] and re.fullmatch(r'[A-Z0-9 _-]{1,64}', value):
                managements.add(value)
        result['saved_key_mgmt'] = sorted(managements)
    country = ctx.command(['wpa_cli', '-i', 'wlan0', 'get', 'country'], timeout=2)
    value = (country.get('output') or '').strip()
    if country['ok'] and re.fullmatch(r'[0-9A-Z]{2}', value):
        result['country'] = value
    return result


class EventMonitor:
    """Passive supplicant events. Never issues CONNECT or owns reconnect."""
    SCAN_EVIDENCE_S = 30
    def __init__(self, ctx):
        self.ctx, self.sock, self.path = ctx, None, None
        self.generation = None
        self.started = time.monotonic()
        self.counts, self.last = {}, {}
        self.last_disconnect = None
        self.interrupted_handshakes = 0
        # The network of the last connect: the one a self-heal selects.
        self.connected_network = None
        # wpa_supplicant's handshake timeout (≈10 s; how a wrong password ends
        # on the Y2) also disconnects locally; any other local disconnect cut
        # a handshake short and proves nothing about the password.
        self.timed_out = self.interrupted = False
        # A local disconnect with the radio on and no try since: the owner's
        # Disconnect, which a self-heal must not undo.
        self.held = False

    def summary(self, now=None):
        """Counts and ages of the supplicant's events for network.json and the journal."""
        now = time.monotonic() if now is None else now
        return {'since_s': round(now - self.started, 1), 'counts': dict(self.counts),
                'ago_s': {kind: round(now - at, 1) for kind, at in self.last.items()},
                'last_disconnect': self.last_disconnect,
                'interrupted_handshakes': self.interrupted_handshakes}

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
                self.held = False
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

    def wait(self, timeout, *, wake_fd=None):
        # Events wake readiness immediately. DHCP remains owned by wpa-event;
        # the bounded acquisition cadence also observes its lease publication.
        readers = ([] if self.sock is None else [self.sock]) + ([] if wake_fd is None else [wake_fd])
        if not readers:
            time.sleep(timeout)
            return
        try:
            select.select(readers, [], [], timeout)
        except OSError:
            self.close()

    def event(self, text):
        body = text.split('>', 1)[1].strip() if text.startswith('<') and '>' in text else text.strip()
        if body.startswith('Trying to associate with '):
            self.held = False
            return
        if body.startswith('Authentication with ') and body.endswith('timed out.'):
            kind = 'AUTH-TIMEOUT'
        elif body.startswith('CTRL-EVENT-'):
            kind = body.split(' ', 1)[0][len('CTRL-EVENT-'):]
        else:
            return
        if kind in EVENT_KINDS:
            self.counts[kind] = self.counts.get(kind, 0) + 1
            self.last[kind] = time.monotonic()
        if kind == 'AUTH-TIMEOUT':
            self.timed_out = True
            return
        if kind == 'DISCONNECTED':
            code = re.search(r'\breason=(\d{1,5})\b', body)
            local = 'locally_generated=1' in body
            self.last_disconnect = {'reason': int(code.group(1)) if code else None, 'local': local,
                                    'timeout': self.timed_out}
            self.interrupted = local and not self.timed_out
            self.held = self.interrupted and not wlan_switched_off(self.ctx)
            self.timed_out = False
            return
        if kind in ('CONNECTED', 'SSID-TEMP-DISABLED', 'ASSOC-REJECT', 'AUTH-REJECT'):
            self.held = False
        error = None
        network = re.search(r'\bid=(\d+)\b', text)
        network = int(network.group(1)) if network else None
        if kind == 'SSID-TEMP-DISABLED':
            # A sleep blocks the radio mid-handshake: cfg80211 disconnects
            # locally (950a93ac 246 s); the rfkill read covers a missed event.
            interrupted = self.interrupted or wlan_switched_off(self.ctx)
            self.interrupted = False
            if 'reason=WRONG_KEY' not in text:
                error = 'authentication_failed'
            elif interrupted:
                self.interrupted_handshakes += 1
                return
            else:
                error = 'handshake_failed'
        elif kind == 'AUTH-REJECT':
            error = 'authentication_rejected'
        elif kind == 'NETWORK-NOT-FOUND':
            error = 'ap_unavailable'
        elif kind == 'CONNECTED':
            self.connected_network = network
        else:
            return
        old = self.ctx.json('/run/y2/wifi-failure.json', {})
        boot = self.ctx.read('/proc/sys/kernel/random/boot_id')
        # Failed handshakes per network since the last connect (kept here, so
        # a service restart does not forget the first one).
        wrong_key = old.get('wrong_key')
        if not (isinstance(wrong_key, dict) and wrong_key.get('boot_id') == boot and
                isinstance(wrong_key.get('count'), int)) or kind == 'CONNECTED':
            wrong_key = None
        if error == 'handshake_failed':
            count = wrong_key['count'] + 1 if wrong_key and wrong_key.get('network_id') == network else 1
            wrong_key = {'network_id': network, 'count': count, 'boot_id': boot}
            if count >= WRONG_KEY_REPEAT:
                error = 'wrong_credentials'
        failures = old.get('failures', 0)
        record = {'error': error, 'epoch': self.ctx.read('/run/y2/wifi-epoch'),
                  'failures': (failures if isinstance(failures, int) else 0) + bool(error)}
        if wrong_key:
            record['wrong_key'] = wrong_key
        # wpa_supplicant's own code (reason=CONN_FAILED, status_code=15): it
        # separates a password from a timeout or SAE (F31). Never a name.
        detail = re.search(r'\b(?:reason|status_code)=([A-Za-z0-9_]{1,32})\b', text) if error else None
        if detail:
            record['reason'] = detail.group(1)
        # A connect ends a failure run: keep what the last failure showed
        # (F13's scan evidence was gone by the time a report was saved).
        last = old if old.get('error') else old.get('last_failure')
        if isinstance(last, dict) and last.get('error'):
            record['last_failure'] = {key: last[key] for key in LAST_FAILURE_KEYS if key in last}
        # Y2Reborn #20: a wrong password stays reported until a connect. The
        # "network not found" that follows (the network is disabled for a while
        # after each failure) never replaces it; it names the network it was.
        if error in CREDENTIAL_ERRORS:
            record['credential'] = {'error': error, 'reason': record.get('reason'), 'boot_id': boot,
                                    'network_id': network, 'monotonic_s': time.monotonic()}
        elif error is not None and isinstance(old.get('credential'), dict) and \
                old['credential'].get('boot_id') == boot:
            record['credential'] = old['credential']
        if error == 'ap_unavailable':
            # wpa_supplicant reports this after every scan: look at most every 30 s.
            last = old.get('scan_monotonic_s')
            if (old.get('boot_id') == self.ctx.read('/proc/sys/kernel/random/boot_id') and
                    isinstance(last, (int, float)) and 0 <= time.monotonic() - last < self.SCAN_EVIDENCE_S and
                    isinstance(old.get('scan'), dict)):
                record.update(scan=old['scan'], scan_monotonic_s=last)
            else:
                record.update(scan=scan_evidence(self.ctx), scan_monotonic_s=time.monotonic())
        publish(self.ctx, 'wifi-failure', record)


class WifiRecovery:
    """F52: after a start or a wake the supplicant sat DISCONNECTED for up to
    17 min with no failure event while the saved network was in range; a Join
    (SELECT_NETWORK) connected in 3-4 s. Once that lasts IDLE_S, do what the
    Join does: select the saved network in range and enable the others again
    (Reborn's join ends so too). Not in range: one scan, so the next look sees
    fresh results. Backed off per episode; every try is a journal row."""
    IDLE_S = 30
    BACKOFF_S = (30, 60, 120, 300)

    def __init__(self, ctx):
        self.ctx = ctx
        self.since = None
        self.tries = 0
        self.next_try = 0
        self.first_try = None

    def wpa(self, *argv):
        return self.ctx.command(['wpa_cli', '-i', 'wlan0', *argv], timeout=2)

    def observe(self, value, now, monitor=None):
        state = value.get('association')
        if state == 'COMPLETED' or not value.get('radio_present') or wlan_switched_off(self.ctx):
            if self.tries and state == 'COMPLETED':
                journal(self.ctx, 'wifi_recovered', tries=self.tries, seconds=round(now - self.first_try, 1))
            self.since, self.tries, self.next_try, self.first_try = None, 0, 0, None
            return None
        # The supplicant is trying (scan, association), a password verdict
        # stands or the owner disconnected: never interfere.
        if (state not in ('DISCONNECTED', 'INACTIVE') or value.get('reason') in CREDENTIAL_ERRORS or
                (monitor is not None and monitor.held)):
            self.since = None
            return None
        self.since = now if self.since is None else self.since
        if now - self.since < self.IDLE_S or now < self.next_try:
            return None
        listed, scan = self.wpa('list_networks'), self.wpa('scan_results')
        self.next_try = now + self.BACKOFF_S[min(self.tries, len(self.BACKOFF_S) - 1)]
        if not (listed.get('ok') and scan.get('ok')):
            return None
        saved = [row for row in (line.split('\t') for line in (listed.get('output') or '').splitlines()[1:])
                 if len(row) >= 4 and row[0].isdigit() and row[1] and '[DISABLED]' not in row[3]]
        if not saved:
            return None
        signal = {}
        for row in (line.split('\t') for line in (scan.get('output') or '').splitlines()[1:]):
            if len(row) >= 5 and row[4] and re.fullmatch(r'-?\d+', row[2]):
                signal[row[4]] = max(signal.get(row[4], -999), int(row[2]))
        seen = [row for row in saved if row[1] in signal]
        self.tries += 1
        self.first_try = now if self.first_try is None else self.first_try
        fields = {'state': state, 'idle_s': round(now - self.since, 1), 'tries': self.tries,
                  'saved': len(saved), 'in_range': len(seen), 'scan_entries': len(signal),
                  'temp_disabled': sum('TEMP-DISABLED' in row[3] for row in saved)}
        if seen:
            last = monitor.connected_network if monitor is not None else None
            chosen = next((row for row in seen if int(row[0]) == last), None) or \
                max(seen, key=lambda row: (signal[row[1]], -int(row[0])))
            selected = self.wpa('select_network', chosen[0])
            enabled = self.wpa('enable_network', 'all')
            action = 'wifi_reselect'
            fields.update(network_id=int(chosen[0]), last_connected=chosen[0] == str(last),
                          ok=[(answer.get('output') or '').strip() == 'OK' for answer in (selected, enabled)])
        else:
            answer = self.wpa('scan')
            action = 'wifi_rescan'
            fields['ok'] = (answer.get('output') or '').strip() == 'OK'
        if monitor is not None:
            fields['events'] = monitor.summary(now)
        journal(self.ctx, action, **fields)
        return action
