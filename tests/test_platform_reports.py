"""Golden cases of port group `reports` that need more than tests/test_platform_differential.py's plain run
(tests/platform_cases/reports.py, EXTRA_CASES), run by the Rust host build (`Y2_PLATFORM_RUST`) on the
player-shaped fixture root and compared with tests/platform_goldens/reports/<case>.json (tests/platform_golden.py).

A report or export left on disk carries a random ID and the wall clock in its name, tokens keyed by a per-report
random key, member times, and deflate bytes (miniz's). `normalised` renames such files and replaces each archive by
a JSON listing of its members (name, mode, size, owner; the data as text with every token numbered in order of
appearance, IDs and clock values replaced), so the tree difference shows the archive contents. The upload request's
random headers are replaced in the recorded calls. `plan_first` runs `reset plan` on the same root before the case's
`reset execute|resume`, with the confirmation the plan printed.
"""
import base64
import grp
import gzip
import hashlib
import io
import os
import pwd
import random
import json
import re
import shutil
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import SKIP, check_all, read_golden, run_case, rust_binary, touched  # noqa: E402
from platform_cases import reports  # noqa: E402
import test_platform_differential  # noqa: E402

UUID = re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
NAMES = [(re.compile(r'\d{8}T\d{6}Z-' + UUID.pattern), '<stamp>-<uuid>'),
         (re.compile(r'y2linux-report-\d{8}-\d{6}'), 'y2linux-report-<stamp>'),
         (re.compile(r'state-[0-9a-f]{32}'), 'state-<hex>'), (re.compile(r'y2-diagnostics-\d+-[0-9a-f]{32}'),
                                                              'y2-diagnostics-<id>')]
TOKEN = re.compile(r'#([0-9a-f]{6})>')
PLACES = ('data/system/platform/reports', 'media/internal/music/Error Reports', 'media/sd/Error Reports',
          'data/exports')
FIXED = set(reports.ARCHIVES)
LIBRARY = Path(__file__).resolve().parent / 'platform_cases/library.db'


def text(data, tokens):
    """Member data as text with tokens numbered by first appearance and IDs replaced."""
    value = data.decode('utf-8', errors='replace')
    value = TOKEN.sub(lambda m: '#%d>' % tokens.setdefault(m.group(1), len(tokens)), value)
    value = UUID.sub('<uuid>', value)
    return re.sub(r'(?m)^(\d+\.\d+|\d+) (\d+\.\d+)$', '<uptime>', value)


def owner(name, names):
    # A member copied from the fixture is owned by whoever runs the test: its name is the host's.
    return '<owner>' if name and name in names else name


def summary(value):
    """Member text over 64 KiB as its line count, digest and first and last lines (the golden stays small)."""
    if len(value) <= 65536:
        return value
    lines = value.splitlines()
    return {'lines': len(lines), 'sha256': hashlib.sha256(value.encode('utf-8', 'surrogatepass')).hexdigest(),
            'head': lines[:3], 'tail': lines[-3:]}


def listing(data):
    """An archive's members in order, normalised."""
    tokens, members = {}, []
    users = set()
    for lookup in (lambda: pwd.getpwuid(os.geteuid()).pw_name, lambda: grp.getgrgid(os.getegid()).gr_name):
        try:
            users.add(lookup())
        except KeyError:
            pass
    with tarfile.open(fileobj=io.BytesIO(gzip.decompress(data)), mode='r:') as archive:
        for member in archive.getmembers():
            body = archive.extractfile(member).read() if member.isfile() else b''
            entry = {'name': member.name, 'mode': oct(member.mode), 'type': member.type.decode(),
                     'uname': owner(member.uname, users), 'gname': owner(member.gname, users)}
            if member.name.endswith('report-progress.json'):
                entry['data'] = '<progress>'          # the run's own live record, read while it ran
            elif member.name.endswith('.tar') and body:
                inner = tarfile.open(fileobj=io.BytesIO(body))
                entry['data'] = [[m.name, text(inner.extractfile(m).read(), tokens)] for m in inner.getmembers()]
            else:
                entry['data'] = text(body, tokens)
                try:
                    entry['data'] = json.loads(entry['data'])
                except ValueError:
                    entry['data'] = summary(entry['data'])
            members.append(entry)
    return members


def normalised(side):
    """`side`, with archives the run left renamed and listed, and the upload's random headers replaced."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        result = side(root, argv, commands_file, calls_file, timeout, env)
        for place in PLACES:
            folder = root / place
            for path in sorted(folder.glob('*.tar.gz')) if folder.is_dir() else ():
                if path.name in FIXED:
                    continue
                name = path.name
                for pattern, replacement in NAMES:
                    name = pattern.sub(replacement, name)
                data = path.read_bytes()
                path.unlink()
                (folder / (name + '.members.json')).write_text(json.dumps(listing(data), sort_keys=True))
        calls = calls_file.read_text()
        calls = re.sub(r'X-Y2-Sha256: [0-9a-f]{64}', 'X-Y2-Sha256: <sha>', calls)
        calls = re.sub(r'Content-Length: \d+', 'Content-Length: <bytes>', UUID.sub('<uuid>', calls))
        calls_file.write_text(calls)
        return result
    return run


def planned(side, scope):
    """`side`, running `reset plan --scope <scope>` first and confirming what it planned; the quarantine
    named by the plan's random ID is renamed to a fixed one."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        side(root, ['reset', 'plan', '--scope', scope], commands_file, calls_file, timeout, env)
        plan = json.loads((root / 'data/system/platform/reset-plan.json').read_text())
        result = side(root, [*argv, '--confirm', plan['confirmation']], commands_file, calls_file, timeout, env)
        quarantine = root / 'data/reset-quarantine' / plan['id']
        if quarantine.is_dir():
            quarantine.rename(quarantine.parent / ('f' * 32))
        return result
    return run


def with_library(side):
    """`side`, with Reborn's library database (binary, so not a case's `files`) put in the root first: a kept
    file (tracks: 3 rows, a table "odd name", user_version 7), since its bytes name the SQLite that wrote it."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        path = root / 'data/reborn/library.db'
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(LIBRARY, path)
        return side(root, argv, commands_file, calls_file, timeout, env)
    return run


def tar(members, compressed=False):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w') as archive:
        for name, data in members:
            info = tarfile.TarInfo(name)
            info.size, info.mtime = len(data), 1761000000
            archive.addfile(info, io.BytesIO(data))
        directory = tarfile.TarInfo('folder')
        directory.type = tarfile.DIRTYPE
        archive.addfile(directory)
    return gzip.compress(raw.getvalue(), mtime=0) if compressed else raw.getvalue()


def with_archives(side):
    """`side`, with retained boot evidence, a Reborn diagnostics archive and a large log made in the root first:
    the scrubber's tar path, the parallel parts of a log and the 2 MiB budget (the log is halved)."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        retained = root / 'data/system/platform/retained/boot-1.tar'
        retained.parent.mkdir(parents=True, exist_ok=True)
        retained.write_bytes(tar([('boot.log', b'joined Home from 10.0.0.7\n'),
                                  ('evidence.json', b'{"ssid": "Home", "n": 1}'), ('x.jsonl', b'{"a": 1}\n')]))
        diagnostics = root / 'data/reborn/diagnostics/d.tar.gz'
        diagnostics.parent.mkdir(parents=True, exist_ok=True)
        diagnostics.write_bytes(tar([('panic.txt', b'at /media/sd/Music/a.flac\n')], compressed=True))
        (root / 'data/reborn/diagnostics/broken.tar.gz').write_bytes(b'not an archive')
        noise = random.Random(6582)
        lines = [base64.b64encode(noise.randbytes(90)).decode() + ' 192.168.0.%d' % (n % 250) for n in range(40000)]
        (root / 'data/logs').mkdir(parents=True, exist_ok=True)
        (root / 'data/logs/system.log').write_text('\n'.join(lines) + '\n')
        return side(root, argv, commands_file, calls_file, timeout, env)
    return run


def options(case):
    """A case's options for run_case, with the side wrapper it needs."""
    case = dict(case)
    archives, scope = case.pop('archives', False), case.pop('plan_first', None)
    library, archives_made = case.pop('library', False), case.pop('retained', False)

    def wrap(side):
        side = planned(side, scope) if scope else side
        side = with_library(side) if library else side
        side = with_archives(side) if archives_made else side
        return normalised(side) if archives else side
    return {**case, 'wrap': wrap}


class ReportsGoldens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('reports',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [(('reports', name), lambda argv=argv, case=case: run_case(self.fixture, argv, **options(case)))
                         for name, argv, case in reports.EXTRA_CASES])

    def test_goldens_exercise_the_paths_they_name(self):
        results = {name: read_golden('reports', name) for name, _, _ in reports.EXTRA_CASES}
        queue = '/' + reports.QUEUE.lstrip('/') + '/<stamp>-<uuid>.tar.gz.members.json'
        sent = results['report-send-queued']
        self.assertEqual(sent['stdout']['upload']['reason'], 'offline')
        members = touched(sent)[queue]['content']
        self.assertEqual([m['name'] for m in members[:4]],
                         ['report.json', 'hardware.json', 'reborn/session.json', 'reborn/library.json'])
        self.assertEqual(members[0]['data']['topic'], 'crash')
        log = next(m for m in members if m['name'] == 'files/data/logs/system.log')['data']
        self.assertNotIn('Home', log)
        self.assertNotIn('passphrase', log)
        self.assertTrue({'ssid', 'wifi_secret', 'email', 'ip', 'mac', 'media_path'} <=
                        set(members[0]['data']['scrubbed']))
        self.assertEqual(results['report-send']['stdout']['upload']['uploaded'][0]['outcome'], 'retry')
        self.assertEqual(results['report-check-queues']['stdout']['reports'][0]['queued'], True)
        self.assertTrue(results['report-save-internal']['stdout']['saved'])
        self.assertIn('/media/internal/music/Error Reports/y2linux-report-<stamp>.tar.gz.members.json',
                      touched(results['report-save-internal']))
        self.assertTrue(results['report-save-sd']['stdout']['saved'])
        self.assertEqual(results['report-save-sd']['calls'][-1]['argv'], ['mount', '-o', 'remount,rw', '/media/sd'])
        export = touched(results['export-state'])['/data/exports/state-<hex>.tar.gz.members.json']['content']
        self.assertEqual([m['name'] for m in export], ['manifest.json', 'reborn--state--session.json',
                                                       'network--wpa_supplicant.conf', 'network--enabled',
                                                       'library.db'])
        library = next(m for m in members if m['name'] == 'reborn/library.json')['data']
        self.assertEqual((library['tables'], library['by_codec'], library['by_source_id'], library['user_version']),
                         ({'tracks': 3}, [['flac', 2], ['mp3', 1]], [['source#1', 2], ['source#2', 1]], 7))
        self.assertEqual(results['export-state']['stdout']['contains_network_credentials'], True)
        settings = results['reset-execute-settings']
        self.assertEqual((settings['exit'], settings['stdout']['state']), (0, 'Complete'))
        session = touched(settings)['/data/reborn/state/session.json']['content']
        self.assertEqual(session['model']['settings'], {'volume': 5})
        self.assertEqual(results['reset-execute-network']['stdout']['completed'], ['network'])
        self.assertEqual(results['reset-execute-defaults-unavailable']['stdout']['failure'],
                         'application_defaults_unavailable')
        big = touched(results['report-send-retained'])[queue]['content']
        index = big[0]['data']
        self.assertTrue(index['sections']['files/data/logs/system.log']['cut'])
        retained = next(m for m in big if m['name'] == 'files/data/system/platform/retained/boot-1.tar')['data']
        self.assertEqual([name for name, _ in retained], ['boot.log', 'evidence.json', 'x.jsonl'])
        self.assertIn('files/data/reborn/diagnostics/d.tar', index['sections'])
        diagnostics = results['export-diagnostics-complete']
        self.assertEqual(diagnostics['stdout']['state'], 'Complete')
        bundle = touched(diagnostics)['/data/exports/y2-diagnostics-<id>.tar.gz.members.json']['content'][0]['data']
        self.assertEqual(bundle['logs']['system'][0]['components'], ['msdc'])
        self.assertIn('checks', bundle['health'])
        refused = results['reset-execute-plan-no-expiry']
        self.assertEqual((refused['exit'], refused['stdout']),
                         (1, {'state': 'Failed', 'failure': 'reset_plan_unreadable'}))


if __name__ == '__main__':
    unittest.main()
