"""Exercise the installed suspend helper against captured radio status formats."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'buildroot/board/y2/production-overlay/usr/sbin/y2-suspend'


class SuspendPolicy(unittest.TestCase):
    def run_status(self, powered, functions, qualify=True, enabled=False,
                   fail_radio=None, fail_restore_record=False, fail_prepare=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('run/y2', 'data/network', 'data/bluetooth', 'sys/power',
                         'sys/devices/platform/18070000.connectivity', 'bin'):
                (root / name).mkdir(parents=True)
            for name in ('network', 'bluetooth'):
                (root / 'data' / name / 'enabled').write_text('1\n' if enabled else '0\n')
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
            for command in ('y2-radio', 'sleep', 'sync', 'y2-suspend-record', 'y2-platform'):
                mock = root / 'bin' / command
                mock.write_text('#!/bin/sh\nexit 0\n')
                mock.chmod(0o755)
            if fail_prepare:
                (root / 'bin/y2-platform').write_text('#!/bin/sh\nexit 1\n')
            calls = root / 'radio-calls'
            (root / 'bin/y2-radio').write_text(
                '#!/bin/sh\n'
                f'echo "$*" >> "{calls}"\n'
                '[ "$1:$2" != "${FAIL_RADIO}:on-runtime" ]\n')
            env = dict(os.environ, PATH=str(root / 'bin') + ':' + os.environ['PATH'])
            env['FAIL_RADIO'] = fail_radio or ''
            if fail_restore_record:
                (root / 'bin/y2-suspend-record').write_text(
                    '#!/bin/sh\n[ "$1" != restoring_radios ]\n')
            result = subprocess.run(['sh', str(script), *(['--owner-qualify'] if qualify else [])], env=env, capture_output=True,
                                    text=True, timeout=5)
            if enabled and qualify:
                self.assertIn('wifi on-runtime', calls.read_text())
                self.assertIn('bluetooth on-runtime', calls.read_text())
            if fail_prepare:
                self.assertNotIn('off-runtime', calls.read_text() if calls.exists() else '')
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

    def test_failed_reborn_preparation_never_enters_or_quiesces_radios(self):
        result, state = self.run_status('0', '0x0', enabled=True, fail_prepare=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(state, '')

    def test_restore_failure_is_not_a_passing_suspend(self):
        for failed in ('wifi', 'bluetooth'):
            with self.subTest(failed=failed):
                result, state = self.run_status('0', '0x0', enabled=True,
                                                fail_radio=failed)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(state, 'mem\n')
                self.assertIn('Radio restoration failed', result.stderr)

    def test_restore_success_preserves_original_failure(self):
        result, state = self.run_status('1', '0x0', enabled=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state, '')
        self.assertIn('Radios did not stop', result.stderr)

    def test_failed_resume_receipt_still_restores_both_radios(self):
        result, state = self.run_status('0', '0x0', enabled=True,
                                       fail_restore_record=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(state, 'mem\n')

    def test_restore_success_allows_a_successful_stage(self):
        result, state = self.run_status('0', '0x0', enabled=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(state, 'mem\n')

    def test_wifi_switch_never_changes_the_wlan_default(self):
        # F37 (72f7e3ee, a19646dd): `rfkill block wlan` at each sleep also set
        # the type's default; with Wi-Fi off the wake never cleared it, and the
        # phy rfkill of the next switch-on started blocked. Every wlan device
        # is switched by itself; Bluetooth's rfkill is not touched.
        radio = HELPER.with_name('y2-radio')
        commands = [line for line in radio.read_text().splitlines() if not line.lstrip().startswith('#')]
        self.assertFalse([line for line in commands if 'rfkill block' in line or 'rfkill unblock' in line])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            devices = {'rfkill0': 'wlan', 'rfkill1': 'bluetooth', 'rfkill3': 'wlan'}
            for name, kind in devices.items():
                (root / 'sys/class/rfkill' / name).mkdir(parents=True)
                (root / 'sys/class/rfkill' / name / 'type').write_text(kind + '\n')
                (root / 'sys/class/rfkill' / name / 'soft').write_text('0\n')
            script = root / 'y2-radio'
            script.write_text(radio.read_text().replace('/sys/', str(root) + '/sys/'))
            soft = lambda: {n: (root / 'sys/class/rfkill' / n / 'soft').read_text().strip() for n in devices}
            result = subprocess.run(['sh', str(script), 'wifi', 'off-runtime'], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(soft(), {'rfkill0': '1', 'rfkill1': '0', 'rfkill3': '1'})
            # The card's phy rfkill goes away once y2-wifi is blocked.
            for name in ('soft', 'type'):
                (root / 'sys/class/rfkill/rfkill3' / name).unlink()
            (root / 'sys/class/rfkill/rfkill3').rmdir()
            del devices['rfkill3']
            result = subprocess.run(['sh', str(script), 'wifi', 'on-runtime'], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(soft(), {'rfkill0': '0', 'rfkill1': '0'})


if __name__ == '__main__':
    unittest.main()
