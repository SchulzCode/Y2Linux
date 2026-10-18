"""Opt-in error reports (docs/planning/ERROR-REPORTS-PLAN.md).

A report holds everything the player records about an error: kernel, platform,
update/recovery and Reborn logs and state, hardware identity. Every byte passes
`Scrubber` before it is written: the player's own secrets (Wi-Fi names and
passphrases, Bluetooth names, account data, serial numbers, the songs in the
queue) are replaced wherever they appear, then patterns (MAC and IP addresses,
e-mail, URL paths, music paths), and JSON values under music keys. Library
contents, account files, keys and anything below /media are never read into a
report at all. Nothing here uploads; the queue waits for the uploader.
"""
# SPDX-License-Identifier: GPL-2.0-only
import datetime
import gzip
import hashlib
import io
import ipaddress
import json
import os
import re
import tarfile
import time
import uuid
from pathlib import Path

from .common import atomic_json

SCHEMA = 1
STATE = '/data/system/platform/reports.json'
QUEUE = '/data/system/platform/reports'
QUEUE_MAX = 3
DAILY_MAX = 5
BUDGET = 8 * 1024 * 1024          # compressed report
SECTION_MAX = 4 * 1024 * 1024     # raw bytes per section (logs keep their newest end)
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
    serial = re.search(r'(?mi)^Serial\s*:\s*(\S+)', ctx.read('/proc/cpuinfo') or '')
    if serial:
        found['serial'].add(serial.group(1))
    session = _large_json(ctx, '/data/reborn/state/session.json')
    model = session.get('model', {}) if isinstance(session, dict) else {}
    for track in (model.get('queue') or [])[:5000] if isinstance(model, dict) else []:
        if isinstance(track, dict):
            for key in ('title', 'artist', 'album', 'album_artist', 'filename', 'genre'):
                if isinstance(track.get(key), str):
                    found['music'].add(track[key])
    # Too short to replace safely (would hit ordinary words and numbers).
    return {name: {v for v in values if isinstance(v, str) and len(v.strip()) >= 3 and v not in COMMON_WORDS
                   and v.strip() not in ('0000000000000000',)} for name, values in found.items()}


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
    """Replace personal data; tokens are stable within one report."""

    def __init__(self, known=None):
        self.tokens = {}
        self.counts = {}
        pairs = []
        for name, values in (known or {}).items():
            for value in values:
                pairs.append((value, name))
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
        return self.token(self.known[value], value)

    def token(self, name, value, keep=''):
        key = (name, value)
        if key not in self.tokens:
            self.tokens[key] = sum(1 for k in self.tokens if k[0] == name) + 1
        self.counts[name] = self.counts.get(name, 0) + 1
        return '<%s%s#%d>' % (name, keep, self.tokens[key])

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
        value = MEDIA.sub(self._media, value)
        value = URL.sub(lambda m: '%s://%s/<path>' % (m.group(1), m.group(2)) if m.group(3) not in ('', '/') else m.group(0), value)
        if self.known_pattern:
            value = self.known_pattern.sub(self._known, value)
        value = EMAIL.sub(lambda m: self.token('email', m.group(0)), value)
        value = SERIAL_FIELD.sub(lambda m: m.group(1) + m.group(2) + self.token('serial', m.group(3)), value)
        value = MAC.sub(self._mac, value)
        value = IPV6.sub(self._ipv6, value)
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

def _tail(data, limit=SECTION_MAX):
    if len(data) <= limit:
        return data, False
    cut = data[-limit:]
    newline = cut.find(b'\n')
    return (cut[newline + 1:] if 0 <= newline < 4096 else cut), True


def _read_bytes(ctx, path, limit=SECTION_MAX):
    try:
        with open(ctx.path(path), 'rb') as stream:
            stream.seek(0, os.SEEK_END)
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
    '/data/logs/recovery-display.txt',
    '/proc/version', '/proc/cmdline', '/proc/meminfo', '/proc/vmstat', '/proc/loadavg', '/proc/interrupts',
    '/proc/softirqs', '/proc/stat', '/proc/pressure/*', '/proc/mounts', '/proc/sys/kernel/tainted',
    '/sys/power/suspend_stats/*', '/sys/kernel/debug/wakeup_sources', '/sys/kernel/debug/suspend_stats',
    '/sys/kernel/debug/regulator/regulator_summary', '/sys/kernel/debug/clk/clk_summary',
    '/sys/kernel/debug/regmap/1-0030/registers',
    '/sys/devices/system/cpu/cpu*/cpuidle/state*/usage', '/sys/devices/system/cpu/cpu*/cpuidle/state*/time',
    '/sys/devices/system/cpu/cpu*/cpuidle/state*/name',
    '/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state', '/sys/devices/system/cpu/online',
    '/sys/class/thermal/thermal_zone*/temp', '/sys/class/thermal/thermal_zone*/type',
    '/sys/class/power_supply/*/uevent', '/sys/fs/ext4/*/errors_count', '/sys/fs/ext4/*/first_error_time',
    '/sys/devices/platform/11200000.usb/status', '/sys/devices/platform/11200000.usb/rxdiag',
    '/data/system/platform/boot.json', '/data/system/platform/previous-boot-evidence.json',
    '/data/system/platform/power-journal.jsonl', '/data/system/platform/sleep-diagnostics.jsonl',
    '/data/system/platform/suspend-last.json', '/data/system/platform/product-sleep-guard.json',
    '/data/system/platform/rtc-policy.json', '/data/system/platform/time-last.json',
    '/data/system/platform/battery-ui.json', '/data/system/platform/panel.json',
    '/data/system/platform/retained/*.tar',
    '/data/updates/*.json', '/data/updates/current/manifest.json',
    '/run/y2/*.json', '/run/y2/*.jsonl', '/run/y2/*.log',
    '/data/bluetooth/codec-history.json', '/data/bluetooth/enabled', '/data/network/enabled',
    '/data/network/power-save.json',
    '/data/reborn/logs/*.jsonl', '/data/reborn/logs/panic-last.json', '/data/reborn/diagnostics/*.tar.gz',
)
# Never read into a report, even if a glob above ever matched them.
NEVER = re.compile(r'^/(media|data/music|data/ssh|root/\.ssh)/|wpa_supplicant|lastfm\.json|scrobbles\.jsonl|'
                   r'library\.db|/bluez/|machine-id|entropy|/keys?/|\.pem$|\.key$|session\.json')


def _scrub_blob(scrubber, path, data):
    """One file's bytes, scrubbed by its kind (JSON, JSON lines, tar, text)."""
    if path.endswith(('.tar', '.tar.gz')):
        return _scrub_tar(scrubber, data, path.endswith('.gz'))
    text = data.decode('utf-8', errors='replace')
    if path.endswith('.jsonl'):
        lines = []
        for line in text.splitlines():
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


def build(ctx, trigger, detail='', topic=None, install_id=None, now=None):
    """The complete report (tar.gz bytes) and its index."""
    if trigger not in TRIGGERS:
        raise ValueError('unknown_trigger')
    if topic is not None and topic not in TOPICS:
        raise ValueError('unknown_topic')
    scrubber = Scrubber(secrets(ctx))
    now = now or datetime.datetime.now(datetime.timezone.utc)
    versions = ctx.json('/etc/y2linux/versions.json', {})
    local = datetime.datetime.now().astimezone()
    index = {
        'schema': SCHEMA, 'report_id': str(uuid.uuid4()), 'install_id': install_id,
        'trigger': trigger, 'detail': scrubber.text(detail or ''), 'topic': topic,
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
    for name, result in _commands(ctx).items():
        output = (result or {}).get('output') or ''
        data, cut = _tail(output.encode())
        try:
            clean = json.dumps(scrubber.value(json.loads(data)), indent=1, sort_keys=True).encode()
        except ValueError:
            clean = scrubber.text(data.decode('utf-8', errors='replace')).encode()
        add('commands/%s.txt' % name, clean, 'command' + ('' if result.get('ok') else ' (%s)' % result.get('reason')), cut)
    seen = set()
    for pattern in FILES:
        for path in ctx.glob(pattern) if '*' in pattern else [ctx.path(pattern)]:
            relative = '/' + str(path.relative_to(ctx.root)).lstrip('/')
            if relative in seen or NEVER.search(relative) or not path.is_file():
                continue
            seen.add(relative)
            data, cut = _read_bytes(ctx, relative)
            if data is None:
                continue
            # Scrubbed archives are re-packed uncompressed (the report itself is gzip).
            name = relative[:-3] if relative.endswith('.tar.gz') else relative
            add('files' + name, _scrub_blob(scrubber, relative, data), relative, cut)
    index['scrubbed'] = dict(sorted(scrubber.counts.items()))
    return _pack(index, members), index


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
            'queued': queued, 'last': value.get('last')}


def queue(ctx, trigger, detail='', topic=None, now=None):
    """Build and queue one report if the user opted in and the limits allow it."""
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
        if trigger in value['boot_triggers'].get(boot, []):
            return {'queued': False, 'reason': 'already_reported_this_boot'}
    data, index = build(ctx, trigger, detail, topic, value['install_id'], now)
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
    value['boot_triggers'] = {boot: value['boot_triggers'].get(boot, []) + [trigger]}
    value['last'] = {'report_id': index['report_id'], 'trigger': trigger, 'state': 'queued',
                     'created_utc': index['created_utc'], 'bytes': len(data)}
    save(ctx, value)
    return {'queued': True, 'report_id': index['report_id'], 'bytes': len(data), 'name': name}


# ---------------------------------------------------------------- triggers

CRASH = re.compile(r'Kernel panic|Internal error: Oops|BUG: |Unable to handle kernel|kernel BUG at')
DARK = re.compile(r'DSI register reads still lag|flip_done timed out|vblank wait timed out')
STORAGE = re.compile(r'EXT4-fs error|I/O error, dev mmcblk|Remounting filesystem read-only|blk_update_request: I/O error')


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
    pstore = ''.join(ctx.read(_rel(ctx, p), limit=1 << 20) or '' for p in ctx.glob('/sys/fs/pstore/*'))
    if CRASH.search(pstore) and _changed(value, 'pstore', hashlib.sha256(pstore.encode()).hexdigest()):
        fired.append(('kernel_crash', 'previous boot'))
    tainted = ctx.integer('/proc/sys/kernel/tainted')
    if tainted and _changed(value, 'tainted', '%s:%s' % (boot, tainted)):
        fired.append(('kernel_warning', 'taint %d' % tainted))
    evidence = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if evidence.get('previous_orderly_shutdown') is False and \
            _changed(value, 'unclean', str(evidence.get('previous_boot_id') or boot)):
        fired.append(('unclean_restart', '%s %s last stage %s' % (
            evidence.get('reset_cause'), evidence.get('reset_cause_reason'), evidence.get('previous_last_stage'))))
    panic = ctx.path('/data/reborn/logs/panic-last.json')
    if panic.is_file() and _changed(value, 'reborn_panic', str(int(panic.stat().st_mtime))):
        fired.append(('reborn_crash', 'panic'))
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
    fails = ctx.integer('/sys/power/suspend_stats/fail')
    if fails and _changed(value, 'suspend_fail', '%s:%s' % (boot, fails)):
        fired.append(('sleep_failure', 'suspend_stats fail %d' % fails))
    wifi = ctx.path('/run/y2/wifi-failure.json')
    if wifi.is_file() and _changed(value, 'wifi_failure', str(int(wifi.stat().st_mtime))):
        fired.append(('radio_failure', 'wifi'))
    status_line = ctx.read('/sys/devices/platform/18070000.connectivity/status') or ''
    error = re.search(r'\berror=(-?\d+)', status_line)
    if error and error.group(1) != '0' and _changed(value, 'conn_error', '%s:%s' % (boot, error.group(1))):
        fired.append(('radio_failure', 'connectivity error=%s' % error.group(1)))
    kernel = ctx.command(['dmesg'], timeout=5.0, limit=2 * 1024 * 1024).get('output') or ''
    if DARK.search(kernel) and _changed(value, 'dark', boot):
        fired.append(('display_failure', DARK.search(kernel).group(0)))
    storage = STORAGE.search(kernel)
    ext4 = sum(ctx.integer(_rel(ctx, p)) or 0 for p in ctx.glob('/sys/fs/ext4/*/errors_count'))
    if (storage or ext4) and _changed(value, 'storage', '%s:%s:%d' % (boot, bool(storage), ext4)):
        fired.append(('storage_error', storage.group(0) if storage else 'ext4 errors_count %d' % ext4))
    save(ctx, value)
    return fired


def run_checks(ctx):
    """Queue a report for each fired trigger (when enabled); returns what happened."""
    if state(ctx)['enabled'] is not True:
        return []
    return [dict(queue(ctx, trigger, detail), trigger=trigger) for trigger, detail in check(ctx)]
