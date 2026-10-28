"""Differential cases of port group `observation` that the shared hermeticity audit of
tests/test_platform_differential.py misreads (tests/platform_cases/observation.py, EXTRA_CASES): boot-evidence's
descriptor-relative opens below /data and the tracebacks of inputs the Python platform crashes on. Python against
Python, an audit that allows exactly those names, and against the Rust host build when `Y2_PLATFORM_RUST` is set."""
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_differential import compare, host_access, run_case, python_side, rust_binary, rust_side, RUST_ENV  # noqa: E402
from platform_cases import observation  # noqa: E402
import test_platform_differential  # noqa: E402


class ObservationDifferential(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture')

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_python_against_python_is_identical(self):
        for name, argv, options in observation.EXTRA_CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, **options), [])

    def test_cases_stay_inside_the_fixture_root(self):
        allowed = {os.path.join(os.getcwd(), name) for name in observation.AUDIT_NAMES}
        for name, argv, options in observation.EXTRA_CASES:
            with self.subTest(name):
                found = host_access(self.fixture, argv, env=options.get('env'), files=options.get('files'))
                self.assertEqual([a for a in found if not (a['event'] == 'open' and a['target'] in allowed)], [])

    def test_cases_exercise_the_paths_they_name(self):
        results = {name: run_case(python_side, self.fixture, argv, **options)
                   for name, argv, options in observation.EXTRA_CASES}
        for name in observation.CRASHES:
            self.assertEqual((results[name]['exit'], results[name]['stdout']), (1, []), name)
        evidence = results['boot-evidence']['stdout']
        self.assertEqual([log['name'] for log in evidence['logs']], ['system.log.0', 'system.log'])
        self.assertEqual(evidence['pmic']['long_press_reset_reason'], 'first_record')
        self.assertIn('/data/system/platform/retained/boot-boot-a.tar', results['boot-evidence']['tree'])
        pstore = results['boot-evidence-pstore']['stdout']['retained']
        self.assertTrue(pstore['pstore_mounted'])
        self.assertIn('pstore/' + 'p' * 120, [f['name'] for f in pstore['files']])
        self.assertEqual(results['boot-evidence-many-pstore']['stdout']['retained']['errors'],
                         [{'source': 'pstore', 'error': 'entry_count_exceeds_32'}])
        rotated = [p for p in results['boot-evidence-rotation']['tree'] if p.endswith('.tar')]
        self.assertNotIn('/data/system/platform/retained/boot-old0.tar', rotated)
        self.assertIn('/data/system/platform/retained/boot-old2.tar', rotated)
        bounded = results['boot-evidence-bounded']['stdout']['logs']
        self.assertLess(sum(len(log['tail_lines']) for log in bounded), 64)
        self.assertEqual(results['boot-evidence-logs-file']['stdout']['type'], 'NotADirectoryError')
        self.assertEqual(results['boot-evidence-log-directory']['stdout']['type'], 'IsADirectoryError')
        self.assertEqual(results['boot-evidence-odd-boot-id']['stdout']['retained']['error'],
                         'invalid_boot_archive_identity')

    @unittest.skipUnless(rust_binary(), RUST_ENV + ' (host build of the Rust y2-platform) not given')
    def test_rust_matches_python(self):
        rust = rust_side(rust_binary())
        for name, argv, options in observation.EXTRA_CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, right=rust, **options), [])


if __name__ == '__main__':
    unittest.main()
