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
    def test_automatic_boot_policy_requires_current_receipt_and_real_control_readbacks(self):
        record = {'schema': 'org.y2linux.cpu-idle-policy/v1', 'boot_id': 'boot',
                  'state': 'Enabled', 'budget': '-1', 'disabled': '0'}
        self.assertTrue(DEVICE.boot_policy_verdict(record, 'boot', '-1', '0')['pass'])
        for key, value in (('schema', 'unknown'), ('boot_id', 'previous-boot'),
                           ('state', 'Skipped'), ('budget', '0'), ('disabled', '1')):
            self.assertFalse(DEVICE.boot_policy_verdict({**record, key: value}, 'boot', '-1', '0')['pass'])
        self.assertFalse(DEVICE.boot_policy_verdict(record, 'boot', '0', '0')['pass'])
        self.assertFalse(DEVICE.boot_policy_verdict(record, 'boot', '-1', '1')['pass'])
        self.assertFalse(DEVICE.boot_policy_verdict({}, 'boot', '-1', '0')['pass'])

    def checked_cycle(self):
        counts = ('dormant_attempts', 'dormant_entries', 'dormant_resumes', 'dormant_successes',
                  'dormant_residency_us', 'dormant_restore_failures', 'dormant_failures', 'dormant_aborts',
                  'uart_sleep_attempts', 'uart_sleep_successes', 'uart_sleep_timeouts', 'uart_sleep_restore_failures')
        a = {n: '0' for n in counts}
        b = {**a, 'dormant_attempts': '3', 'dormant_aborts': '2', 'dormant_entries': '1',
             'dormant_resumes': '1', 'dormant_successes': '1', 'dormant_residency_us': '8000',
             'dormant_broken': '0', 'dormant_wake_result': '0', 'dormant_stage': 'budget',
             'dormant_result': '-13', 'dormant_wake': '0x10', 'uart_sleep_attempts': '1',
             'uart_sleep_successes': '1', 'uart_sleep_request': '1', 'uart_sleep_ack': '1',
             'uart_sleep_result': '0', 'uart_power_before': '0x15820', 'uart_power_request': '0x15821',
             'uart_power_after': '0x15820', 'uart_r13_ack': '0x8140000'}
        base = {'boot': 'boot', 'taint': '0',
                'spm': ' '.join(k+'='+v for k, v in a.items()),
                'modules': {'clocks': {'uart_restore_failures': '0'}},
                'metrics': {'states': {'cpu0/cpuidle/state2': {'usage': '0', 'time': '0'}}}}
        after = {**base, 'spm': ' '.join(k+'='+v for k, v in b.items()),
                 'modules': {'clocks': {'uart_restore_failures': '0', 'uart1_gate_before': '0',
                    'uart1_gate_after': '0', 'uart_sleep_handoff': 'phase=restored uart1_sleep_before=0 uart1_sleep_now=0'},
                    'local_timer': {'context_failures': '0', 'context_saves': '1', 'context_restores': '1',
                                    'handoff_remaining_ns': '6000000'}},
                 'cirq': {'restore_failures': '0', 'clone_failures': '0', 'entries': '1', 'flushes': '1'},
                 'journal': {'state': 'valid=1 stage=DORMANT_COMPLETE error=0 reset_resume_entry=0x59325253',
                    'devices': ' '.join('stage='+n for n in ('BACKSTOP_STARTED', 'UART_REQUEST', 'UART_ACK',
                        'DORMANT_CONTEXT', 'DORMANT_FINISH', 'DORMANT_RETURN', 'DORMANT_COMPLETE'))},
                 'metrics': {'states': {'cpu0/cpuidle/state2': {'usage': '1', 'time': '9000'}}}}
        return base, after

    def test_checked_cycle_requires_uart_and_retained_real_reset_return(self):
        before, after = self.checked_cycle()
        self.assertTrue(DEVICE.cycle_verdict(before, after, 'boot')['pass'])
        for section, key, value in (('cirq', 'restore_failures', '1'), ('cirq', 'flushes', '0'),
                                   ('journal', 'state', 'valid=1 stage=DORMANT_COMPLETE error=0 reset_resume_entry=0'),
                                   ('journal', 'devices', '')):
            bad = {**after, section: {**after[section], key: value}}
            self.assertFalse(DEVICE.cycle_verdict(before, bad, 'boot')['pass'])
        for module, key, value in (('clocks', 'uart1_gate_after', '1'),
                ('clocks', 'uart_sleep_handoff', 'phase=restored uart1_sleep_before=0 uart1_sleep_now=1'),
                ('local_timer', 'context_restores', '0'), ('local_timer', 'handoff_remaining_ns', '1999999')):
            bad = {**after, 'modules': {**after['modules'], module: {**after['modules'][module], key: value}}}
            self.assertFalse(DEVICE.cycle_verdict(before, bad, 'boot')['pass'])
        self.assertFalse(DEVICE.cycle_verdict(before, {**after, 'boot': 'reset'}, 'boot')['pass'])

    def test_twenty_cycle_continuation_rechecks_first_entry_and_original_foundation_hash(self):
        before, after = self.checked_cycle()
        actual = {'boot': 'boot', 'versions': {'source': 'exact'}}
        phases = ('C1', 'hotplug', 'DVFS', 'C2_parking', 'storage_after_C2',
                  'storage_after_C3', 'screen_wake', 'DVFS_after_C3', 'playback_workload_wake')
        counters = ('dormant_entries', 'dormant_resumes', 'dormant_successes', 'uart_sleep_attempts',
                    'dormant_failures', 'dormant_restore_failures', 'dormant_broken')
        original = {'start_boot': 'boot', 'end_boot': 'boot', 'source': actual['versions'], 'done': True,
                    'phases': {n: {'pass': True} for n in phases},
                    'final': {'spm': ' '.join(n+'=0' for n in counters), 'taint': '0'},
                    'USB_WiFi_integrity': {'pass': True}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'foundation.json'
            raw = json.dumps(original).encode(); path.write_bytes(raw)
            first = {**original, 'c3_cycles': [{'cycle': 0, 'before': before, 'after': after}],
                     'foundation_receipt': {'path': str(path), 'sha256': HOST.hashlib.sha256(raw).hexdigest()}}
            self.assertEqual(HOST.repeat_errors(first, actual), [])
            self.assertTrue(HOST.repeat_errors({**first, 'c3_cycles': []}, actual))
            self.assertTrue(HOST.repeat_errors({**first, 'c3_cycles': first['c3_cycles']*2}, actual))
            bad = {**after, 'boot': 'new'}
            self.assertTrue(HOST.repeat_errors({**first, 'c3_cycles': [{'cycle': 0, 'before': before, 'after': bad}]}, actual))
            path.write_bytes(raw+b' ')
            self.assertIn('original_foundation', HOST.repeat_errors(first, actual))

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
        a = {k: '0' for k in ('dormant_attempts', 'dormant_entries', 'dormant_resumes', 'dormant_successes',
                             'dormant_residency_us', 'dormant_restore_failures', 'dormant_failures', 'dormant_aborts')}
        b = {**a, 'dormant_attempts': '1', 'dormant_entries': '1', 'dormant_resumes': '1', 'dormant_successes': '1',
             'dormant_residency_us': '4000', 'dormant_broken': '0', 'dormant_result': '-13', 'dormant_wake_result': '0'}
        self.assertTrue(DEVICE.dormant_verdict(a, b)['pass'])
        safe = {**b, 'dormant_attempts': '3', 'dormant_aborts': '2', 'dormant_stage': 'budget',
                'dormant_result': '-13'}
        self.assertTrue(DEVICE.dormant_verdict(a, safe)['pass'])
        self.assertTrue(DEVICE.dormant_verdict(a, {**safe, 'dormant_stage': 'resumed', 'dormant_result': '0'})['pass'])
        self.assertEqual(DEVICE.dormant_verdict(a, safe)['safe_admission_refusals'], 2)
        self.assertFalse(DEVICE.dormant_verdict(a, {**safe, 'dormant_stage': 'UART_BUSY'})['pass'])
        self.assertFalse(DEVICE.dormant_verdict(a, {**safe, 'dormant_aborts': '1'})['pass'])
        for k, value in (('dormant_attempts', '2'), ('dormant_successes', '0'), ('dormant_residency_us', '0'),
                         ('dormant_restore_failures', '1'), ('dormant_broken', '1'),
                         ('dormant_entries', '2'), ('dormant_wake_result', '-5')):
            self.assertFalse(DEVICE.dormant_verdict(a, {**b, k: value})['pass'], k)
        with self.assertRaises(RuntimeError):
            DEVICE.delta({}, {}, 'dormant_successes')

    def test_rgu_readback_is_restored_as_its_integer_control(self):
        for seconds in (0, 10, 30):
            text = 'provider=1 armed_s=%d running=0 paused=0 staged=0 seconds=0 error=0' % seconds
            self.assertEqual(DEVICE.number(DEVICE.fields(text).get('armed_s')), seconds)

    def test_uart_requires_new_real_ack_and_request_cleanup(self):
        a = {k: '0' for k in ('uart_sleep_attempts', 'uart_sleep_successes',
                             'uart_sleep_timeouts', 'uart_sleep_restore_failures')}
        b = {**a, 'uart_sleep_attempts': '1', 'uart_sleep_successes': '1',
             'uart_sleep_request': '1', 'uart_sleep_ack': '1', 'uart_sleep_result': '0',
             'uart_power_before': '0x15820', 'uart_power_request': '0x15821',
             'uart_power_after': '0x15820', 'uart_r13_ack': '0x100000'}
        self.assertTrue(DEVICE.uart_sleep_verdict(a, b)['pass'])
        for key, value in (('uart_sleep_attempts', '0'), ('uart_sleep_successes', '0'),
                           ('uart_sleep_timeouts', '1'), ('uart_sleep_restore_failures', '1'),
                           ('uart_sleep_ack', '0'), ('uart_sleep_request', '0'),
                           ('uart_power_before', '0x15821'), ('uart_power_request', '0x15820'),
                           ('uart_power_after', '0x15821'), ('uart_r13_ack', '0'),
                           ('uart_sleep_result', '-16'), ('uart_power_after', None)):
            self.assertFalse(DEVICE.uart_sleep_verdict(a, {**b, key: value})['pass'], key)

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
        events = []
        q.save = lambda: events.append('save')
        q.wait_for_c3_admission = lambda: events.append('quiet') or {'pass': True, 'last': {'preflight': 'unmet='}}
        writes = []
        state = {'dormant_attempts': '0', 'dormant_entries': '0', 'dormant_resumes': '0', 'dormant_successes': '0',
                 'dormant_residency_us': '0', 'dormant_restore_failures': '0', 'dormant_failures': '0',
                 'dormant_aborts': '0', 'dormant_broken': '0', 'dormant_result': '-13',
                 'dormant_wake_result': '-16', 'dormant_wake': '0'}
        state.update({k: '0' for k in ('uart_sleep_attempts', 'uart_sleep_successes',
                                     'uart_sleep_timeouts', 'uart_sleep_restore_failures')})
        snaps = {'spm': ' '.join(k+'='+v for k, v in state.items()), 'taint': '0', 'boot': 'boot',
                 'modules': {'clocks': {'uart1_gate_before': '0', 'uart1_gate_after': '0',
                                        'uart_restore_failures': '0', 'uart_sleep_handoff': 'phase=never_prepared'},
                             'local_timer': {'context_failures': '0', 'context_saves': '0',
                                             'context_restores': '0', 'handoff_remaining_ns': '2000000'}},
                 'cirq': {'restore_failures': '0', 'clone_failures': '0', 'entries': '0', 'flushes': '0'},
                 'journal': {'state': 'valid=0', 'devices': ''},
                 'metrics': {'states': {'cpu0/cpuidle/state2': {'usage': '0', 'time': '0'}}}}
        role = types.SimpleNamespace()
        with patch.object(DEVICE.P, 'glob', return_value=iter([role])), \
             patch.object(DEVICE, 'read', return_value='device'), \
             patch.object(DEVICE, 'write', side_effect=lambda p, v: (writes.append((p, v)),
                 events.append('enable') if p == DEVICE.C3 and v == 0 else None)), \
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
        # The first attempted entry is a 25ms wake, never the 1ms boundary probe.
        self.assertNotIn('near_deadline_fallback', q.result)
        self.assertEqual(events[:5], ['quiet', 'save', 'quiet', 'enable', 'save'])

    def test_c3_admission_waits_after_fsync_for_real_mmc_gates_without_writes(self):
        q = object.__new__(DEVICE.Qualification)
        q.result = {'start_boot': 'boot'}
        q.save = lambda: self.fail('no durable write in final quiet window')
        clock = [0]
        prerequisites = ('system_running', 'boot_policy', 'spm', 'local_events', 'cirq',
                         'topology', 'frequency', 'screen_off', 'workload', 'runtime_budget',
                         'qualification_backstop', 'linux_context', 'timer_context',
                         'usb_restore', 'broken', 'clocks', 'display_clocks', 'bus')
        ready = ('online=1 power_status=0x314c/0x314c secondary_power=0 domain_blockers=0 '
                 'peri_blockers=0 infra_blockers=0\n' +
                 '\n'.join('prerequisite_'+n+'=1' for n in prerequisites)+'\nunmet=\n')
        mmc_path = Path('/sys/bus/platform/devices/11230000.mmc/y2_runtime_pm')
        def sleep(seconds):
            clock[0] += seconds
        def read(path):
            if path == DEVICE.C3:
                return '1'
            if path == DEVICE.CPU/'online':
                return '0'
            return 'gated=%d error=0 last_mismatch=0' % (clock[0] >= 1)
        with patch.object(DEVICE.time, 'monotonic', side_effect=lambda: clock[0]), \
             patch.object(DEVICE.time, 'sleep', side_effect=sleep), \
             patch.object(DEVICE, 'boot', return_value='boot'), \
             patch.object(DEVICE, 'read', side_effect=read), \
             patch.object(DEVICE, 'params', return_value={'parked_mask': '14'}), \
             patch.object(DEVICE, 'spm', return_value=ready), \
             patch.object(DEVICE.P, 'glob', return_value=[mmc_path]), \
             patch.object(DEVICE, 'write', side_effect=AssertionError('must not arm or force a gate')):
            result = q.wait_for_c3_admission(timeout=5)
        self.assertTrue(result['pass'])
        self.assertEqual(clock[0], 3)
        self.assertEqual(result['quiet_s'], 2)

    def test_c3_continuation_requires_same_boot_all_regressions_and_no_uart_attempt(self):
        # Build a small receipt without relying on local hardware evidence files.
        phases = ('C1', 'hotplug', 'DVFS', 'C2_parking', 'storage_after_C2',
                  'storage_after_C3', 'screen_wake', 'DVFS_after_C3', 'playback_workload_wake')
        counters = ('dormant_entries', 'dormant_resumes', 'dormant_successes', 'uart_sleep_attempts',
                    'dormant_failures', 'dormant_restore_failures', 'dormant_broken')
        actual = {'boot': 'boot', 'versions': {'build_id': 'exact-source'}}
        prior = {'start_boot': 'boot', 'end_boot': 'boot', 'source': actual['versions'],
                 'done': True, 'phases': {n: {'pass': True} for n in phases},
                 'final': {'taint': '0', 'spm': ' '.join(n+'=0' for n in counters)},
                 'USB_WiFi_integrity': {'pass': True}}
        self.assertEqual(HOST.foundation_errors(prior, actual), [])
        for key, value in (('start_boot', 'old'), ('source', {}), ('done', False),
                           ('cleanup_errors', ['bad']), ('filesystem_irq_errors', ['bad'])):
            self.assertTrue(HOST.foundation_errors({**prior, key: value}, actual), key)
        for n in phases:
            self.assertIn(n, HOST.foundation_errors({**prior, 'phases':
                {**prior['phases'], n: {'pass': False}}}, actual))
        for n in counters:
            final = {**prior['final'], 'spm': prior['final']['spm'].replace(n+'=0', n+'=1')}
            self.assertIn('never_entered_'+n, HOST.foundation_errors({**prior, 'final': final}, actual))

    def test_c3_admission_never_ignores_a_legitimate_clock_blocker(self):
        q = object.__new__(DEVICE.Qualification)
        q.result = {'start_boot': 'boot'}
        q.save = lambda: self.fail('no writes while checking a blocker')
        clock = [0]
        def sleep(seconds):
            clock[0] += seconds
        with patch.object(DEVICE.time, 'monotonic', side_effect=lambda: clock[0]), \
             patch.object(DEVICE.time, 'sleep', side_effect=sleep), \
             patch.object(DEVICE, 'boot', return_value='boot'), \
             patch.object(DEVICE, 'read', return_value='1'), \
             patch.object(DEVICE, 'params', return_value={'parked_mask': '14'}), \
             patch.object(DEVICE, 'spm', return_value='prerequisite_clocks=0 peri_blockers=0x1000\nunmet=clocks,'), \
             patch.object(DEVICE.P, 'glob', return_value=[]), \
             patch.object(DEVICE, 'write', side_effect=AssertionError('never enable C3 with a blocker')):
            with self.assertRaisesRegex(RuntimeError, 'admission did not settle'):
                q.wait_for_c3_admission(timeout=1)
        self.assertEqual(clock[0], 1)

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
