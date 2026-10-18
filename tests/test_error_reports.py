"""Error reports (ERROR-REPORTS-PLAN R2): no planted secret survives, the evidence does."""
import datetime
import gzip
import io
import json
import sqlite3
import tarfile
import tempfile
import unittest
from pathlib import Path

from y2_platform import report
from y2_platform.common import Context

SSID, PSK = 'CanaryNet Home', 'canary-pass-9876'
BT_NAME, BT_MAC = 'Canary Buds Pro', '14:14:7D:AB:CD:EF'
LASTFM_USER, LASTFM_KEY = 'canaryuser42', 'c4n4rys3ss10nk3y'
SSH_BLOB, SSH_COMMENT = 'AAAACcanaryKEYblob0123', 'canary@laptop'
CID, MACHINE_ID, CPU_SERIAL = '150100464e58324d4206ec82a80434f9', '6f1d2c3b4a5e6f708192a3b4c5d6e7f8', '00000000deadbeef'
TITLE, ARTIST = 'Canary Song Title', 'Canary Band'
ZONE, PRIVATE_IP, PUBLIC_IP, EMAIL = 'Europe/Berlin', '192.168.123.45', '93.184.216.34', 'canary@example.org'
BSSID = '3C:22:FB:9A:BC:DE'
# Every planted secret, as it may appear (MACs also in BlueZ's underscore form).
CANARIES = [SSID, PSK, BT_NAME, 'AB:CD:EF', 'AB_CD_EF', LASTFM_USER, LASTFM_KEY, SSH_BLOB, SSH_COMMENT,
            CID, MACHINE_ID, 'deadbeef', TITLE, ARTIST, ZONE, '123.45', PUBLIC_IP, EMAIL, '9A:BC:DE',
            'Canary Band/', 'canary-station-token', 'NEVER-canary-value', 'C4A2-77E1']


def write(root, path, text, mode='w'):
    target = Path(root) / path.lstrip('/')
    target.parent.mkdir(parents=True, exist_ok=True)
    if mode == 'wb':
        target.write_bytes(text)
    else:
        target.write_text(text)
    return target


def player(root):
    """A synthetic player with a secret in every place one could be."""
    write(root, '/data/network/wpa_supplicant.conf',
          'ctrl_interface=/run/wpa\nnetwork={\n ssid="%s"\n psk="%s"\n bssid=%s\n}\n' % (SSID, PSK, BSSID))
    write(root, '/data/bluetooth/bluez/A2:5F:D7:57:80:23/%s/info' % BT_MAC, '[General]\nName=%s\nAlias=%s\n' % (BT_NAME, BT_NAME))
    write(root, '/data/bluetooth/codec-history.json', json.dumps({'peers': {BT_MAC: {'codec': 'AAC'}}}))
    write(root, '/data/reborn/state/lastfm.json', json.dumps({'user': LASTFM_USER, 'session_key': LASTFM_KEY}))
    write(root, '/root/.ssh/authorized_keys', 'ssh-ed25519 %s %s\nssh-ed25519 %s Y2Linux\n' % (SSH_BLOB, SSH_COMMENT, SSH_BLOB))
    card = '/sys/class/mmc_host/mmc0/mmc0:0001/'
    for name, value in {'cid': CID, 'serial': '0xec82a804', 'name': 'FNX2MB', 'manfid': '0x000015',
                        'date': '03/2017', 'prv': '0x6', 'fwrev': '0x0600000000000000'}.items():
        write(root, card + name, value + '\n')
    write(root, '/data/system/dbus/machine-id', MACHINE_ID + '\n')
    write(root, '/proc/cpuinfo', 'processor\t: 0\nCPU part\t: 0xc07\nSerial\t\t: %s\n' % CPU_SERIAL)
    write(root, '/proc/meminfo', 'MemTotal:         950636 kB\n')
    write(root, '/proc/sys/kernel/random/boot_id', 'boot-1\n')
    write(root, '/proc/sys/kernel/tainted', '0\n')
    write(root, '/proc/uptime', '123.4 400.0\n')
    write(root, '/sys/firmware/y2_boot/lcm', 'lcm=0-st7701_hvga_dsi_vdo_boe\n')
    write(root, '/sys/module/panel/parameters/panel_kind', '2\n')
    path = '/media/internal/music/%s/%s.flac' % (ARTIST, TITLE)
    write(root, '/data/reborn/state/session.json', json.dumps({'schema': 2, 'model': {
        'queue': [{'id': 7, 'title': TITLE, 'artist': ARTIST, 'album': 'Canary Album', 'path': path,
                   'filename': TITLE + '.flac', 'codec': 'flac', 'sample_rate': 96000, 'duration_ms': 1000}]
                 + [{'id': n, 'title': 'Filler %d' % n, 'path': '/media/sd/f%d.mp3' % n} for n in range(3000)],
        'queue_position': 0, 'position_ms': 500, 'playback': 'paused', 'output': {'kind': 'wired'},
        'settings': {'volume': 47, 'hp_level': 'low', 'time_zone': ZONE, 'music_directory': '/media/internal/music'}}}))
    write(root, '/data/reborn/logs/reborn-current.jsonl', '\n'.join(json.dumps(e) for e in [
        {'subsystem': 'alsa', 'event': 'prepared', 'fields': {'device': 'bluealsa:DEV=%s,PROFILE=a2dp' % BT_MAC}},
        {'subsystem': 'wifi', 'event': 'connected', 'message': 'joined %s from %s' % (SSID, PRIVATE_IP), 'fields': {'ssid': SSID}},
        {'subsystem': 'lastfm', 'event': 'sent', 'fields': {'url': 'https://ws.audioscrobbler.com/2.0/?sk=%s&user=%s' % (LASTFM_KEY, LASTFM_USER)}},
        {'subsystem': 'scanner', 'event': 'unreadable', 'message': 'failed to open %s: EIO' % path},
        {'subsystem': 'core', 'event': 'mail', 'message': 'contact %s via %s, usb 10.42.0.1' % (EMAIL, PUBLIC_IP)},
        {'subsystem': 'radio', 'event': 'stream', 'message': 'opened https://stream.example.net/live/canary-station-token'},
    ]) + '\n')
    write(root, '/data/reborn/logs/panic-last.json', json.dumps({'message': 'index out of bounds playing %s' % TITLE,
                                                                  'location': 'app/reborn/src/playback.rs:1200'}))
    write(root, '/data/logs/system.log',
          'kernel: mmc0: new HS200 card cid %s\nkernel: wlan0: associate with %s (SSID=\'%s\')\n'
          'kernel: bt peer dev_%s connected\nkernel: reborn_audio::native ok at 20:39:14, kernel 6.18.56\n'
          'kernel: Y2Linux stays Y2Linux\n'
          % (CID, BSSID, SSID, BT_MAC.replace(':', '_')))
    write(root, '/sys/fs/pstore/console-ramoops-0', '<0>Kernel panic - not syncing: Attempted to kill init!\n')
    # A file a report glob matches but must never read (the NEVER list).
    write(root, '/run/y2/wpa_supplicant-debug.log', 'NEVER-canary-value\n')
    write(root, '/run/y2/media.json', json.dumps({'sources': [{'kind': 'sd_card', 'uuid': 'C4A2-77E1', 'mount': '/media/sd'}]}))
    write(root, '/data/reborn/logs/splash-boot.jsonl', json.dumps({'event': 'source', 'fields': {'source': {'kind': 'sd_card', 'id': 'C4A2-77E1'}}}) + '\n')
    write(root, '/run/y2/network.json', json.dumps({'ssid': SSID, 'ip': PRIVATE_IP, 'gateway': '192.168.123.1'}))
    write(root, '/data/updates/state.json', json.dumps({'state': 'Acknowledged', 'sequence': 6900}))
    write(root, '/data/system/platform/previous-boot-evidence.json',
          json.dumps({'previous_orderly_shutdown': True, 'previous_boot_id': 'boot-0', 'reset_cause': None}))
    # A retained boot archive and a Reborn diagnostic bundle, both holding a secret.
    for name, compressed in (('/data/system/platform/retained/boot-x.tar', False),
                             ('/data/reborn/diagnostics/reborn-diagnostic-1-2.tar.gz', True)):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode='w:gz' if compressed else 'w') as tar:
            body = json.dumps({'wifi': {'ssid': SSID}, 'note': 'peer %s' % BT_NAME}).encode()
            info = tarfile.TarInfo('diagnostic.json')
            info.size = len(body)
            tar.addfile(info, io.BytesIO(body))
        write(root, name, buffer.getvalue(), 'wb')
    db = Path(root) / 'data/reborn/library.db'
    connection = sqlite3.connect(db)
    connection.execute('create table tracks (id integer, title text, artist text, path text, codec text, sample_rate integer)')
    connection.execute('insert into tracks values (1, ?, ?, ?, "flac", 96000)', (TITLE, ARTIST, path))
    connection.commit()
    connection.close()


def runner(argv, **kwargs):
    outputs = {
        'dmesg': '[    1.0] wlan0: authenticated with %s SSID %s\n[    2.0] mtk-dsi: handoff ok\n' % (BSSID, SSID),
        'rebornctl': json.dumps({'bluetooth': {'devices': [{'name': BT_NAME, 'address': BT_MAC, 'connected': True}]},
                                 'wifi': {'ssid': SSID, 'ip': PRIVATE_IP}, 'subsystems': {}}),
    }
    name = argv[0]
    if name in outputs:
        return {'ok': True, 'reason': None, 'output': outputs[name]}
    return {'ok': False, 'reason': 'command_unavailable', 'output': None}


def members(data):
    with tarfile.open(fileobj=io.BytesIO(gzip.decompress(data))) as tar:
        found = {}
        for member in tar.getmembers():
            body = tar.extractfile(member).read()
            if member.name.endswith(('.tar', '.tar.gz')):
                inner = gzip.decompress(body) if body[:2] == b'\x1f\x8b' else body
                with tarfile.open(fileobj=io.BytesIO(inner)) as nested:
                    for item in nested.getmembers():
                        found[member.name + '!' + item.name] = nested.extractfile(item).read()
            found[member.name] = body
        return found


class Report(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = self.temp.name
        player(self.root)
        self.ctx = Context(self.root, runner=runner)

    def tearDown(self):
        self.temp.cleanup()

    def test_no_planted_secret_reaches_the_report(self):
        data, index = report.build(self.ctx, 'manual', 'from %s' % SSID, 'sound', 'install-1')
        everything = members(data)
        for name, body in everything.items():
            text = body.decode('utf-8', errors='replace')
            for canary in CANARIES:
                self.assertNotIn(canary, text, '%s in %s' % (canary, name))
                self.assertNotIn(canary, name)
        self.assertGreater(index['scrubbed'].get('ssid', 0), 3)
        for name in ('mac', 'ip', 'music', 'serial', 'media_path', 'private_key', 'bt_name', 'email'):
            self.assertIn(name, index['scrubbed'])

    def test_the_evidence_survives_scrubbing(self):
        data, index = report.build(self.ctx, 'kernel_crash', 'previous boot', None, 'install-1')
        everything = {k: v.decode('utf-8', errors='replace') for k, v in members(data).items()}
        self.assertIn('Kernel panic - not syncing', everything['files/sys/fs/pstore/console-ramoops-0'])
        log = everything['files/data/reborn/logs/reborn-current.jsonl']
        self.assertIn('"prepared"', log)
        self.assertIn('14:14:7D:<mac#', log)                     # maker kept, device masked
        self.assertIn('<ip:private#1>', log)
        self.assertIn('10.42.0.1', log)                          # the USB network stays
        self.assertIn('/media/internal/<path>.flac: EIO', log)   # text after the file stays
        self.assertIn('https://ws.audioscrobbler.com/<path>', log)
        system = everything['files/data/logs/system.log']
        self.assertIn('3C:22:FB:<mac#', system)
        self.assertIn('dev_14_14_7D_<mac#', system)
        self.assertIn('reborn_audio::native ok at 20:39:14, kernel 6.18.56', system)
        self.assertIn('Y2Linux stays Y2Linux', everything['files/data/logs/system.log'])   # a key comment, not a secret
        session = json.loads(everything['reborn/session.json'])
        self.assertEqual(session['settings']['volume'], 47)
        self.assertEqual(session['settings']['time_zone'], 'set')
        self.assertEqual(session['queue_length'], 3001)
        self.assertEqual(session['current_track'], {'codec': 'flac', 'sample_rate': 96000, 'duration_ms': 1000})
        library = json.loads(everything['reborn/library.json'])
        self.assertEqual(library['tables'], {'tracks': 1})
        self.assertEqual(library['by_codec'], [['flac', 1]])
        hardware = json.loads(everything['hardware.json'])
        self.assertEqual(hardware['storage']['mmc0']['name'], 'FNX2MB')
        self.assertNotIn('cid', hardware['storage']['mmc0'])
        self.assertIn('lcm=0-st7701', hardware['y2_boot']['lcm'])
        self.assertIn('bluetooth', everything['commands/reborn_status.txt'])
        self.assertIn("'<ssid#", everything['files/data/logs/system.log'])
        index_json = json.loads(everything['report.json'])
        self.assertEqual(index_json['trigger'], 'kernel_crash')
        self.assertEqual(index_json['install_id'], 'install-1')
        # Never read: library contents, account files, keys, the raw session.
        for name in everything:
            self.assertNotRegex(name, r'library\.db|lastfm|wpa_supplicant|authorized_keys|state/session\.json|bluez')

    def test_a_huge_log_is_cut_to_the_budget(self):
        import os
        write(self.root, '/data/logs/system.log.0', '', 'w')
        with open(Path(self.root) / 'data/logs/system.log.0', 'wb') as stream:
            stream.write(os.urandom(12 * 1024 * 1024).hex().encode()[:20 * 1024 * 1024])
        data, index = report.build(self.ctx, 'manual', install_id='i')
        self.assertLessEqual(len(data), report.BUDGET)
        self.assertTrue(index['sections']['files/data/logs/system.log.0']['cut'])

    def test_queue_is_opt_in_and_bounded(self):
        now = datetime.datetime(2026, 10, 19, 12, tzinfo=datetime.timezone.utc)
        self.assertEqual(report.queue(self.ctx, 'manual', now=now)['reason'], 'not_enabled')
        self.assertIsNone(report.status(self.ctx)['enabled'])
        status = report.set_enabled(self.ctx, True)
        self.assertRegex(status['install_id'], r'^[0-9a-f-]{36}$')
        first = report.queue(self.ctx, 'reborn_crash', now=now)
        self.assertTrue(first['queued'])
        self.assertEqual(report.queue(self.ctx, 'reborn_crash', now=now)['reason'], 'already_reported_this_boot')
        for minute in range(4):
            self.assertTrue(report.queue(self.ctx, 'manual', now=now + datetime.timedelta(minutes=minute + 1))['queued'])
        self.assertEqual(len(report.status(self.ctx)['queued']), report.QUEUE_MAX)
        for n, trigger in enumerate(('kernel_warning', 'sleep_failure', 'radio_failure', 'storage_error')):
            result = report.queue(self.ctx, trigger, now=now + datetime.timedelta(minutes=10 + n))
        self.assertEqual(result['reason'], 'daily_limit')       # 5 automatic a day
        old = report.status(self.ctx)['install_id']
        self.assertNotEqual(report.new_install_id(self.ctx)['install_id'], old)
        self.assertEqual(report.set_enabled(self.ctx, False)['queued'], [])

    def test_triggers_fire_once_each(self):
        fired = dict(report.check(self.ctx))
        self.assertEqual(fired, {'kernel_crash': 'previous boot', 'reborn_crash': 'panic'})
        self.assertEqual(report.check(self.ctx), [])
        write(self.root, '/data/system/platform/previous-boot-evidence.json',
              json.dumps({'previous_orderly_shutdown': False, 'previous_boot_id': 'boot-0',
                          'reset_cause': 'watchdog', 'previous_last_stage': 'ui'}))
        write(self.root, '/data/updates/state.json', json.dumps({'state': 'RolledBack', 'sequence': 6901}))
        write(self.root, '/proc/sys/kernel/tainted', '512\n')
        write(self.root, '/sys/power/suspend_stats/fail', '2\n')
        write(self.root, '/sys/fs/ext4/mmcblk0p8/errors_count', '1\n')
        write(self.root, '/run/y2/wifi-failure.json', '{}')
        fired = dict(report.check(self.ctx))
        self.assertEqual(set(fired), {'unclean_restart', 'update_failure', 'kernel_warning', 'sleep_failure',
                                      'storage_error', 'radio_failure'})   # the panic was reported above
        self.assertIn('watchdog', fired['unclean_restart'])
        self.assertEqual(report.check(self.ctx), [])
        self.assertEqual(report.run_checks(self.ctx), [])        # not enabled: nothing queued

    def test_opting_in_reports_only_what_happens_afterwards(self):
        report.set_enabled(self.ctx, True)                       # old pstore and panic records exist
        self.assertEqual(report.run_checks(self.ctx), [])
        write(self.root, '/data/reborn/logs/panic-last.json', '{"message": "new"}')
        import os
        os.utime(Path(self.root) / 'data/reborn/logs/panic-last.json', (2_000_000_000, 2_000_000_000))
        result = report.run_checks(self.ctx)
        self.assertEqual([(r['trigger'], r['queued']) for r in result], [('reborn_crash', True)])

    def test_unpaired_bluetooth_names_next_to_an_address_are_private(self):
        s = report.Scrubber({})
        self.assertEqual(s.value({'devices': [{'name': 'Neighbour Headphones', 'address': '11:22:33:44:55:66'}]}),
                         {'devices': [{'name': '<bt_name>', 'address': '11:22:33:<mac#1>'}]})
        self.assertEqual(s.value({'name': 'BAT0', 'type': 'Battery'}), {'name': 'BAT0', 'type': 'Battery'})


class Scrubbing(unittest.TestCase):
    def test_patterns(self):
        s = report.Scrubber({'ssid': {'Home'}})
        self.assertEqual(s.text('Home wifi, Homepage'), '<ssid#1> wifi, Homepage')
        self.assertEqual(s.text('fe80::1c2d:3eff:fe4f:5a6b up'), '<ip6:link#1> up')
        self.assertEqual(s.text('std::fmt and a::b'), 'std::fmt and a::b')
        self.assertEqual(s.text('at 20:39:14 v6.18.56 build 2026.08'), 'at 20:39:14 v6.18.56 build 2026.08')
        self.assertEqual(s.text('00:00:00:00:00:00 ff:ff:ff:ff:ff:ff'), '00:00:00:00:00:00 ff:ff:ff:ff:ff:ff')
        self.assertEqual(s.text('/media/sd/Some Folder/x y.mp3, then'), '/media/sd/<path>.mp3, then')
        self.assertEqual(s.text('dir /media/internal/A Folder Name'), 'dir /media/internal/<path>')
        self.assertEqual(s.text('serial=ABCDEF0123456789'), 'serial=<serial#1>')
        self.assertEqual(s.value({'title': 'x', 'n': ['Home']}), {'title': '<title>', 'n': ['<ssid#1>']})
        self.assertEqual(s.value({'path': '/media/sd/a', 'uuid': '6ACE-5706'}), {'path': '/media/sd/<path>', 'uuid': '<uuid>'})
        self.assertEqual(s.text('/dev/mmcblk1p1: UUID="6ACE-5706" TYPE="vfat"'), '/dev/mmcblk1p1: UUID="<serial#2>" TYPE="vfat"')


if __name__ == '__main__':
    unittest.main()
