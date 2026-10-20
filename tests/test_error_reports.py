"""Error reports (ERROR-REPORTS-PLAN R2): no planted secret survives, the evidence does."""
import datetime
import gzip
import io
import json
import os
import re
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


def plain(value):
    """Tokens without their per-report hash: '<ssid#3fa2c1>' -> '<ssid#>'."""
    return re.sub(r'#[0-9a-f]{6}>', '#>', value if isinstance(value, str) else json.dumps(value))


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
        self.assertIn('<ip:private#>', plain(log))
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

    def test_parallel_scrubbing_matches_one_process_and_tokens_hold_across_files(self):
        write(self.root, '/data/logs/system.log.0', ''.join(
            'line %d: wlan0 joined %s from %s\n' % (n, SSID, PRIVATE_IP) for n in range(30000)))
        known = report.secrets(self.ctx)
        items = [(path, (Path(self.root) / path.lstrip('/')).read_bytes())
                 for path in ('/data/logs/system.log.0', '/data/logs/system.log', '/data/reborn/logs/reborn-current.jsonl')]
        self.assertGreater(len(report._parts(*items[0])), 2)
        one = report.Scrubber(known)
        alone = [report._scrub_blob(one, path, data) for path, data in items]     # whole files, one process
        pool = report._pool(known, one.key)
        self.assertIsNotNone(pool)
        try:
            together = report.Scrubber(known, one.key)
            parallel = report._scrub_files(together, known, items, pool)
        finally:
            pool.terminate()
        self.assertEqual(parallel, alone)
        self.assertEqual(together.counts, one.counts)
        token = one.text(SSID)
        self.assertIn(token.encode(), parallel[0])               # every piece of the big log
        self.assertIn(token.encode(), parallel[2])               # and Reborn's log
        self.assertNotIn(SSID.encode(), b''.join(parallel))
        self.assertNotIn(one.key.hex().encode(), b''.join(parallel))
        self.assertNotEqual(report.Scrubber(known).text(SSID), token)   # a new report, new tokens

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
        write(self.root, '/sys/devices/platform/18070000.connectivity/status',
              'activated=1 powered=1 chip=6582 error=-110 transport_errors=0\n')
        fired = dict(report.check(self.ctx))
        self.assertEqual(set(fired), {'unclean_restart', 'update_failure', 'kernel_warning', 'sleep_failure',
                                      'storage_error', 'radio_failure'})   # the panic was reported above
        self.assertIn('watchdog', fired['unclean_restart'])
        self.assertEqual(fired['radio_failure'], 'connectivity error=-110')
        self.assertEqual(report.check(self.ctx), [])
        # A sleep a wake event cancelled (-EBUSY while suspending devices) is routine.
        write(self.root, '/sys/power/suspend_stats/fail', '5\n')
        write(self.root, '/sys/power/suspend_stats/last_failed_errno', '-16\n')
        write(self.root, '/sys/power/suspend_stats/last_failed_step', 'suspend\n')
        self.assertEqual(report.check(self.ctx), [])
        write(self.root, '/sys/power/suspend_stats/failed_resume', '1\n')
        self.assertEqual([t for t, _ in report.check(self.ctx)], ['sleep_failure'])
        # Wi-Fi joining a network, or a wrong password / an absent network, is no radio failure.
        for error in (None, 'wrong_credentials', 'ap_unavailable'):
            write(self.root, '/run/y2/wifi-failure.json', json.dumps({'error': error, 'failures': 1}))
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

    def test_one_check_sends_one_report_for_all_its_triggers(self):
        # a8125bf1 (8.10): unclean restart, Reborn crash and rolled-back update
        # within seconds went out as three near copies against the daily limit.
        report.set_enabled(self.ctx, True)
        write(self.root, '/data/system/platform/previous-boot-evidence.json',
              json.dumps({'previous_orderly_shutdown': False, 'previous_boot_id': 'boot-0',
                          'reset_cause': 'None', 'previous_last_stage': 'application_ready'}))
        write(self.root, '/data/reborn/logs/panic-last.json',
              json.dumps({'version': '8.10', 'location': 'playlists.rs:223', 'message': 'not a char boundary'}))
        write(self.root, '/data/updates/state.json', json.dumps({'state': 'RolledBack', 'sequence': 7500}))
        result = report.run_checks(self.ctx)
        self.assertEqual(len(result), 1)
        self.assertTrue(result[0]['queued'])
        self.assertEqual(result[0]['triggers'], ['unclean_restart', 'reborn_crash', 'update_failure'])
        self.assertEqual(len(report.status(self.ctx)['queued']), 1)
        queued = sorted((Path(self.root) / report.QUEUE.lstrip('/')).glob('*.tar.gz'))
        index = json.loads(members(queued[0].read_bytes())['report.json'])
        self.assertEqual((index['trigger'], index['triggers']), ('unclean_restart', result[0]['triggers']))
        self.assertIn('reborn_crash: panic', index['detail'])
        self.assertEqual(report.run_checks(self.ctx), [])

    def test_a_crash_loop_reports_its_panic_once(self):
        report.set_enabled(self.ctx, True)
        panic = Path(self.root) / 'data/reborn/logs/panic-last.json'
        record = {'version': '8.11', 'location': 'playlists.rs:223', 'message': 'not a char boundary'}
        fired = []
        for second in range(5):                                  # rewritten at every restart
            write(self.root, '/data/reborn/logs/panic-last.json', json.dumps(record | {'session': str(second)}))
            os.utime(panic, (2_000_000_000 + second, 2_000_000_000 + second))
            fired += [t for t, _ in report.check(self.ctx)]
        self.assertEqual(fired, ['reborn_crash'])
        write(self.root, '/data/reborn/logs/panic-last.json', json.dumps(record | {'location': 'other.rs:1'}))
        self.assertEqual(report.check(self.ctx), [('reborn_crash', 'panic')])
        # A worker panic Reborn caught (one file skipped, still running) is reported, named apart.
        write(self.root, '/data/reborn/logs/panic-last.json',
              json.dumps(record | {'location': 'worker.rs:9', 'isolated': True}))
        self.assertEqual(report.check(self.ctx), [('reborn_crash', 'isolated panic')])
        write(self.root, '/data/reborn/logs/panic-last.json',
              json.dumps(record | {'location': 'worker.rs:9', 'isolated': True, 'session': 'next'}))
        self.assertEqual(report.check(self.ctx), [])

    @unittest.skipUnless(Path('/proc/version').is_file(), 'needs procfs')
    def test_files_without_a_size_are_read_through(self):
        # seq_file refuses SEEK_END: /proc and debugfs were left out of every report.
        data, cut = report._read_bytes(Context('/'), '/proc/version')
        self.assertTrue(data.startswith(b'Linux version'))
        self.assertFalse(cut)
        self.assertIn('/sys/kernel/debug/regmap/*-0030/registers', report.FILES)

    def mount(self, *paths, options='rw,relatime'):
        lines = ['22 1 179:2 / / ro,relatime - ext4 /dev/root ro']
        for n, path in enumerate(paths):
            (Path(self.root) / path.lstrip('/')).mkdir(parents=True, exist_ok=True)
            lines.append('%d 22 179:%d / %s %s - vfat /dev/mmcblk1p1 rw' % (30 + n, 8 + n, path, options))
        write(self.root, '/proc/self/mountinfo', '\n'.join(lines) + '\n')

    def test_a_report_is_saved_onto_mounted_storage_without_opting_in(self):
        self.assertEqual(report.status(self.ctx)['destinations'], [])
        self.assertEqual(report.save_to(self.ctx, 'sd', 'battery')['reason'], 'not_mounted')
        self.mount('/media/sd', '/media/internal')
        # Internal Storage over MTP is the music folder: without it, nowhere to put the file.
        self.assertEqual(report.status(self.ctx)['destinations'], ['sd'])
        (Path(self.root) / 'media/internal/music').mkdir()
        status = report.status(self.ctx)
        self.assertEqual(status['destinations'], ['internal', 'sd'])
        self.assertIs(status['upload'], False)          # no upload key in this tree
        now = datetime.datetime(2026, 10, 19, 12, tzinfo=datetime.timezone.utc)
        first = report.save_to(self.ctx, 'sd', 'battery', now=now)
        second = report.save_to(self.ctx, 'sd', 'battery', now=now)
        self.assertTrue(first['saved'])
        self.assertRegex(first['file'], r'^Error Reports/y2linux-report-\d{8}-\d{6}\.tar\.gz$')
        self.assertEqual(second['file'], first['file'][:-7] + '-2.tar.gz')
        folder = Path(self.root) / 'media/sd/Error Reports'
        self.assertEqual(sorted(p.name.startswith('.') for p in folder.iterdir()), [False, False])
        everything = members((Path(self.root) / 'media/sd' / first['file']).read_bytes())
        index = json.loads(everything['report.json'])
        self.assertEqual((index['trigger'], index['topic'], index['install_id']), ('manual', 'battery', None))
        for name, body in everything.items():
            for canary in CANARIES:
                self.assertNotIn(canary, body.decode('utf-8', errors='replace'), '%s in %s' % (canary, name))
        last = report.status(self.ctx)['last']
        self.assertEqual((last['state'], last['destination'], last['file']), ('saved', 'sd', second['file']))
        self.assertIsNone(report.status(self.ctx)['enabled'])    # saving is not an answer to the question
        self.assertEqual(report.status(self.ctx)['queued'], [])  # and queues nothing
        report.set_enabled(self.ctx, True)
        saved = report.save_to(self.ctx, 'internal', now=now)
        self.assertFalse((Path(self.root) / 'media/internal/Error Reports').exists())
        index = json.loads(members((Path(self.root) / 'media/internal/music' / saved['file']).read_bytes())['report.json'])
        self.assertEqual(index['install_id'], report.status(self.ctx)['install_id'])

    def test_the_command_saves_at_low_priority(self):
        import contextlib
        import sys
        from unittest.mock import patch
        from y2_platform import cli
        self.mount('/media/sd')
        output = io.StringIO()
        with patch.object(cli, 'Context', return_value=self.ctx), patch.object(report.os, 'nice') as nice, \
                patch.object(sys, 'argv', ['y2-platform', 'report', 'save', '--to', 'sd', '--topic', 'battery']), \
                contextlib.redirect_stdout(output):
            self.assertEqual(cli.main(), 0)
        nice.assert_called_once_with(10)
        result = json.loads(output.getvalue())
        self.assertTrue(result['saved'])
        self.assertTrue((Path(self.root) / 'media/sd' / result['file']).is_file())

    def test_saving_refuses_read_only_storage_and_a_folder_link(self):
        self.mount('/media/sd', options='ro,relatime')
        self.assertEqual(report.status(self.ctx)['destinations'], [])
        self.assertEqual(report.save_to(self.ctx, 'sd')['reason'], 'not_mounted')
        self.mount('/media/sd')
        elsewhere = Path(self.root) / 'data/elsewhere'
        elsewhere.mkdir(parents=True)
        (Path(self.root) / 'media/sd/Error Reports').symlink_to(elsewhere)
        self.assertEqual(report.save_to(self.ctx, 'sd')['reason'], 'write_failed')
        self.assertEqual(list(elsewhere.iterdir()), [])
        with self.assertRaises(ValueError):
            report.save_to(self.ctx, 'usb')

    def test_the_boot_id_is_not_taken_for_a_volume_serial(self):
        boot = '63c1a2b4-5d6e-4f70-8192-a3b4c5d6e7f8'
        write(self.root, '/proc/sys/kernel/random/boot_id', boot + '\n')
        write(self.root, '/run/y2/media-lifecycle.json', json.dumps({'boot_id': boot, 'uuid': 'C4A2-77E1'}))
        found = report.secrets(self.ctx)['serial']
        self.assertNotIn(boot, found)
        self.assertIn('C4A2-77E1', found)

    def test_json_lines_scrubbed_as_text_keep_the_rules_of_values(self):
        s = report.Scrubber({'ssid': {'Café Wörth'}, 'music': {'Canary Song Title'}})
        lines = [json.dumps({'event': 'joined', 'message': 'joined Café Wörth'}),             # \\u escapes
                 json.dumps({'event': 'joined', 'message': 'joined Café Wörth'}, ensure_ascii=False),
                 json.dumps({'event': 'play', 'Title': 'Anything At All', 'n': 1}),            # the key rule
                 json.dumps({'peer': {'address': '11:22:33:44:55:66', 'name': 'Neighbour Buds'}}),
                 json.dumps({'note': 'from /media/sd/A Folder/x.flac to 192.168.1.20'}),
                 json.dumps({'volume': {'uuid': 'C4A2-77E1', 'label': ''}, 'artist': ['Canary "Band"', 'B']}),
                 json.dumps({'volume': {'UUID': 'Say \\"hi\\"', 'n': 2}})]
        out = report._scrub_blob(s, '/data/reborn/logs/reborn-current.jsonl', '\n'.join(lines).encode()).decode()
        for secret in ('Caf', 'Anything At All', 'Neighbour', 'A Folder', '192.168.1.20', '44:55:66',
                       'C4A2', 'Canary', 'hi'):
            self.assertNotIn(secret, out)
        rows = [json.loads(line) for line in out.splitlines()]
        self.assertEqual(rows[0]['message'], rows[1]['message'])       # one token for both forms
        self.assertEqual(plain(rows[0]['message']), 'joined <ssid#>')
        self.assertEqual(rows[2]['Title'], '<title>')
        self.assertEqual(rows[3]['peer']['name'], '<bt_name>')
        self.assertEqual(plain(rows[4]['note']), 'from /media/sd/<path>.flac to <ip:private#>')
        self.assertEqual(rows[5], {'volume': {'uuid': '<uuid>', 'label': ''}, 'artist': ['<artist>', '<artist>']})
        self.assertEqual(rows[6], {'volume': {'UUID': '<uuid>', 'n': 2}})

    def test_unpaired_bluetooth_names_next_to_an_address_are_private(self):
        s = report.Scrubber({})
        self.assertEqual(plain(s.value({'devices': [{'name': 'Neighbour Headphones', 'address': '11:22:33:44:55:66'}]})),
                         plain({'devices': [{'name': '<bt_name>', 'address': '11:22:33:<mac#>'}]}))
        self.assertEqual(plain(s.value({'name': 'BAT0', 'type': 'Battery'})), plain({'name': 'BAT0', 'type': 'Battery'}))


class Triggers(unittest.TestCase):
    def test_a_boot_time_dsi_lag_counts_only_when_no_frame_reached_the_screen(self):
        with tempfile.TemporaryDirectory() as root:
            player(root)
            lag = '[    1.011318] mtk-dsi 1400c000.dsi: modeset: DSI register reads still lag after 3 power cycles\n'
            dmesg = {'output': lag}

            def run(argv, **kwargs):
                return dict(dmesg, ok=True, reason=None) if argv[0] == 'dmesg' else runner(argv, **kwargs)
            ctx = Context(root, runner=run)
            write(root, '/run/y2/application-ready.json', json.dumps({'boot_id': 'boot-1', 'first_frame': True}))
            fired = [t for t, _ in report.check(ctx)]
            self.assertNotIn('display_failure', fired)                   # the panel lit: worked around
            self.assertEqual(report.check(ctx), [])
            write(root, '/run/y2/application-ready.json', json.dumps({'boot_id': 'boot-0', 'first_frame': True}))
            self.assertEqual([t for t, _ in report.check(ctx)], ['display_failure'])   # no frame this boot
            write(root, '/proc/sys/kernel/random/boot_id', 'boot-2\n')
            write(root, '/run/y2/application-ready.json', json.dumps({'boot_id': 'boot-2', 'first_frame': True}))
            dmesg['output'] = lag + '[  900.0] [drm] flip_done timed out\n'
            self.assertEqual(report.check(ctx), [('display_failure', 'flip_done timed out')])

    def test_storage_errors_are_the_internal_emmcs_not_a_pulled_card(self):
        for line in ('I/O error, dev mmcblk0, sector 4096 op 0x0:(READ)', 'EXT4-fs error (device mmcblk0p8): x',
                     'EXT4-fs (mmcblk0p8): Remounting filesystem read-only'):
            self.assertTrue(report.STORAGE.search(line), line)
        for line in ('I/O error, dev mmcblk1, sector 0 op 0x0:(READ)', 'mmc1: card 59b4 removed',
                     'FAT-fs (mmcblk1p1): Filesystem has been set read-only'):
            self.assertFalse(report.STORAGE.search(line), line)


class Scrubbing(unittest.TestCase):
    def test_patterns(self):
        s = report.Scrubber({'ssid': {'Home'}})
        self.assertEqual(plain(s.text('Home wifi, Homepage')), plain('<ssid#> wifi, Homepage'))
        self.assertEqual(plain(s.text('fe80::1c2d:3eff:fe4f:5a6b up')), plain('<ip6:link#> up'))
        self.assertEqual(plain(s.text('std::fmt and a::b')), plain('std::fmt and a::b'))
        self.assertEqual(plain(s.text('at 20:39:14 v6.18.56 build 2026.08')), plain('at 20:39:14 v6.18.56 build 2026.08'))
        self.assertEqual(plain(s.text('00:00:00:00:00:00 ff:ff:ff:ff:ff:ff')), plain('00:00:00:00:00:00 ff:ff:ff:ff:ff:ff'))
        self.assertEqual(plain(s.text('/media/sd/Some Folder/x y.mp3, then')), plain('/media/sd/<path>.mp3, then'))
        self.assertEqual(plain(s.text('dir /media/internal/A Folder Name')), plain('dir /media/internal/<path>'))
        self.assertEqual(plain(s.text('serial=ABCDEF0123456789')), plain('serial=<serial#>'))
        self.assertEqual(plain(s.value({'title': 'x', 'n': ['Home']})), plain({'title': '<title>', 'n': ['<ssid#>']}))
        self.assertEqual(plain(s.value({'path': '/media/sd/a', 'uuid': '6ACE-5706'})), plain({'path': '/media/sd/<path>', 'uuid': '<uuid>'}))
        self.assertEqual(plain(s.text('/dev/mmcblk1p1: UUID="6ACE-5706" TYPE="vfat"')), plain('/dev/mmcblk1p1: UUID="<serial#>" TYPE="vfat"'))


if __name__ == '__main__':
    unittest.main()
