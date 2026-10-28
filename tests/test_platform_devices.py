"""Differential cases of port group `devices` that the shared checks of tests/test_platform_differential.py
cannot see as equal (tests/platform_cases/devices.py, EXTRA_CASES): Python against Python, and against the Rust
host build when `Y2_PLATFORM_RUST` is set. They run on the same player-shaped fixture root."""
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_differential import compare, python_side, run_case, rust_binary, rust_side, RUST_ENV  # noqa: E402
from platform_cases import devices  # noqa: E402
import test_platform_differential  # noqa: E402

STAGE_ID = re.compile(r'[0-9a-f]{32}')


def renamed(side):
    """`side`, with the stage directory `transfer begin` named by a random id renamed to a fixed one."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        result = side(root, argv, commands_file, calls_file, timeout, env)
        staging = root / 'data/transfer-staging'
        for entry in staging.iterdir() if staging.is_dir() else ():
            if STAGE_ID.fullmatch(entry.name):
                entry.rename(staging / ('f' * 32))
        return result
    return run


def options(case):
    """A case's options for run_case, and the side wrapper it needs."""
    case = dict(case)
    wrap = renamed if case.pop('rename_stage', False) else (lambda side: side)
    case.pop('needs_no_dropbear', None)
    if wrap is renamed:
        case['patterns'] = [(STAGE_ID.pattern, '<id>')]
    return case, wrap


def runnable(case):
    return not (case.get('needs_no_dropbear') and os.path.exists('/usr/sbin/dropbear'))


class DevicesDifferential(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('devices',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_python_against_python_is_identical(self):
        for name, argv, case in devices.EXTRA_CASES:
            with self.subTest(name):
                if not runnable(case):
                    self.skipTest('the host has /usr/sbin/dropbear')
                case, wrap = options(case)
                self.assertEqual(compare(self.fixture, argv, left=wrap(python_side), right=wrap(python_side),
                                         **case), [])

    def test_cases_exercise_the_paths_they_name(self):
        results = {name: run_case(options(case)[1](python_side), self.fixture, argv, **options(case)[0])
                   for name, argv, case in devices.EXTRA_CASES if runnable(case)}
        stage = '/data/transfer-staging/' + 'f' * 32
        self.assertEqual(results['transfer-begin']['stdout']['state'], 'Staging')
        self.assertEqual(results['transfer-begin']['tree'][stage]['mode'], '0o700')
        commit = results['transfer-commit']
        self.assertEqual((commit['exit'], commit['stdout']['state']), (0, 'Committed'))
        self.assertEqual(commit['tree']['/data/music/Album/song.flac']['content'], ['hello'])
        self.assertNotIn(devices.STAGE + '/payload.part', commit['tree'])
        self.assertIn('/media/internal/music/Album/song.flac', results['transfer-commit-internal']['tree'])
        self.assertEqual(results['transfer-commit-internal-unverified']['stdout']['reason'],
                         'verified_internal_media_unavailable')
        self.assertEqual(results['transfer-commit-exists']['stdout']['type'], 'FileExistsError')
        self.assertEqual(results['transfer-commit-sealed-left']['stdout']['reason'],
                         "[Errno 17] File exists: 'sealed.part'")
        self.assertEqual(results['transfer-commit-wrong-hash']['stdout']['reason'], 'upload_hash_or_size_mismatch')
        crashed = results['transfer-commit-record-not-dict']
        self.assertEqual((crashed['exit'], crashed['stdout']), (1, []))
        self.assertIn('/data/music/Album/song.flac', crashed['tree'])
        if 'ssh-starts-dropbear' in results:
            self.assertEqual(results['ssh-starts-dropbear']['stdout']['type'], 'FileNotFoundError')

    @unittest.skipUnless(rust_binary(), RUST_ENV + ' (host build of the Rust y2-platform) not given')
    def test_rust_matches_python(self):
        rust = rust_side(rust_binary())
        for name, argv, case in devices.EXTRA_CASES:
            with self.subTest(name):
                if not runnable(case):
                    self.skipTest('the host has /usr/sbin/dropbear')
                case, wrap = options(case)
                found = compare(self.fixture, argv, left=wrap(python_side), right=wrap(rust), **case)
                self.assertEqual(found, [])


if __name__ == '__main__':
    unittest.main()
