"""Golden cases of port group `observation` kept apart from tests/test_platform_differential.py's
(tests/platform_cases/observation.py, EXTRA_CASES): boot-evidence below /data and the records the platform refuses
(REFUSALS). Run by the Rust host build (`Y2_PLATFORM_RUST`) and compared with
tests/platform_goldens/observation/<case>.json (tests/platform_golden.py)."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import SKIP, check_all, read_golden, run_case, rust_binary, touched  # noqa: E402
from platform_cases import observation  # noqa: E402
import test_platform_differential  # noqa: E402


class ObservationGoldens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('observation',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [(('observation', name), lambda argv=argv, options=options: run_case(
            self.fixture, argv, **options)) for name, argv, options in observation.EXTRA_CASES])

    def test_goldens_exercise_the_paths_they_name(self):
        results = {name: read_golden('observation', name) for name, _, _ in observation.EXTRA_CASES}
        for name, reason in observation.REFUSALS.items():
            record = {'schema': 'org.y2linux.error/v1', 'state': 'Failed', 'reason': reason, 'type': 'ValueError'}
            self.assertEqual((results[name]['exit'], results[name]['stdout']), (1, record), name)
        for name in observation.UNREADABLE_POWER:
            self.assertEqual((results[name]['exit'], results[name]['stdout']['power']['low_battery']['state']),
                             (0, 'Unavailable'), name)
        for name in observation.UNREADABLE_SIGNAL:
            self.assertEqual((results[name]['exit'], results[name]['stdout']['wifi']['rssi_dbm']), (0, -55), name)
        for name in observation.UNREADABLE_IP:
            self.assertEqual((results[name]['exit'], results[name]['stdout']['wifi']['ip_addresses']), (0, []), name)
        for name in observation.UNREADABLE_EVIDENCE:
            self.assertEqual((results[name]['exit'], results[name]['stdout']['system']['previous_boot_evidence']),
                             (0, {'state': 'Unavailable', 'reason': 'invalid_previous_boot_evidence'}), name)
        evidence = results['boot-evidence']['stdout']
        self.assertEqual([log['name'] for log in evidence['logs']], ['system.log.0', 'system.log'])
        self.assertEqual(evidence['pmic']['long_press_reset_reason'], 'first_record')
        self.assertIn('/data/system/platform/retained/boot-boot-a.tar', touched(results['boot-evidence']))
        pstore = results['boot-evidence-pstore']['stdout']['retained']
        self.assertTrue(pstore['pstore_mounted'])
        self.assertIn('pstore/' + 'p' * 120, [f['name'] for f in pstore['files']])
        self.assertEqual(results['boot-evidence-many-pstore']['stdout']['retained']['errors'],
                         [{'source': 'pstore', 'error': 'entry_count_exceeds_32'}])
        rotation = results['boot-evidence-rotation']['tree']
        self.assertIn('/data/system/platform/retained/boot-old0.tar', rotation['removed'])
        self.assertNotIn('/data/system/platform/retained/boot-old2.tar', rotation['removed'])
        bounded = results['boot-evidence-bounded']['stdout']['logs']
        self.assertLess(sum(len(log['tail_lines']) for log in bounded), 64)
        self.assertEqual(results['boot-evidence-logs-file']['stdout']['type'], 'NotADirectoryError')
        self.assertEqual(results['boot-evidence-log-directory']['stdout']['type'], 'IsADirectoryError')
        self.assertEqual(results['boot-evidence-odd-boot-id']['stdout']['retained']['error'],
                         'invalid_boot_archive_identity')


if __name__ == '__main__':
    unittest.main()
