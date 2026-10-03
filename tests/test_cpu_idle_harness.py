"""Offline fault tests for the owner-run hardware harness."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT/'tools/development'/file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HOST = load('cpu_idle_host', 'qualify-cpu-idle-completion.py')
DEVICE = load('cpu_idle_device', 'cpu_idle_completion_device.py')


class Harness(unittest.TestCase):
    def test_default_plan_never_contacts_hardware(self):
        with patch.object(HOST.subprocess, 'run', side_effect=AssertionError('SSH')):
            with patch('sys.argv', ['qualification']):
                self.assertEqual(HOST.main(), 0)

    def test_every_installed_identity_must_match(self):
        manifest = {k: 'expected-'+k for k in ('build_git_commit', 'reborn_source_commit',
                    'kernel_version', 'rootfs_version', 'release_version', 'build_id',
                    'platform_api_version', 'data_schema_version')}
        actual = {'versions': manifest.copy(), 'boot': 'physical-boot',
                  'uname_release': manifest['kernel_version'], 'reborn_build': manifest['reborn_source_commit']}
        self.assertEqual(HOST.identity_errors(manifest, actual), [])
        for key in manifest:
            bad = {**actual, 'versions': {**manifest, key: 'stale'}}
            self.assertIn(key, HOST.identity_errors(manifest, bad))
        actual['reborn_build'] = 'old-reborn'
        self.assertIn('running_reborn_build', HOST.identity_errors(manifest, actual))

    def test_disconnect_fails_over_observation_but_never_retries_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            args = types.SimpleNamespace(output=Path(directory)/'evidence', host='usb', wifi_host='wifi')
            runner = HOST.Run(args)
            calls = []
            def ssh(host, command, body=None, timeout=30):
                calls.append((host, command))
                if command == 'true' and host == 'wifi':
                    return 0, b'', b''
                return 255, b'', b'lost transport'
            runner.ssh = ssh
            self.assertEqual(runner.launch_once('MUTATE', b'payload')[0], 255)
            self.assertEqual(calls, [('usb', 'true'), ('wifi', 'true'), ('wifi', 'MUTATE')])

    def test_real_reset_return_requires_residency_and_all_restores(self):
        a = {k: '0' for k in ('dormant_entries', 'dormant_resumes', 'dormant_successes',
                             'dormant_residency_us', 'dormant_restore_failures', 'dormant_failures')}
        b = {**a, 'dormant_entries': '1', 'dormant_resumes': '1', 'dormant_successes': '1',
             'dormant_residency_us': '4000', 'dormant_broken': '0', 'dormant_result': '-13', 'dormant_wake_result': '0'}
        self.assertTrue(DEVICE.dormant_verdict(a, b)['pass'])
        for k, value in (('dormant_successes', '0'), ('dormant_residency_us', '0'),
                         ('dormant_restore_failures', '1'), ('dormant_broken', '1'),
                         ('dormant_entries', '2'), ('dormant_wake_result', '-5')):
            self.assertFalse(DEVICE.dormant_verdict(a, {**b, k: value})['pass'], k)
        with self.assertRaises(RuntimeError):
            DEVICE.delta({}, {}, 'dormant_successes')

    def test_rgu_readback_is_restored_as_its_integer_control(self):
        for seconds in (0, 10, 30):
            text = 'provider=1 armed_s=%d running=0 paused=0 staged=0 seconds=0 error=0' % seconds
            self.assertEqual(DEVICE.number(DEVICE.fields(text).get('armed_s')), seconds)

    def test_hotplug_checks_both_exact_descending_mt6582_power_copies(self):
        q = object.__new__(DEVICE.Qualification)
        # Receipt11: actual CPU3/2/1 off and CPU1/2/3 on power values.
        values = (0x3d4e, 0x394e, 0x314e, 0x394e, 0x3d4e, 0x3f4e)
        writes = []
        with patch.object(DEVICE, 'write', side_effect=lambda p, v: writes.append((p, v))), \
             patch.object(DEVICE.time, 'sleep', return_value=None), \
             patch.object(DEVICE, 'read', return_value='0-3'), \
             patch.object(DEVICE, 'spm', side_effect=['power=%#x/%#x' % (v, v) for v in values]*3):
            result = q.hotplug()
        self.assertTrue(result['pass'])
        self.assertEqual(len(result['transitions']), 18)
        self.assertEqual([(p.parent.name, v) for p, v in writes[:6]],
                         [('cpu3', 0), ('cpu2', 0), ('cpu1', 0), ('cpu1', 1), ('cpu2', 1), ('cpu3', 1)])
        with patch.object(DEVICE, 'write'), patch.object(DEVICE.time, 'sleep'), \
             patch.object(DEVICE, 'spm', return_value='power=0x3d4e/0x3f4e'):
            with self.assertRaisesRegex(RuntimeError, 'power copies disagree'):
                q.hotplug()

    def test_first_failed_cycle_disables_c3_and_stops_repetition(self):
        # Exercise the actual trial loop, not a duplicate state machine.
        q = object.__new__(DEVICE.Qualification)
        q.result = {'start_boot': 'boot'}
        q.config = {}
        q.cmd = lambda *a: ''
        q.save = lambda: None
        q.wait_for_c3_topology = lambda: {'pass': True}
        writes = []
        state = {'dormant_entries': '0', 'dormant_resumes': '0', 'dormant_successes': '0',
                 'dormant_residency_us': '0', 'dormant_restore_failures': '0', 'dormant_failures': '0',
                 'dormant_broken': '0', 'dormant_result': '-13', 'dormant_wake_result': '-16'}
        snaps = {'spm': ' '.join(k+'='+v for k, v in state.items()), 'taint': '0',
                 'metrics': {'states': {'cpu0/cpuidle/state2': {'usage': '0', 'time': '0'}}}}
        role = types.SimpleNamespace()
        with patch.object(DEVICE.P, 'glob', return_value=iter([role])), \
             patch.object(DEVICE, 'read', return_value='device'), \
             patch.object(DEVICE, 'write', side_effect=lambda p, v: writes.append((p, v))), \
             patch.object(DEVICE, 'spm', side_effect=lambda n='state': 'unmet=' if n == 'dormant_preflight' else snaps['spm']), \
             patch.object(DEVICE, 'snapshot', return_value=snaps), \
             patch.object(DEVICE.time, 'sleep', return_value=None), \
             patch.object(DEVICE, 'boot', return_value='boot'):
            with self.assertRaisesRegex(RuntimeError, 'first failed bounded cycle'):
                q.c3_cycles()
        self.assertEqual(len(q.result['c3_cycles']), 1)
        self.assertIn((DEVICE.C3, 1), writes)
        self.assertIn((DEVICE.BUDGET, 0), writes)
        self.assertEqual(writes[-1], (role, 'device'))

    def test_c3_waits_for_owned_parking_and_both_physical_power_copies(self):
        q = object.__new__(DEVICE.Qualification)
        q.result = {'start_boot': 'boot'}
        saves = []
        q.save = lambda: saves.append(clock[0])
        clock = [0]
        rows = [('0-3', '0', '0x3f4c/0x3f4c'),
                ('0', '0', '0x314c/0x314c'),
                ('0', '14', '0x314c/0x334c'),
                ('0', '14', '0x314c/0x314c')]
        def row():
            return rows[min(clock[0]//10, len(rows)-1)]
        def sleep(seconds):
            clock[0] += seconds
        with patch.object(DEVICE.time, 'monotonic', side_effect=lambda: clock[0]), \
             patch.object(DEVICE.time, 'sleep', side_effect=sleep), \
             patch.object(DEVICE, 'boot', return_value='boot'), \
             patch.object(DEVICE, 'read', side_effect=lambda p: row()[0]), \
             patch.object(DEVICE, 'params', side_effect=lambda m: {'parked_mask': row()[1]}), \
             patch.object(DEVICE, 'spm', side_effect=lambda: 'power='+row()[2]), \
             patch.object(DEVICE, 'write', side_effect=AssertionError('must not force topology')):
            result = q.wait_for_c3_topology(timeout=100)
        self.assertTrue(result['pass'])
        self.assertEqual(clock[0], 50)
        self.assertEqual(len(result['trace']), 6)
        self.assertEqual(saves, [0])

    def test_c3_topology_timeout_and_reset_never_arm_entry(self):
        q = object.__new__(DEVICE.Qualification)
        q.result = {'start_boot': 'boot'}
        q.save = lambda: None
        clock = [0]
        def sleep(seconds):
            clock[0] += seconds
        with patch.object(DEVICE.time, 'monotonic', side_effect=lambda: clock[0]), \
             patch.object(DEVICE.time, 'sleep', side_effect=sleep), \
             patch.object(DEVICE, 'boot', return_value='boot'), \
             patch.object(DEVICE, 'read', return_value='0'), \
             patch.object(DEVICE, 'params', return_value={'parked_mask': '14'}), \
             patch.object(DEVICE, 'spm', return_value='power=unavailable'), \
             patch.object(DEVICE, 'write', side_effect=AssertionError('must not arm entry')):
            with self.assertRaisesRegex(RuntimeError, 'did not settle'):
                q.wait_for_c3_topology(timeout=10)
        with patch.object(DEVICE, 'boot', return_value='changed-boot'), \
             patch.object(DEVICE, 'write', side_effect=AssertionError('must not arm entry')):
            with self.assertRaisesRegex(RuntimeError, 'boot changed'):
                q.wait_for_c3_topology()


if __name__ == '__main__':
    unittest.main()
