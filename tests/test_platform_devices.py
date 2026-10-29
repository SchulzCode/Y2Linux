"""Golden cases of port group `devices` that need more than tests/test_platform_differential.py's plain run
(tests/platform_cases/devices.py, EXTRA_CASES): the stage directory `transfer begin` names by a random id is
renamed to a fixed one, `transfer commit` past its stage lock, an ssh-daemon that starts dropbear, and tampered
records it refuses. Run by the Rust host build (`Y2_PLATFORM_RUST`) and compared with
tests/platform_goldens/devices/<case>.json (tests/platform_golden.py)."""
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import SKIP, check_all, read_golden, run_case, rust_binary, touched  # noqa: E402
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
    """A case's options for run_case."""
    case = dict(case)
    case.pop('needs_no_dropbear', None)
    if case.pop('rename_stage', False):
        case['wrap'] = renamed
        case['patterns'] = [(STAGE_ID.pattern, '<id>')]
    return case


def runnable(case):
    # With a host /usr/sbin/dropbear the case would start it (a direct exec, no fixture command).
    return not (case.get('needs_no_dropbear') and os.path.exists('/usr/sbin/dropbear'))


class DevicesGoldens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('devices',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [(('devices', name), lambda argv=argv, case=case: run_case(self.fixture, argv, **options(case)))
                         for name, argv, case in devices.EXTRA_CASES if runnable(case)])

    def test_goldens_exercise_the_paths_they_name(self):
        results = {name: read_golden('devices', name) for name, _, _ in devices.EXTRA_CASES}
        stage = '/data/transfer-staging/' + 'f' * 32
        self.assertEqual(results['transfer-begin']['stdout']['state'], 'Staging')
        self.assertEqual(touched(results['transfer-begin'])[stage]['mode'], '0o700')
        commit = results['transfer-commit']
        self.assertEqual((commit['exit'], commit['stdout']['state']), (0, 'Committed'))
        self.assertEqual(touched(commit)['/data/music/Album/song.flac']['content'], ['hello'])
        self.assertIn(devices.STAGE + '/payload.part', commit['tree']['removed'])
        self.assertIn('/media/internal/music/Album/song.flac', touched(results['transfer-commit-internal']))
        self.assertEqual(results['transfer-commit-internal-unverified']['stdout']['reason'],
                         'verified_internal_media_unavailable')
        self.assertEqual(results['transfer-commit-exists']['stdout']['type'], 'FileExistsError')
        self.assertEqual(results['transfer-commit-sealed-left']['stdout']['reason'],
                         "[Errno 17] File exists: 'sealed.part'")
        self.assertEqual(results['transfer-commit-wrong-hash']['stdout']['reason'], 'upload_hash_or_size_mismatch')
        # A tampered manifest is refused before anything is sealed or published.
        for name in ('transfer-commit-record-not-dict', 'transfer-commit-manifest-list'):
            refused = results[name]
            self.assertEqual((refused['exit'], refused['stdout']['reason']), (1, 'invalid_transfer_manifest'))
            self.assertNotIn('/data/music/Album/song.flac', touched(refused))
        self.assertEqual(results['ssh-starts-dropbear']['stdout']['type'], 'FileNotFoundError')


if __name__ == '__main__':
    unittest.main()
