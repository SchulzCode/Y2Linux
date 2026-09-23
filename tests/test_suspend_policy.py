"""Exercise the installed suspend helper against captured radio status formats."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'buildroot/board/y2/production-overlay/usr/sbin/y2-suspend'


class SuspendPolicy(unittest.TestCase):
    def run_status(self, powered, functions, qualify=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('run/y2', 'data/network', 'data/bluetooth', 'sys/power',
                         'sys/devices/platform/18070000.connectivity', 'bin'):
                (root / name).mkdir(parents=True)
            for name in ('network', 'bluetooth'):
                (root / 'data' / name / 'enabled').write_text('0\n')
            state = root / 'sys/power/state'
            state.write_text('')
            (root / 'sys/devices/platform/18070000.connectivity/status').write_text(
                f'activated=1 powered={powered} functions={functions} calibrated=1 '
                'chip=6582 hvr=8a01 fvr=8a00 error=0 transport_errors=0 recoveries=0\n')
            # Redirect only the environment; execute the real policy/locking/loop.
            source = HELPER.read_text()
            for prefix in ('/run/', '/data/', '/sys/'):
                source = source.replace(prefix, str(root) + prefix)
            script = root / 'suspend.sh'
            script.write_text(source)
            for command in ('y2-radio', 'sleep', 'sync'):
                mock = root / 'bin' / command
                mock.write_text('#!/bin/sh\nexit 0\n')
                mock.chmod(0o755)
            env = dict(os.environ, PATH=str(root / 'bin') + ':' + os.environ['PATH'])
            result = subprocess.run(['sh', str(script), *(['--owner-qualify'] if qualify else [])], env=env, capture_output=True,
                                    text=True, timeout=5)
            return result, state.read_text()

    def test_unqualified_default_never_enters_suspend(self):
        result, state = self.run_status('0', '0', qualify=False)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(state, '')
        self.assertIn('PHYSICAL_GATE', result.stderr)

    def test_current_and_legacy_all_off_status_allow_suspend(self):
        for functions in ('0x0', '0'):
            with self.subTest(functions=functions):
                result, state = self.run_status('0', functions)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(state, 'mem\n')

    def test_power_or_live_function_prevents_suspend(self):
        for powered, functions in (('1', '0x0'), ('1', '0'), ('0', '0x1'),
                                   ('0', '0x8'), ('0', '0x9'), ('0', '0x01')):
            with self.subTest(powered=powered, functions=functions):
                result, state = self.run_status(powered, functions)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(state, '')
                self.assertIn('Radios did not stop', result.stderr)


if __name__ == '__main__':
    unittest.main()
