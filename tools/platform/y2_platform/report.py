"""Opt-in error reports (docs/planning/ERROR-REPORTS-PLAN.md).

A report holds everything the player records about an error: kernel, platform,
update/recovery and Reborn logs and state, hardware identity. Every byte passes
`Scrubber` before it is written: the player's own secrets (Wi-Fi names and
passphrases, Bluetooth names, account data, serial numbers, the songs in the
queue) are replaced wherever they appear, then patterns (MAC and IP addresses,
e-mail, URL paths, music paths), and JSON values under music keys. Library
contents, account files, keys and anything below /media are never read into a
report at all. Queued reports go to the receiver (tools/reports/worker) over
HTTPS once the owner opted in, and only from a build that carries the upload key;
`save_to` writes a report onto Internal Storage or the SD card instead, for the
owner to send by hand.
"""
# SPDX-License-Identifier: GPL-2.0-only
import contextlib
import datetime
import errno
import fcntl
import gzip
import hashlib
import hmac
import io
import ipaddress
import json
import os
import re
import tarfile
import threading
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

from .common import atomic_json
from .observe import mountinfo
from .workload import workload

SCHEMA = 1
STATE = '/data/system/platform/reports.json'
QUEUE = '/data/system/platform/reports'
QUEUE_MAX = 3
DAILY_MAX = 5
BUDGET = 2 * 1024 * 1024          # compressed report (the receiver takes at most 2 MiB; real ones ≈0.5 MB)
SECTION_MAX = 4 * 1024 * 1024     # raw bytes per section (logs keep their newest end)
# The receiver (tools/reports/worker). The build-wide upload key comes from the
# release build (tools/production/build.py); a build without it never uploads,
# and Reborn then offers only Save Report.
ENDPOINT = 'https://reports.schulzcode.dev/v1/report'
UPLOAD_KEY = '/usr/share/y2linux/report-upload.json'
RETRY_S = (300, 900, 3600, 3 * 3600, 6 * 3600)
GIVE_UP_S = 3 * 86400
MINIMUM_PERCENT = 15              # never below this on battery
REFUSED = (400, 403, 404, 405, 411, 413)   # the receiver will never take this report
# Storage a report can be saved to: its mount and the folder MTP shows as the
# storage's top level (Internal Storage is the music folder, /run/y2/umtprd.conf).
DESTINATIONS = {'internal': ('/media/internal', 'music'), 'sd': ('/media/sd', '')}
FOLDER = 'Error Reports'
# Live progress of a report the owner started (Reborn's report screen reads it,
# like the update's /run/y2/update-progress.json); removed when the run ends.
PROGRESS = '/run/y2/report-progress.json'
TOPICS = ('sound', 'display', 'battery', 'bluetooth', 'wifi', 'crash', 'other')
TRIGGERS = ('manual', 'kernel_crash', 'kernel_warning', 'unclean_restart', 'reborn_crash',
            'reborn_failure', 'update_failure', 'sleep_failure', 'radio_failure',
            'display_failure', 'storage_error')

# JSON keys whose values are music metadata or account data, whatever the source.
PRIVATE_KEYS = frozenset((
    'title', 'artist', 'album', 'album_artist', 'albumartist', 'composer', 'genre', 'filename',
    'comment', 'lyrics', 'ssid', 'bssid', 'psk', 'passphrase', 'password', 'session_key',
    'username', 'user', 'email', 'station', 'stream_url', 'playlist', 'playlist_name', 'query',
    # Filesystem identity of the user's cards (a FAT volume serial, a label they chose).
    'uuid', 'fs_uuid', 'partuuid', 'label', 'volume_label', 'volume_id',
))
# Never a secret, even when a key comment or account field holds one of them.
COMMON_WORDS = frozenset(('Y2Linux', 'y2linux', 'Reborn', 'reborn', 'root', 'localhost', 'true', 'false', 'null'))
# F41: a song called "Ready" or "1.0" also replaced Reborn's shutdown answer and
# the release inside build IDs. Library names without a letter and the state
# words reports are read by stay; network, device and account names do not.
MUSIC_KEPT = frozenset(('Ready', 'Failed', 'Normal', 'Idle', 'Online', 'Offline', 'Charging', 'Discharging',
                        'Full', 'Connected', 'Disconnected', 'Paused', 'Playing', 'Stopped', 'Unknown', 'Error',
                        'None', 'Starting', 'Scanning', 'Bluetooth', 'Wired'))
# The stock LK and preloader regions (production-partitions.json): which
# bootloader a player carries (3.2.0 vs 3.2.1, #45/#46).
BOOTLOADERS = (('lk', '/dev/mmcblk0', 0x1d20000, 0x60000), ('preloader', '/dev/mmcblk0boot0', 0, 0x40000))


# ---------------------------------------------------------------- secrets

VOLUME_ID = re.compile(r'[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}|[0-9A-Fa-f]{16}|'
                       r'[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}')

def _strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from _strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from _strings(item)


def _wpa(text):
    """ssid/psk/identity/password values of wpa_supplicant networks (quoted or hex)."""
    found = {'ssid': set(), 'wifi_secret': set()}
    for key, value in re.findall(r'(?m)^\s*(ssid|psk|password|identity|anonymous_identity|sae_password)\s*=\s*(.+?)\s*$', text or ''):
        if value.startswith('"') and value.endswith('"') and len(value) >= 2:
            value = value[1:-1]
        elif key == 'ssid' and re.fullmatch(r'(?:[0-9a-fA-F]{2})+', value):
            try:
                value = bytes.fromhex(value).decode('utf-8')
            except UnicodeDecodeError:
                continue
        found['ssid' if key == 'ssid' else 'wifi_secret'].add(value)
    return found


def _large_json(ctx, path, limit=16 * 1024 * 1024):
    """ctx.json() reads at most 128 KiB; Reborn's session (the whole queue) is larger."""
    try:
        return json.loads(ctx.read(path, limit=limit) or 'null')
    except ValueError:
        return None


def _rel(ctx, path):
    """A ctx.glob() result as the player path ctx.read() expects."""
    return '/' + str(Path(path).relative_to(ctx.root)).lstrip('/')


def secrets(ctx):
    """This player's private strings by class, read from where they are stored."""
    found = {'ssid': set(), 'wifi_secret': set(), 'bt_name': set(), 'account': set(),
             'ssh': set(), 'serial': set(), 'music': set()}
    for path in ('/data/network/wpa_supplicant.conf', '/etc/wpa_supplicant.conf'):
        for name, values in _wpa(ctx.read(path, limit=262144)).items():
            found[name] |= values
    for path in ('/run/y2/network.json', '/run/y2/wifi-events.json', '/run/y2/wifi-failure.json'):
        value = ctx.json(path)
        for key in ('ssid',):
            for item in _key_values(value, key):
                found['ssid'].add(item)
    for info in ctx.glob('/data/bluetooth/bluez/*/*/info') + ctx.glob('/data/bluetooth/bluez/*/cache/*'):
        for name in re.findall(r'(?m)^(?:Name|Alias)=(.+)$', ctx.read(_rel(ctx, info), limit=65536) or ''):
            found['bt_name'].add(name.strip())
    for value in _strings(ctx.json('/data/reborn/state/lastfm.json', {})):
        found['account'].add(value)
    for path in ('/root/.ssh/authorized_keys',) + tuple(_rel(ctx, p) for p in ctx.glob('/data/ssh/authorized_keys.d/*')):
        for line in (ctx.read(path, limit=65536) or '').splitlines():
            parts = line.split()
            # options? type key comment: everything but the key type is private.
            found['ssh'].update(part for part in parts if not part.startswith(('ssh-', 'ecdsa-', 'sk-')))
    for path in ctx.glob('/sys/class/mmc_host/*/*/cid') + ctx.glob('/sys/class/mmc_host/*/*/serial'):
        found['serial'].add(ctx.read(_rel(ctx, path)) or '')
    for path in ('/data/system/dbus/machine-id', '/etc/machine-id'):
        found['serial'].add(ctx.read(path) or '')
    # Volume serials of the user's cards (FAT XXXX-XXXX, exFAT/NTFS 16 hex, ext4
    # UUID): Reborn names an SD source by it ({"kind":"sd_card","id":...}).
    for path in ('/run/y2/media.json', '/run/y2/media-mount.json', '/run/y2/storage.json',
                 '/run/y2/internal-media.json', '/run/y2/mtp-roots.json', '/run/y2/media-lifecycle.json'):
        for value in _strings(ctx.json(path)):
            if VOLUME_ID.fullmatch(value):
                found['serial'].add(value)
    # A record that names the boot (a random ID per boot, kept in report.json)
    # looks like an ext4 UUID; it is not a volume serial.
    found['serial'].discard(ctx.read('/proc/sys/kernel/random/boot_id') or '')
    serial = re.search(r'(?mi)^Serial\s*:\s*(\S+)', ctx.read('/proc/cpuinfo') or '')
    if serial:
        found['serial'].add(serial.group(1))
    # A bootloader can pass the device serial on the command line.
    found['serial'].update(re.findall(r'(?:^|\s)\S*serial(?:no)?=(\S+)', ctx.read('/proc/cmdline') or ''))
    session = _large_json(ctx, '/data/reborn/state/session.json')
    model = session.get('model', {}) if isinstance(session, dict) else {}
    for track in (model.get('queue') or [])[:5000] if isinstance(model, dict) else []:
        if isinstance(track, dict):
            for key in ('title', 'artist', 'album', 'album_artist', 'filename', 'genre'):
                if isinstance(track.get(key), str):
                    found['music'].add(track[key])
    # Too short to replace safely (would hit ordinary words and numbers).
    def kept(name, value):
        return name == 'music' and (value.strip() in MUSIC_KEPT or not re.search(r'[^\W\d_]', value))
    return {name: {v for v in values if isinstance(v, str) and len(v.strip()) >= 3 and v not in COMMON_WORDS
                   and v.strip() not in ('0000000000000000',) and not kept(name, v)}
            for name, values in found.items()}


def _key_values(value, key):
    if isinstance(value, dict):
        for k, v in value.items():
            if k == key and isinstance(v, str):
                yield v
            else:
                yield from _key_values(v, key)
    elif isinstance(value, list):
        for v in value:
            yield from _key_values(v, key)


# ---------------------------------------------------------------- scrubbing

MAC = re.compile(r'(?<![0-9A-Fa-f])([0-9A-Fa-f]{2})([:_-])([0-9A-Fa-f]{2})\2([0-9A-Fa-f]{2})\2'
                 r'([0-9A-Fa-f]{2})\2([0-9A-Fa-f]{2})\2([0-9A-Fa-f]{2})(?![0-9A-Fa-f])')
IPV4 = re.compile(r'(?<![\d.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?![\d.]|\.\d)')
IPV6 = re.compile(r'(?<![0-9A-Za-z:])(?=[0-9A-Fa-f:]{0,39}::|(?:[0-9A-Fa-f]{1,4}:){5})'
                  r'(?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}(?![0-9A-Za-z:])')
# Every pattern starts only at a boundary and is bounded: a long run of hex or
# letters (a hash, a key, a corrupt log) must not make a scan quadratic.
EMAIL = re.compile(r'(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63}){0,8}\.[A-Za-z]{2,24}(?![A-Za-z0-9-])')
URL = re.compile(r'\b(https?|rtsp|icy)://([^/\s"\'<>?#]+)([^\s"\'<>]*)')
# A music path ends at its file extension; without one (a folder, a name with
# spaces) the rest of the line goes too: privacy before context.
MEDIA = re.compile(r'(?:/run)?/media/(?:[^\n\t"\']*?\.(?:flac|mp3|m4a|mp4|aac|ogg|oga|opus|wav|aiff?|ape|wv|'
                   r'dsf|dff|alac|jpe?g|png|webp|bmp|gif|txt|cue|m3u8?|pls|lrc)(?=[\s"\',:;)\]]|$)|[^\n\t"\']*)',
                   re.IGNORECASE)
SERIAL_FIELD = re.compile(r'(?i)\b(serial(?:_number)?|cid|machine[-_]id|(?:part)?uuid)(\s*[:=]\s*"?)([0-9A-Za-z-]{8,})')


def _trie_pattern(values):
    """One regex for many literal strings, shaped as a prefix tree: the engine
    then follows one branch per character instead of trying every string at
    every position (2000 queue titles: 4 s → milliseconds per 200 KB).
    Optional tails are greedy, so the longest string wins."""
    trie = {}
    for value in values:
        node = trie
        for char in value:
            node = node.setdefault(char, {})
        node[''] = {}

    def pattern(node):
        end = '' in node
        branches = [re.escape(char) + pattern(child) for char, child in sorted(node.items()) if char]
        if not branches:
            return ''
        body = branches[0] if len(branches) == 1 else '(?:' + '|'.join(branches) + ')'
        return '(?:%s)?' % body if end else body

    return re.compile(pattern(trie)) if values else None


class Scrubber:
    """Replace personal data; tokens are stable within one report.

    A token names its value by a keyed hash (`<ssid#3fa2c1>`): the same value
    gets the same token in every part of a report, also when parts are
    scrubbed in parallel, and the per-report random key (never written) keeps
    a short hash of, say, an IP address from being guessed back."""

    def __init__(self, known=None, key=None):
        self.key = key or os.urandom(16)
        self.counts = {}
        pairs = []
        self.original = {}
        for name, values in (known or {}).items():
            for value in values:
                pairs.append((value, name))
                # Also as written inside JSON by Python (\u escapes, \"): JSON
                # lines are scrubbed as text (_scrub_blob).
                escaped = json.dumps(value)[1:-1]
                if escaped != value:
                    pairs.append((escaped, name))
                    self.original[escaped] = value      # one token for both forms
        # Longest first so a passphrase containing the SSID is replaced whole.
        pairs.sort(key=lambda pair: (-len(pair[0]), pair[0]))
        self.known = dict(pairs)
        self.known_pattern = _trie_pattern([value for value, _ in pairs])

    def _known(self, match):
        value, text = match.group(0), match.string
        # Word-edged values only replace whole words ("Home", not "Homepage").
        if value[0].isalnum() and match.start() > 0 and text[match.start() - 1].isalnum():
            return value
        if value[-1].isalnum() and match.end() < len(text) and text[match.end()].isalnum():
            return value
        return self.token(self.known[value], self.original.get(value, value))

    def token(self, name, value, keep=''):
        self.counts[name] = self.counts.get(name, 0) + 1
        digest = hmac.new(self.key, ('%s\0%s' % (name, value)).encode(), 'sha256').hexdigest()
        return '<%s%s#%s>' % (name, keep, digest[:6])

    def _mac(self, match):
        octets = [match.group(i) for i in (1, 3, 4, 5, 6, 7)]
        if all(o == '00' for o in octets) or all(o.lower() == 'ff' for o in octets):
            return match.group(0)
        sep = match.group(2)
        maker = sep.join(octets[:3]).upper()
        return '%s%s%s' % (maker, sep, self.token('mac', sep.join(octets).upper()))

    def _ipv4(self, match):
        text = match.group(0)
        try:
            address = ipaddress.IPv4Address(text)
        except ValueError:
            return text
        if address.is_loopback or address.is_unspecified or text == '255.255.255.255':
            return text
        if address in ipaddress.IPv4Network('10.42.0.0/24'):
            return text  # the fixed USB network of every Y2Linux player
        kind = 'private' if address.is_private else 'link' if address.is_link_local else 'public'
        return self.token('ip', text, ':' + kind)

    def _ipv6(self, match):
        text = match.group(0)
        try:
            address = ipaddress.IPv6Address(text)
        except ValueError:
            return text
        if address.is_loopback or address.is_unspecified:
            return text
        if text.count(':') < 3 and not any(c.isdigit() for c in text):
            return text  # "a::b", "dead::beef": a path or a name, not an address
        kind = 'link' if address.is_link_local else 'private' if address.is_private else 'public'
        return self.token('ip6', text, ':' + kind)

    def _media(self, match):
        text = match.group(0).rstrip()
        extension = re.search(r'\.([A-Za-z0-9]{1,5})$', text)
        self.counts['media_path'] = self.counts.get('media_path', 0) + 1
        root = '/media/sd' if text.startswith('/media/sd') else '/media/internal' if text.startswith('/media/internal') else '/media'
        return root + '/<path>' + ('.' + extension.group(1) if extension else '')

    def text(self, value):
        if not value:
            return value
        # Paths and URLs first: their shape is lost once a name inside is replaced.
        # A pattern runs only when the text holds a character it needs.
        if '/media/' in value:
            value = MEDIA.sub(self._media, value)
        if '://' in value:
            value = URL.sub(lambda m: '%s://%s/<path>' % (m.group(1), m.group(2)) if m.group(3) not in ('', '/') else m.group(0), value)
        if self.known_pattern:
            value = self.known_pattern.sub(self._known, value)
        if '@' in value:
            value = EMAIL.sub(lambda m: self.token('email', m.group(0)), value)
        lower = value.lower()
        if 'serial' in lower or 'cid' in lower or 'machine' in lower or 'uuid' in lower:
            value = SERIAL_FIELD.sub(lambda m: m.group(1) + m.group(2) + self.token('serial', m.group(3)), value)
        value = MAC.sub(self._mac, value)
        if ':' in value:
            value = IPV6.sub(self._ipv6, value)
        if '.' in value:
            value = IPV4.sub(self._ipv4, value)
        return value

    def value(self, value, key=None):
        if isinstance(value, dict):
            peer = isinstance(value.get('address'), str) and MAC.search(value['address'])
            return {self.text(str(k)): ('<bt_name>' if peer and k in ('name', 'alias') and isinstance(v, str) and v
                                        else self.value(v, str(k))) for k, v in value.items()}
        if isinstance(value, list):
            return [self.value(v, key) for v in value]
        if isinstance(value, str):
            if key is not None and key.lower() in PRIVATE_KEYS and value:
                self.counts['private_key'] = self.counts.get('private_key', 0) + 1
                return '<%s>' % key.lower()
            return self.text(value)
        return value


# ---------------------------------------------------------------- sources

# Bound on a file without a size (a seq_file) read through for its tail.
READ_THROUGH_MAX = 16 * 1024 * 1024


def _tail(data, limit=SECTION_MAX):
    if len(data) <= limit:
        return data, False
    cut = data[-limit:]
    newline = cut.find(b'\n')
    return (cut[newline + 1:] if 0 <= newline < 4096 else cut), True


def _read_bytes(ctx, path, limit=SECTION_MAX):
    try:
        with open(ctx.path(path), 'rb') as stream:
            try:
                stream.seek(0, os.SEEK_END)
            except OSError:
                # seq_file (all of /proc but /proc/sys, debugfs) refuses
                # SEEK_END: until 8.12 every such file was silently left out.
                # Read it through and keep the newest end.
                data, total = b'', 0
                while total < READ_THROUGH_MAX and (chunk := stream.read(65536)):
                    total += len(chunk)
                    data = (data + chunk)[-(limit + 4096):]
                return _tail(data)
            size = stream.tell()
            stream.seek(max(0, size - limit - 4096))
            return _tail(stream.read())
    except OSError:
        return None, False


def _session_projection(session):
    """Every setting and technical playback fact of session.json; no queue entries."""
    if not isinstance(session, dict):
        return None
    model = session.get('model') if isinstance(session.get('model'), dict) else {}
    queue = model.get('queue') if isinstance(model.get('queue'), list) else []
    position = model.get('queue_position')
    current = queue[position] if isinstance(position, int) and 0 <= position < len(queue) else None
    technical = ('codec', 'sample_rate', 'channels', 'bitrate', 'bits_per_sample', 'duration_ms', 'source_id',
                 'online', 'artwork')
    settings = dict(model.get('settings') or {})
    if isinstance(settings.get('time_zone'), str):
        settings['time_zone'] = 'set' if settings['time_zone'] else ''
    return {
        'schema': session.get('schema'),
        'settings': settings,
        'playback': model.get('playback'), 'position_ms': model.get('position_ms'),
        'output': model.get('output'), 'queue_length': len(queue), 'queue_position': position,
        'current_track': {k: current.get(k) for k in technical if k in current} if isinstance(current, dict) else None,
        'other_keys': sorted(k for k in model if k not in ('queue', 'queue_entry_ids', 'settings')),
    }


def _library_statistics(ctx):
    """Counts only (sqlite read-only); never a name, title or path."""
    path = ctx.path('/data/reborn/library.db')
    if not path.exists():
        return None
    try:
        import sqlite3
        connection = sqlite3.connect('file:%s?mode=ro&immutable=1' % path, uri=True, timeout=1)
    except Exception as error:  # sqlite missing or file unreadable
        return {'error': type(error).__name__}
    result = {'bytes': path.stat().st_size}
    try:
        tables = [row[0] for row in connection.execute("select name from sqlite_master where type='table'")]
        result['tables'] = {}
        for table in tables:
            if re.fullmatch(r'[A-Za-z0-9_]+', table):
                result['tables'][table] = connection.execute('select count(*) from "%s"' % table).fetchone()[0]
        columns = {row[1] for row in connection.execute('pragma table_info(tracks)')} if 'tracks' in tables else set()
        for column in ('codec', 'sample_rate', 'channels', 'source_id', 'bits_per_sample', 'online'):
            if column in columns:
                rows = connection.execute('select "%s", count(*) from tracks group by 1 order by 2 desc limit 32' % column)
                result['by_' + column] = [[str(value) if column == 'source_id' else value, count] for value, count in rows]
        result['user_version'] = connection.execute('pragma user_version').fetchone()[0]
    except Exception as error:
        result['error'] = type(error).__name__
    finally:
        connection.close()
    # Source IDs are SD card filesystem IDs: keep their number, not the value.
    if 'by_source_id' in result:
        result['by_source_id'] = [['source#%d' % (i + 1), count] for i, (_, count) in enumerate(result['by_source_id'])]
    return result


def _hardware(ctx):
    def first(pattern):
        hits = ctx.glob(pattern)
        return hits[0] if hits else None
    data = {'compatible': (ctx.read('/proc/device-tree/compatible') or '').replace('\x00', ' ').strip(),
            'cpuinfo': ctx.read('/proc/cpuinfo'), 'meminfo_total': None,
            'y2_boot': {name: ctx.read('/sys/firmware/y2_boot/' + name) for name in ('metadata', 'lcm', 'normal_boot')},
            'panel': {name: ctx.read('/sys/module/panel/parameters/' + name)
                      for name in ('panel_kind', 'panel_hint', 'power_down', 'stock_suspend')},
            'storage': {}, 'connectivity': ctx.read('/sys/devices/platform/18070000.connectivity/status'),
            'input_devices': ctx.read('/proc/bus/input/devices'), 'partitions': ctx.read('/proc/partitions'),
            'bootloaders': {}}
    for line in (ctx.read('/proc/meminfo') or '').splitlines():
        if line.startswith('MemTotal:'):
            data['meminfo_total'] = line.split(':', 1)[1].strip()
    for host in ('mmc0', 'mmc1'):
        card = first('/sys/class/mmc_host/%s/%s:*' % (host, host))
        if card:
            card = Path(_rel(ctx, card))
            fields = {}
            for name in ('type', 'name', 'manfid', 'oemid', 'prv', 'rev', 'fwrev', 'hwrev', 'date', 'csd',
                         'ocr', 'life_time', 'pre_eol_info', 'enhanced_area_size', 'erase_size', 'preferred_erase_size'):
                fields[name] = ctx.read(card / name)
            ext_csd = first('/sys/kernel/debug/%s/%s:*/ext_csd' % (host, host))
            fields['ext_csd'] = ctx.read(_rel(ctx, ext_csd)) if ext_csd else None
            fields['ios'] = ctx.read('/sys/kernel/debug/%s/ios' % host)
            data['storage'][host] = fields
    for name, device, offset, length in BOOTLOADERS:
        try:
            with open(ctx.path(device), 'rb') as stream:
                stream.seek(offset)
                blob = stream.read(length)
            data['bootloaders'][name] = {'sha256': hashlib.sha256(blob).hexdigest(), 'bytes': len(blob),
                                         'lk_v320_prefix_sha256': hashlib.sha256(blob[:242440]).hexdigest() if name == 'lk' else None}
        except OSError as error:
            data['bootloaders'][name] = {'error': error.strerror}
    return data


def _commands(ctx):
    """Live state from tools; each bounded."""
    runs = {
        'dmesg': (['dmesg'], 10.0, 2 * 1024 * 1024),
        'platform_health': (['y2-platform', 'health'], 10.0, 1024 * 1024),
        'platform_status': (['y2-platform', 'status'], 10.0, 1024 * 1024),
        'update_status': (['y2-platform', 'update', 'status'], 10.0, 256 * 1024),
        'reborn_status': (['rebornctl', 'status', '--json'], 5.0, 1024 * 1024),
        'reborn_health': (['rebornctl', 'health', '--json'], 5.0, 1024 * 1024),
        'reborn_metrics': (['rebornctl', 'metrics', '--json'], 5.0, 1024 * 1024),
        'reborn_audio': (['rebornctl', 'audio', '--json'], 5.0, 1024 * 1024),
        'reborn_snapshot': (['rebornctl', 'snapshot', '--json'], 5.0, 1024 * 1024),
        'reborn_events': (['rebornctl', 'events', '--last', '500', '--json'], 5.0, 2 * 1024 * 1024),
        'processes': (['ps', '-o', 'pid,ppid,stat,rss,vsz,time,args'], 5.0, 512 * 1024),
        'mounts_space': (['df', '-k'], 5.0, 64 * 1024),
        'amixer': (['amixer', '-c', 'Y2Audio', 'contents'], 5.0, 256 * 1024),
    }
    return {name: ctx.command(argv, timeout=timeout, limit=limit, failure_output=True)
            for name, (argv, timeout, limit) in runs.items()}


# Files copied whole (logs keep their newest end). Globs are bounded by ctx.glob.
FILES = (
    '/sys/fs/pstore/*',
    '/data/logs/system.log', '/data/logs/system.log.0', '/data/logs/recovery-last.log',
    '/data/logs/recovery-display.txt', '/data/logs/recovery-previous-boot.txt',
    '/proc/version', '/proc/cmdline', '/proc/meminfo', '/proc/vmstat', '/proc/loadavg', '/proc/interrupts',
    '/proc/softirqs', '/proc/stat', '/proc/pressure/*', '/proc/mounts', '/proc/sys/kernel/tainted',
    '/sys/power/suspend_stats/*', '/sys/kernel/debug/wakeup_sources', '/sys/kernel/debug/suspend_stats',
    '/sys/kernel/debug/regulator/regulator_summary', '/sys/kernel/debug/clk/clk_summary',
    # The CS43131 at 0x30; its I2C bus number follows probe order (0 or 1).
    '/sys/kernel/debug/regmap/*-0030/registers', '/sys/bus/i2c/devices/*-0030/y2_hp/xtal',
    '/sys/devices/system/cpu/cpu*/cpuidle/state*/usage', '/sys/devices/system/cpu/cpu*/cpuidle/state*/time',
    '/sys/devices/system/cpu/cpu*/cpuidle/state*/name',
    '/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state', '/sys/devices/system/cpu/online',
    '/sys/class/thermal/thermal_zone*/temp', '/sys/class/thermal/thermal_zone*/type',
    '/sys/class/power_supply/*/uevent', '/sys/class/power_supply/BAT0/charging_state', '/sys/fs/ext4/*/errors_count', '/sys/fs/ext4/*/first_error_time',
    '/sys/devices/platform/11200000.usb/status', '/sys/devices/platform/11200000.usb/rxdiag',
    '/data/system/platform/boot.json', '/data/system/platform/previous-boot-evidence.json',
    '/data/system/platform/power-journal.jsonl', '/data/system/platform/sleep-diagnostics.jsonl',
    '/data/system/platform/suspend-last.json', '/data/system/platform/product-sleep-guard.json',
    '/data/system/platform/rtc-policy.json', '/data/system/platform/time-last.json',
    '/data/system/platform/battery-ui.json', '/data/system/platform/panel.json',
    '/data/system/platform/sd-admissions.json', '/data/system/platform/sd-repair-last.json',
    '/data/system/platform/wifi-journal*.jsonl',
    '/data/system/platform/retained/*.tar',
    '/data/updates/*.json', '/data/updates/current/manifest.json', '/data/updates/health/*.txt',
    '/run/y2/*.json', '/run/y2/*.jsonl', '/run/y2/*.log',
    '/data/bluetooth/codec-history.json', '/data/bluetooth/enabled', '/data/network/enabled',
    '/data/network/power-save.json',
    '/data/reborn/logs/*.jsonl', '/data/reborn/logs/panic-last.json', '/data/reborn/diagnostics/*.tar.gz',
)
# Never read into a report, even if a glob above ever matched them.
NEVER = re.compile(r'^/(media|data/music|data/ssh|root/\.ssh)/|wpa_supplicant|lastfm\.json|scrobbles\.jsonl|'
                   r'library\.db|/bluez/|machine-id|entropy|/keys?/|\.pem$|\.key$|session\.json')


# JSON lines are scrubbed as one text (every string in them passes the same
# patterns and known values): value by value, the logs of one report took ≈45 s
# on the Y2. A private key's string value is replaced in the text too; a line
# with a private key holding a list or object, or a Bluetooth peer's address,
# still goes value by value.
_KEYS = '|'.join(sorted(map(re.escape, PRIVATE_KEYS)))
STRUCTURED = re.compile(r'"(?:%s)"\s*:\s*[\[{]|"address"\s*:' % _KEYS, re.IGNORECASE)
PRIVATE_STRING = re.compile(r'"(%s)"(\s*:\s*)"((?:[^"\\]|\\.)*)"' % _KEYS, re.IGNORECASE)


def _private_string(scrubber, match):
    if not match.group(3):
        return match.group(0)
    scrubber.counts['private_key'] = scrubber.counts.get('private_key', 0) + 1
    return '"%s"%s"<%s>"' % (match.group(1), match.group(2), match.group(1).lower())


def _scrub_blob(scrubber, path, data):
    """One file's bytes, scrubbed by its kind (JSON, JSON lines, tar, text)."""
    if path.endswith(('.tar', '.tar.gz')):
        return _scrub_tar(scrubber, data, path.endswith('.gz'))
    text = data.decode('utf-8', errors='replace')
    if path.endswith('.jsonl'):
        lines = []
        for line in text.splitlines():
            if not STRUCTURED.search(line):
                lines.append(scrubber.text(PRIVATE_STRING.sub(lambda m: _private_string(scrubber, m), line)))
                continue
            try:
                lines.append(json.dumps(scrubber.value(json.loads(line)), sort_keys=True))
            except ValueError:
                lines.append(scrubber.text(line))
        return ('\n'.join(lines) + '\n').encode()
    if path.endswith('.json'):
        try:
            return json.dumps(scrubber.value(json.loads(text)), indent=1, sort_keys=True).encode()
        except ValueError:
            pass
    return scrubber.text(text).encode()


PART = 256 * 1024                   # line-aligned pieces of a log scrubbed in parallel
WORKERS = 4                         # the Y2's cores
_worker = None


def _parts(path, data):
    """A file as pieces that scrub independently: logs split at line ends
    (no pattern spans a line), JSON documents and archives whole."""
    if path.endswith(('.json', '.tar', '.tar.gz')) or len(data) <= PART:
        return [data]
    parts, start = [], 0
    while start < len(data):
        end = data.find(b'\n', start + PART)
        end = len(data) if end < 0 else end + 1
        parts.append(data[start:end])
        start = end
    return parts


def _worker_start(known, key):
    global _worker
    _worker = Scrubber(known, key)


def _worker_scrub(job):
    index, path, data = job
    _worker.counts = {}
    return index, (_scrub_blob(_worker, path, data), _worker.counts)


def _scrub_files(scrubber, known, items, pool, progress=None):
    """Each (path, data) scrubbed, in parallel when a pool is there;
    progress(fraction) follows the bytes done."""
    progress = progress or (lambda fraction: None)
    jobs, owners = [], []
    for n, (path, data) in enumerate(items):
        for part in _parts(path, data):
            jobs.append((path, part))
            owners.append(n)
    total = sum(len(data) for _, data in jobs) or 1
    done = 0
    results = [None] * len(jobs)
    if pool is None:
        for i, (path, data) in enumerate(jobs):
            results[i] = (_scrub_blob(scrubber, path, data), {})
            done += len(data)
            progress(done / total)
    else:
        # Big pieces first so no core waits on the last large one.
        order = sorted(range(len(jobs)), key=lambda i: -len(jobs[i][1]))
        for i, result in pool.imap_unordered(_worker_scrub, [(i, *jobs[i]) for i in order], chunksize=1):
            results[i] = result
            done += len(jobs[i][1])
            progress(done / total)
    cleaned = [b''] * len(items)
    for owner, (clean, counts) in zip(owners, results):
        cleaned[owner] += clean
        for name, count in counts.items():
            scrubber.counts[name] = scrubber.counts.get(name, 0) + count
    return cleaned


def _pool(known, key):
    """Worker processes forked before any thread starts; None if unavailable."""
    try:
        import multiprocessing
        return multiprocessing.get_context('fork').Pool(WORKERS, _worker_start, (known, key))
    except (ImportError, OSError, ValueError):
        return None


def _scrub_tar(scrubber, data, compressed):
    """Members of a retained tar (boot archives, Reborn diagnostics), each scrubbed."""
    try:
        source = tarfile.open(fileobj=io.BytesIO(data), mode='r:gz' if compressed else 'r:')
    except (tarfile.TarError, OSError, EOFError):
        return b''
    out = io.BytesIO()
    with source, tarfile.open(fileobj=out, mode='w') as target:
        for member in source.getmembers()[:64]:
            if not member.isfile() or member.size > SECTION_MAX:
                continue
            body = source.extractfile(member).read()
            clean = _scrub_blob(scrubber, member.name, body)
            info = tarfile.TarInfo(scrubber.text(member.name))
            info.size, info.mtime, info.mode = len(clean), member.mtime, 0o600
            target.addfile(info, io.BytesIO(clean))
    return out.getvalue()


class Progress:
    """Live progress of one report run for the owner's screen: the step
    (collect, scrub, pack, then upload or save), the whole run's percent and
    an estimate of the seconds left. At most five writes a second, in RAM."""

    SLICES = {'collect': (0, 12), 'scrub': (12, 76), 'pack': (76, 82), 'upload': (82, 100), 'save': (82, 100)}

    def __init__(self, ctx, destination=None):
        self.path = ctx.path(PROGRESS)
        self.destination = destination
        self.start = time.monotonic()
        self.written = 0.0

    def __call__(self, phase, fraction=0.0, force=False, **fields):
        now = time.monotonic()
        if not force and now - self.written < 0.2:
            return
        self.written = now
        low, high = self.SLICES[phase]
        percent = low + (high - low) * min(max(fraction, 0.0), 1.0)
        elapsed = now - self.start
        left = int(round(elapsed * (100 - percent) / percent)) if percent >= 15 else None
        try:
            self.path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
            atomic_json(self.path, dict(schema=1, phase=phase, percent=round(percent, 1), seconds_left=left,
                                        destination=self.destination, **fields))
        except OSError:
            pass                  # the screen goes without; the report does not

    def close(self):
        try:
            self.path.unlink()
        except OSError:
            pass


def _quiet(*args, **kwargs):
    pass


def build(ctx, trigger, detail='', topic=None, install_id=None, now=None, progress=None, triggers=None):
    """The complete report (tar.gz bytes) and its index."""
    triggers = list(triggers or [trigger])
    if trigger not in TRIGGERS or triggers[0] != trigger or not set(triggers) <= set(TRIGGERS):
        raise ValueError('unknown_trigger')
    if topic is not None and topic not in TOPICS:
        raise ValueError('unknown_topic')
    progress = progress or _quiet
    progress('collect', 0, force=True)
    known = secrets(ctx)
    scrubber = Scrubber(known)
    pool = _pool(known, scrubber.key)
    try:
        return _build(ctx, triggers, detail, topic, install_id, now, scrubber, known, pool, progress)
    finally:
        if pool is not None:
            pool.terminate()


def _build(ctx, triggers, detail, topic, install_id, now, scrubber, known, pool, progress):
    now = now or datetime.datetime.now(datetime.timezone.utc)
    versions = ctx.json('/etc/y2linux/versions.json', {})
    local = datetime.datetime.now().astimezone()
    index = {
        'schema': SCHEMA, 'report_id': str(uuid.uuid4()), 'install_id': install_id,
        'trigger': triggers[0], 'triggers': triggers, 'detail': scrubber.text(detail or ''), 'topic': topic,
        'created_utc': now.isoformat(), 'utc_offset_minutes': int(local.utcoffset().total_seconds() // 60),
        'uptime_seconds': (ctx.read('/proc/uptime') or '0').split()[0],
        'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
        'release': ctx.json('/etc/y2linux/release.json', {}) or versions,
        'versions': versions, 'build_id': ctx.read('/etc/y2linux/build-id'),
        'kernel': ctx.read('/proc/sys/kernel/osrelease'),
        'sections': {}, 'scrubbed': {},
    }
    members = []

    def add(name, data, source=None, cut=False):
        members.append((name, data))
        index['sections'][name] = {'bytes': len(data), 'source': source, 'cut': cut}

    add('hardware.json', json.dumps(scrubber.value(_hardware(ctx)), indent=1, sort_keys=True).encode(), 'sysfs')
    add('reborn/session.json', json.dumps(scrubber.value(_session_projection(
        _large_json(ctx, '/data/reborn/state/session.json'))), indent=1, sort_keys=True).encode(), 'session.json (projected)')
    add('reborn/library.json', json.dumps(_library_statistics(ctx), indent=1, sort_keys=True).encode(), 'library.db (counts)')
    # The live commands (≈4 s, mostly waiting) run while the files scrub.
    commands = {}
    runner = threading.Thread(target=lambda: commands.update(_commands(ctx)), daemon=True)
    runner.start()
    seen, files = set(), []
    progress('collect', 0.3)
    for n, pattern in enumerate(FILES):
        progress('collect', 0.3 + 0.7 * n / len(FILES))
        for path in ctx.glob(pattern) if '*' in pattern else [ctx.path(pattern)]:
            relative = '/' + str(path.relative_to(ctx.root)).lstrip('/')
            if relative in seen or NEVER.search(relative) or not path.is_file():
                continue
            seen.add(relative)
            data, cut = _read_bytes(ctx, relative)
            if data is not None:
                files.append((relative, data, cut))
    progress('scrub', 0, force=True)
    cleaned = _scrub_files(scrubber, known, [(relative, data) for relative, data, _ in files], pool,
                           lambda fraction: progress('scrub', fraction))
    progress('pack', 0, force=True)
    runner.join()
    for name, result in commands.items():
        output = (result or {}).get('output') or ''
        data, cut = _tail(output.encode())
        try:
            clean = json.dumps(scrubber.value(json.loads(data)), indent=1, sort_keys=True).encode()
        except ValueError:
            clean = scrubber.text(data.decode('utf-8', errors='replace')).encode()
        add('commands/%s.txt' % name, clean, 'command' + ('' if result.get('ok') else ' (%s)' % result.get('reason')), cut)
    for (relative, _, cut), clean in zip(files, cleaned):
        # Scrubbed archives are re-packed uncompressed (the report itself is gzip).
        name = relative[:-3] if relative.endswith('.tar.gz') else relative
        add('files' + name, clean, relative, cut)
    index['scrubbed'] = dict(sorted(scrubber.counts.items()))
    packed = _pack(index, members)
    progress('pack', 1, force=True)
    return packed, index


def _pack(index, members):
    """tar.gz within BUDGET: the largest optional sections are cut first."""
    optional = sorted((i for i, (name, _) in enumerate(members) if name.startswith('files/')),
                      key=lambda i: -len(members[i][1]))
    while True:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w') as tar:
            body = json.dumps(index, indent=1, sort_keys=True).encode()
            for name, data in [('report.json', body)] + members:
                info = tarfile.TarInfo(name)
                info.size, info.mtime, info.mode = len(data), int(time.time()), 0o600
                tar.addfile(info, io.BytesIO(data))
        packed = gzip.compress(raw.getvalue(), compresslevel=6, mtime=0)
        if len(packed) <= BUDGET or not optional:
            return packed
        victim = optional.pop(0)
        name, data = members[victim]
        members[victim] = (name, data[len(data) // 2:])
        index['sections'][name]['cut'] = True
        index['sections'][name]['bytes'] = len(members[victim][1])
        optional.append(victim)
        optional.sort(key=lambda i: -len(members[i][1]))


# ---------------------------------------------------------------- state and queue

def state(ctx):
    value = ctx.json(STATE, {})
    value = value if isinstance(value, dict) else {}
    value.setdefault('schema', 1)
    value.setdefault('enabled', None)     # None: not asked yet
    value.setdefault('install_id', None)
    value.setdefault('daily', {})
    value.setdefault('boot_triggers', {})
    value.setdefault('seen', {})
    if not isinstance(value.get('uploads'), dict):
        value['uploads'] = {}             # upload retries: queue file name -> attempts, next, error
    return value


def save(ctx, value):
    path = ctx.path(STATE)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_json(path, value, durable=True)


def set_enabled(ctx, enabled):
    value = state(ctx)
    was = value['enabled']
    value['enabled'] = bool(enabled)
    if enabled and not value['install_id']:
        value['install_id'] = str(uuid.uuid4())
    if enabled and was is not True:
        # Only what happens after the user said yes is reported: record what is
        # already there (an old crash record, a finished rollback) as seen.
        save(ctx, value)
        check(ctx)
        value = state(ctx)
    if not enabled:
        for item in ctx.glob(QUEUE + '/*.tar.gz'):
            item.unlink(missing_ok=True)
    save(ctx, value)
    return status(ctx)


def new_install_id(ctx):
    value = state(ctx)
    value['install_id'] = str(uuid.uuid4())
    save(ctx, value)
    return status(ctx)


def status(ctx):
    value = state(ctx)
    queued = [p.name for p in ctx.glob(QUEUE + '/*.tar.gz')]
    return {'schema': 1, 'enabled': value['enabled'], 'install_id': value['install_id'],
            'queued': queued, 'last': value.get('last'), 'upload': upload_key(ctx) is not None,
            'destinations': destinations(ctx)}


def destinations(ctx):
    """The music storage mounted writable now, in DESTINATIONS order."""
    mounted = {m['path']: m for m in mountinfo(ctx.read('/proc/self/mountinfo', limit=1 << 20))}
    from .media import can_write
    return [name for name, (path, top) in DESTINATIONS.items()
            if path in mounted and ('rw' in mounted[path]['options'] or path == '/media/sd' and can_write(ctx))
            and (ctx.path(path) / top).is_dir()]


def _write_into(mount, top, data, stamp):
    """Write data as top/FOLDER/y2linux-report-<stamp>.tar.gz below mount, durably.

    Every folder must be a real directory on the mounted filesystem itself (no
    symlink out of it); the file is complete before it gets its name."""
    root = os.open(mount, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        device = os.fstat(root).st_dev
        for part in [p for p in top.split('/') if p]:
            inner = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
            os.close(root)
            root = inner
        try:
            os.mkdir(FOLDER, 0o755, dir_fd=root)
        except FileExistsError:
            pass
        folder = os.open(FOLDER, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
    finally:
        os.close(root)
    try:
        if os.fstat(folder).st_dev != device:
            raise OSError(errno.EXDEV, 'report_folder_elsewhere')
        space = os.fstatvfs(folder)
        if space.f_bavail * space.f_frsize < len(data) + 1024 * 1024:
            raise OSError(errno.ENOSPC, 'report_no_space')
        for n in range(1, 100):
            name = 'y2linux-report-%s%s.tar.gz' % (stamp, '' if n == 1 else '-%d' % n)
            try:
                os.lstat(name, dir_fd=folder)
            except FileNotFoundError:
                break
        else:
            raise OSError(errno.EEXIST, 'report_names_taken')
        temporary = '.' + name + '.part'
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o644, dir_fd=folder)
        try:
            with os.fdopen(fd, 'wb') as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            os.rename(temporary, name, src_dir_fd=folder, dst_dir_fd=folder)
        except BaseException:
            try:
                os.unlink(temporary, dir_fd=folder)
            except OSError:
                pass
            raise
        try:
            os.fsync(folder)           # an SD card may be pulled right afterwards
        except OSError:
            pass
        return name
    finally:
        os.close(folder)


def lower_priority():
    """Building a report keeps every core busy for ≈10 s: never at the cost of
    playback or the screen (the worker processes inherit this)."""
    os.nice(10)


def save_to(ctx, destination, topic=None, now=None, progress=None):
    """Build a report and save it onto Internal Storage or the SD card.

    Asked for by the owner on the player, so no opt-in is needed: nothing leaves
    the player unless the owner sends the file. It carries the install ID only
    when error reports are on."""
    if destination not in DESTINATIONS:
        raise ValueError('unknown_destination')
    if destination not in destinations(ctx):
        return {'saved': False, 'reason': 'not_mounted'}
    value = state(ctx)
    now = now or datetime.datetime.now(datetime.timezone.utc)
    progress = progress or _quiet
    data, index = build(ctx, 'manual', topic=topic,
                        install_id=value['install_id'] if value['enabled'] is True else None, now=now,
                        progress=progress)
    try:
        mount, top = DESTINATIONS[destination]
        progress('save', 0, force=True, bytes_total=len(data))
        from .media import sd_writable
        with (sd_writable(ctx, 'report') if destination == 'sd' else contextlib.nullcontext()):
            name = _write_into(ctx.path(mount), top, data, now.astimezone().strftime('%Y%m%d-%H%M%S'))
        progress('save', 1, force=True, bytes_total=len(data))
    except OSError as error:
        return {'saved': False, 'reason': 'no_space' if error.errno == errno.ENOSPC else 'write_failed'}
    value = state(ctx)
    value['last'] = {'report_id': index['report_id'], 'trigger': 'manual', 'state': 'saved',
                     'destination': destination, 'file': FOLDER + '/' + name,
                     'created_utc': index['created_utc'], 'bytes': len(data)}
    save(ctx, value)
    return {'saved': True, 'destination': destination, 'file': FOLDER + '/' + name,
            'report_id': index['report_id'], 'bytes': len(data)}


def queue(ctx, trigger, detail='', topic=None, now=None, progress=None, triggers=None):
    """Build and queue one report if the user opted in and the limits allow it.

    `triggers` names every trigger the report covers (the first is `trigger`).
    """
    triggers = list(triggers or [trigger])
    value = state(ctx)
    if value['enabled'] is not True:
        return {'queued': False, 'reason': 'not_enabled'}
    now = now or datetime.datetime.now(datetime.timezone.utc)
    day = now.date().isoformat()
    boot = ctx.read('/proc/sys/kernel/random/boot_id') or ''
    value['daily'] = {k: v for k, v in value['daily'].items() if k == day}
    if trigger != 'manual':
        if value['daily'].get(day, 0) >= DAILY_MAX:
            return {'queued': False, 'reason': 'daily_limit'}
        if all(t in value['boot_triggers'].get(boot, []) for t in triggers):
            return {'queued': False, 'reason': 'already_reported_this_boot'}
    data, index = build(ctx, trigger, detail, topic, value['install_id'], now, progress, triggers)
    directory = ctx.path(QUEUE)
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    name = '%s-%s.tar.gz' % (now.strftime('%Y%m%dT%H%M%SZ'), index['report_id'])
    temporary = directory / ('.' + name)
    with open(temporary, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, directory / name)
    for old in sorted(ctx.glob(QUEUE + '/*.tar.gz'))[:-QUEUE_MAX]:
        old.unlink(missing_ok=True)
    value['daily'][day] = value['daily'].get(day, 0) + 1
    value['boot_triggers'] = {boot: value['boot_triggers'].get(boot, []) + triggers}
    value['last'] = {'report_id': index['report_id'], 'trigger': trigger, 'state': 'queued',
                     'created_utc': index['created_utc'], 'bytes': len(data)}
    save(ctx, value)
    return {'queued': True, 'report_id': index['report_id'], 'bytes': len(data), 'name': name}


# ---------------------------------------------------------------- upload

def upload_key(ctx):
    """The build-wide upload key, or None when this build has none.

    It is public (it is in every image) and only keeps random internet traffic
    away from the receiver; the receiver's limits are the protection."""
    value = ctx.json(UPLOAD_KEY, {})
    key = value.get('upload_key') if isinstance(value, dict) else None
    return key if isinstance(key, str) and re.fullmatch(r'[A-Za-z0-9_-]{32,128}', key) else None


def upload_blocked(ctx, now):
    """Why queued reports must wait now, or None. Uploads run in the background:
    only on Wi-Fi, with a trustworthy clock (TLS), never during an update and
    never below MINIMUM_PERCENT on battery."""
    if upload_key(ctx) is None:
        return 'no_upload_key'
    if state(ctx)['enabled'] is not True:
        return 'not_enabled'
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    network = ctx.json('/run/y2/network.json', {}) or {}
    if network.get('state') != 'Online' or network.get('boot_id') != boot:
        return 'offline'
    from .timekeeping import status as clock
    if not clock(ctx, now=now).get('tls_ready'):
        return 'clock_not_ready'
    update = ctx.json('/data/updates/state.json', {}) or {}
    if update.get('state') not in (None, 'Idle', 'Acknowledged', 'RolledBack', 'Failed') or \
            ctx.path('/data/system/platform/maintenance-pending').exists():
        return 'update_in_progress'
    power = ctx.json('/run/y2/power.json', {}) or {}
    battery = (power.get('battery') or {}) if power.get('boot_id') in (None, boot) else {}
    charging = battery.get('charging') is True or battery.get('status') in ('Charging', 'Full')
    percent = battery.get('percent')
    if not charging and (type(percent) is not int or percent < MINIMUM_PERCENT):
        return 'battery_low'
    return None


def _queued_index(path):
    """report.json of a queued report (the first member: only it is decompressed)."""
    with tarfile.open(path, 'r:gz') as tar:
        member = tar.next()
        if member is None or member.name != 'report.json':
            raise ValueError('report_index_missing')
        return json.loads(tar.extractfile(member).read())


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """The receiver never redirects; a redirect is an error, never followed."""
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class _Body:
    """The report as urllib's request body, read in pieces so the owner's
    screen can follow the upload."""

    def __init__(self, data, progress):
        self.data, self.sent, self.progress = data, 0, progress

    def read(self, size=-1):
        size = 16384 if size is None or size < 0 else min(size, 16384)
        piece = self.data[self.sent:self.sent + size]
        self.sent += len(piece)
        self.progress('upload', self.sent / max(len(self.data), 1), bytes_done=self.sent, bytes_total=len(self.data))
        return piece


def _post(opener, data, index, key, progress=None):
    """POST one report; (HTTP status, answer) or (None, reason) without an answer."""
    release = index.get('release') or {}
    headers = {
        'Content-Type': 'application/gzip',
        'User-Agent': 'Y2Linux/%s' % (release.get('release_version') or 'unknown'),
        'X-Y2-Upload-Key': key,
        'X-Y2-Schema': str(index.get('schema', SCHEMA)),
        'X-Y2-Install-Id': str(index.get('install_id') or ''),
        'X-Y2-Report-Id': str(index.get('report_id') or ''),
        'X-Y2-Sha256': hashlib.sha256(data).hexdigest(),
        'X-Y2-Trigger': str(index.get('trigger') or ''),
        'X-Y2-Release': str(release.get('release_version') or ''),
        'Content-Length': str(len(data)),
    }
    body = _Body(data, progress) if progress else data
    request = urllib.request.Request(ENDPOINT, data=body, headers=headers, method='POST')
    try:
        with opener.open(request, timeout=30) as response:
            body = response.read(4096)
            status = response.status
    except urllib.error.HTTPError as error:
        with error:
            status, body = error.code, error.read(4096)
    except (urllib.error.URLError, OSError, TimeoutError) as error:
        return None, type(getattr(error, 'reason', error)).__name__
    try:
        answer = json.loads(body or b'{}')
    except ValueError:
        answer = {}
    return status, answer if isinstance(answer, dict) else {}


def _opener(context=None):
    handlers = [_NoRedirect(), urllib.request.ProxyHandler({})]
    if context is not None:
        handlers.append(urllib.request.HTTPSHandler(context=context))
    return urllib.request.build_opener(*handlers)


def _next_midnight(now):
    tomorrow = (datetime.datetime.fromtimestamp(now, datetime.timezone.utc) + datetime.timedelta(days=1)).date()
    return datetime.datetime(tomorrow.year, tomorrow.month, tomorrow.day, 0, 5,
                             tzinfo=datetime.timezone.utc).timestamp()


@workload('NetworkTransfer')
def upload(ctx, now=None, opener=None, progress=None, first=None):
    """Send every queued report that is due, oldest first.

    A stored report (201, or 200 for one the receiver already has) leaves the
    queue; one the receiver refuses (REFUSED) is dropped; a full day (429) waits
    for the next UTC day; no answer or a server error retries after RETRY_S and
    stops this run. A report still unsent GIVE_UP_S after it was made is
    dropped. Only one upload runs at a time.

    `first` (Send Report Now) is the owner's new report: it goes first and the
    progress follows it; once it is stored, the others go too without waiting
    for their retry time (the connection has just worked)."""
    now = time.time() if now is None else now
    blocked = upload_blocked(ctx, now)
    if blocked:
        return {'uploaded': [], 'reason': blocked}
    directory = ctx.path(QUEUE)
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = os.open(directory / '.upload.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {'uploaded': [], 'reason': 'busy'}
        return _upload_queue(ctx, now, opener or _opener(), upload_key(ctx), progress, first)
    finally:
        os.close(lock)


def _upload_queue(ctx, now, opener, key, progress=None, first=None):
    value = state(ctx)
    queued = sorted(ctx.glob(QUEUE + '/*.tar.gz'), key=lambda path: (path.name != first, path.name))
    connected = False
    retries = {name: entry for name, entry in value['uploads'].items()
               if isinstance(entry, dict) and name in {p.name for p in queued}}
    results = []

    def settle(index, outcome, **fields):
        last = value.get('last') or {}
        if last.get('report_id') == index.get('report_id'):
            last = {k: v for k, v in last.items() if k not in ('reason', 'next_attempt')}
            value['last'] = dict(last, state=outcome, **fields)

    for path in queued:
        entry = retries.get(path.name, {})
        if entry.get('next', 0) > now and not connected:
            continue
        shown = progress if first is None or path.name == first else None
        try:
            index = _queued_index(path)
            data = path.read_bytes()
        except (OSError, ValueError, tarfile.TarError, EOFError):
            path.unlink(missing_ok=True)
            results.append({'name': path.name, 'outcome': 'dropped', 'reason': 'unreadable'})
            continue
        created = index.get('created_utc')
        try:
            made = datetime.datetime.fromisoformat(created).timestamp()
        except (TypeError, ValueError):
            made = path.stat().st_mtime
        if now - made > GIVE_UP_S:
            path.unlink(missing_ok=True)
            retries.pop(path.name, None)
            settle(index, 'dropped', reason='expired')
            results.append({'name': path.name, 'outcome': 'dropped', 'reason': 'expired'})
            continue
        if shown:
            shown('upload', 0, force=True, bytes_done=0, bytes_total=len(data))
        status, answer = _post(opener, data, index, key, shown)
        if status in (200, 201):
            connected = connected or path.name == first
            path.unlink(missing_ok=True)
            retries.pop(path.name, None)
            settle(index, 'sent', sent_utc=datetime.datetime.fromtimestamp(now, datetime.timezone.utc).isoformat())
            results.append({'name': path.name, 'outcome': 'sent', 'report_id': answer.get('report_id')})
        elif status in REFUSED:
            path.unlink(missing_ok=True)
            retries.pop(path.name, None)
            reason = 'refused_%d_%s' % (status, answer.get('error', ''))
            settle(index, 'dropped', reason=reason)
            results.append({'name': path.name, 'outcome': 'dropped', 'reason': reason})
        else:
            attempts = entry.get('attempts', 0) + 1
            wait = _next_midnight(now) if status == 429 else now + RETRY_S[min(attempts, len(RETRY_S)) - 1]
            reason = 'HTTP_%d' % status if status is not None else answer
            retries[path.name] = {'attempts': attempts, 'next': wait, 'error': reason}
            settle(index, 'failed', reason=reason, next_attempt=wait)
            results.append({'name': path.name, 'outcome': 'retry', 'reason': reason, 'next': wait})
            if status != 429:
                break          # no answer or a server error: the rest would fare no better
    value['uploads'] = retries
    save(ctx, value)
    return {'uploaded': results}


def next_upload(ctx):
    """When the earliest queued report may be sent (epoch), or None when none waits."""
    queued = {p.name for p in ctx.glob(QUEUE + '/*.tar.gz')}
    if not queued:
        return None
    retries = state(ctx)['uploads']
    return min((retries.get(name) if isinstance(retries.get(name), dict) else {}).get('next', 0) for name in queued)


# ---------------------------------------------------------------- triggers

CRASH = re.compile(r'Kernel panic|Internal error: Oops|BUG: |Unable to handle kernel|kernel BUG at')
DARK = re.compile(r'DSI register reads still lag|flip_done timed out|vblank wait timed out')
# The internal eMMC only: a pulled SD card (mmcblk1) logs I/O errors too.
STORAGE = re.compile(r'EXT4-fs error|I/O error, dev mmcblk0|Remounting filesystem read-only|'
                     r'blk_update_request: I/O error, dev mmcblk0')


def crash_signature(ctx):
    """The newest retained kernel crash as (record time, kernel timestamp, text hash), or None.

    F50: a ramoops panic record stays in pstore over many starts, while the
    console and pmsg records change every start; a hash over all of them sent
    fcdf3a25's 10-25 panic again as 8e9181e5. The record time (pstore's mtime
    of a dmesg dump; up to 6 are kept, the newest counts) and the crash line's
    kernel timestamp name one crash; either matching the last one is enough,
    as RAM decay over a cold start may flip a digit of one. The text hash only
    serves records with neither."""
    found = []
    for path in sorted(ctx.glob('/sys/fs/pstore/*')):
        text = ctx.read(_rel(ctx, path), limit=1 << 20) or ''
        match = CRASH.search(text)
        if not match:
            continue
        stamp = re.search(r'\[\s*(\d+\.\d+)\]', text[text.rfind('\n', 0, match.start()) + 1:match.start()])
        when = 0
        if path.name.startswith('dmesg-'):
            with contextlib.suppress(OSError):
                when = int(path.stat().st_mtime)
        found.append((when, stamp.group(1) if stamp else '', text))
    if not found:
        return None
    when, uptime, text = max(found, key=lambda entry: entry[0])
    digest = '' if when > 0 or uptime else hashlib.sha256(text.encode()).hexdigest()[:16]
    return (str(when) if when > 0 else '', uptime, digest)


def _changed(value, name, current):
    """True once per new value of a watched marker."""
    if current in (None, '', 0) or value['seen'].get(name) == current:
        return False
    value['seen'][name] = current
    return True


def check(ctx):
    """Every automatic trigger that fired since the last check, as (trigger, detail)."""
    value = state(ctx)
    fired = []
    boot = ctx.read('/proc/sys/kernel/random/boot_id') or ''
    crash = crash_signature(ctx)
    if crash:
        seen = (str(value['seen'].get('pstore_crash') or '') + '||').split('|')[:3]
        # Before 8.42 the key was the all-records hash: a crash on record then
        # was already reported by that release (no resend after the update).
        known = 'pstore_crash' not in value['seen'] and 'pstore' in value['seen']
        value['seen']['pstore_crash'] = '|'.join(crash)
        if not known and not any(part and part == last for part, last in zip(crash, seen)):
            fired.append(('kernel_crash', 'previous boot'))
    tainted = ctx.integer('/proc/sys/kernel/tainted')
    if tainted and _changed(value, 'tainted', '%s:%s' % (boot, tainted)):
        fired.append(('kernel_warning', 'taint %d' % tainted))
    evidence = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if evidence.get('previous_orderly_shutdown') is False and \
            _changed(value, 'unclean', str(evidence.get('previous_boot_id') or boot)):
        # F56/F57: the PMIC's long-press record tells a held Power from a cut,
        # the loader's warm start a watchdog reset (or kernel restart) from both.
        pmic = evidence.get('pmic') if isinstance(evidence.get('pmic'), dict) else {}
        loader = evidence.get('loader') if isinstance(evidence.get('loader'), dict) else {}
        fired.append(('unclean_restart', '%s %s last stage %s loader %s pmic %s' % (
            evidence.get('reset_cause'), evidence.get('reset_cause_reason'), evidence.get('previous_last_stage'),
            '%s(%s)' % (loader['reset'], loader.get('boot_reason')) if loader.get('reset') else 'not_observed',
            pmic.get('long_press_reset_reason', 'not_observed'))))
    panic = ctx.path('/data/reborn/logs/panic-last.json')
    if panic.is_file():
        # One report per crash, not per rewrite: a crash at every start
        # (playlists.rs:223, #49) rewrote the file each restart, every boot.
        record = ctx.json('/data/reborn/logs/panic-last.json', {})
        where = [record.get(k) for k in ('version', 'location', 'message')] if isinstance(record, dict) else []
        signature = (hashlib.sha256(json.dumps(where).encode()).hexdigest()[:16] if any(where)
                     else str(int(panic.stat().st_mtime)))
        # A panic of an earlier release is not news on this one: a player that
        # updated past 8.12 resent its old #49 panic once (19a41489, d48362a7).
        # Its hash is still recorded and the file still goes into any report.
        version = record.get('version') if isinstance(record, dict) else None
        running = (ctx.json('/etc/y2linux/release.json', {}) or
                   ctx.json('/etc/y2linux/versions.json', {}) or {}).get('reborn_version')
        earlier = (isinstance(version, str) and isinstance(running, str) and running and
                   version != running and not version.startswith(running + '-'))
        if _changed(value, 'reborn_panic', signature) and not earlier:
            # Reborn 8.12 catches a worker's panic, skips that one file and runs on.
            isolated = isinstance(record, dict) and record.get('isolated') is True
            fired.append(('reborn_crash', 'isolated panic' if isolated else 'panic'))
    health = ctx.command(['rebornctl', 'health', '--json'], timeout=5.0, limit=262144)
    try:
        subsystems = json.loads(health.get('output') or '{}').get('subsystems', {})
    except ValueError:
        subsystems = {}
    failed = sorted(name for name, entry in subsystems.items()
                    if isinstance(entry, dict) and entry.get('state') == 'failed' and entry.get('required'))
    if failed and _changed(value, 'reborn_failed', '%s:%s' % (boot, ','.join(failed))):
        fired.append(('reborn_failure', ','.join(failed)))
    update = ctx.json('/data/updates/state.json', {})
    if update.get('state') in ('Failed', 'RolledBack', 'RescueRequired', 'RollbackPending') and \
            _changed(value, 'update', '%s:%s' % (update.get('sequence'), update.get('state'))):
        fired.append(('update_failure', str(update.get('state'))))
    # A sleep cancelled by a wake event (a button, the charger) is counted as a
    # failure with -EBUSY: routine on a player. Report a failed resume, tasks
    # that would not freeze, or a driver that refused with another error.
    stats = '/sys/power/suspend_stats/'
    fails = ctx.integer(stats + 'fail') or 0
    resumes = sum(ctx.integer(stats + name) or 0 for name in ('failed_resume', 'failed_resume_early',
                                                             'failed_resume_noirq'))
    frozen = ctx.integer(stats + 'failed_freeze') or 0
    code = (ctx.read(stats + 'last_failed_errno') or '').strip()
    real = resumes or frozen or (fails and code not in ('0', '-16'))
    if real and _changed(value, 'suspend_fail', '%s:%s:%s:%s' % (boot, fails, resumes, frozen)):
        fired.append(('sleep_failure', 'suspend_stats fail %d resume %d freeze %d errno %s step %s dev %s' % (
            fails, resumes, frozen, code or '?', (ctx.read(stats + 'last_failed_step') or '').strip() or '?',
            (ctx.read(stats + 'last_failed_dev') or '').strip() or '?')))
    # Only the radio itself: wifi-failure.json is rewritten on every Wi-Fi
    # connection and names the network's answers (wrong password, out of range).
    status_line = ctx.read('/sys/devices/platform/18070000.connectivity/status') or ''
    error = re.search(r'\berror=(-?\d+)', status_line)
    if error and error.group(1) != '0' and _changed(value, 'conn_error', '%s:%s' % (boot, error.group(1))):
        fired.append(('radio_failure', 'connectivity error=%s' % error.group(1)))
    kernel = ctx.command(['dmesg'], timeout=5.0, limit=2 * 1024 * 1024).get('output') or ''
    # The DSI read lag at boot (a restart through the recovery can leave the
    # SMI gate closed) is worked around by the driver: only a problem when
    # Reborn's first frame never reached the screen. Timeouts later always are.
    ready = ctx.json('/run/y2/application-ready.json', {}) or {}
    lit = ready.get('boot_id') == boot and ready.get('first_frame') is True
    dark = [m.group(0) for m in DARK.finditer(kernel) if not (lit and m.group(0).startswith('DSI register'))]
    if dark and _changed(value, 'dark', boot):
        fired.append(('display_failure', dark[0]))
    storage = STORAGE.search(kernel)
    ext4 = sum(ctx.integer(_rel(ctx, p)) or 0 for p in ctx.glob('/sys/fs/ext4/*/errors_count'))
    if (storage or ext4) and _changed(value, 'storage', '%s:%s:%d' % (boot, bool(storage), ext4)):
        fired.append(('storage_error', storage.group(0) if storage else 'ext4 errors_count %d' % ext4))
    save(ctx, value)
    return fired


def run_checks(ctx):
    """Queue one report for the triggers fired in this check (when enabled).

    They usually share a cause (a crash loop: unclean restart, Reborn crash and
    a rolled-back update within seconds) and one report carries all of them;
    one report each sent near copies against the daily limit.
    """
    if state(ctx)['enabled'] is not True:
        return []
    fired = check(ctx)
    if not fired:
        return []
    boot = ctx.read('/proc/sys/kernel/random/boot_id') or ''
    reported = state(ctx)['boot_triggers'].get(boot, [])
    fired = [(t, d) for t, d in fired if t not in reported] or fired
    trigger = fired[0][0]
    detail = fired[0][1] if len(fired) == 1 else '; '.join('%s: %s' % pair for pair in fired)
    triggers = [t for t, _ in fired]
    return [dict(queue(ctx, trigger, detail, triggers=triggers), trigger=trigger, triggers=triggers)]


def upload_now(ctx, progress=None, first=None):
    """upload() for callers whose own work must not fail with it (a queued report
    stays queued): a sleep in progress or an I/O error becomes the reason."""
    try:
        return upload(ctx, progress=progress, first=first)
    except (OSError, ValueError) as error:
        return {'uploaded': [], 'reason': str(error) if isinstance(error, ValueError) else type(error).__name__}
