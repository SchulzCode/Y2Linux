"""Exercise real boot-policy writes against isolated sysfs fixtures and faults."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform import cpu_idle as policy


class BootPolicy(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ctx = Context(self.tmp.name)
        for name, value in (
                ('/proc/sys/kernel/random/boot_id', 'qualified-boot'),
                ('/proc/sys/kernel/osrelease', 'qualified-kernel'),
                ('/proc/sys/kernel/tainted', '0'),
                ('/etc/y2linux/versions.json', json.dumps({'kernel_version': 'qualified-kernel'})),
                ('/sys/devices/system/cpu/cpuidle/current_driver', 'mt6582-idle'),
                (policy.STATE + '/name', 'DORMANT'),
                (policy.STATE + '/disable', '1'), (policy.BUDGET, '0'),
                (policy.PREFLIGHT, '\n'.join('prerequisite_' + n + '=1' for n in policy.FOUNDATIONS)
                 + '\nprerequisite_topology=0\nprerequisite_frequency=0\nprerequisite_clocks=0\nunmet=topology,frequency,clocks,'),
                ('/sys/devices/system/cpu/cpu0/cpuidle/state0/disable', '0'),
                ('/sys/devices/system/cpu/cpu0/cpuidle/state1/disable', '0'),
                ('/sys/devices/system/cpu/cpu1/cpuidle/state2/disable', '1')):
            self.put(name, value)

    def put(self, name, value):
        p = self.ctx.path(name)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(value + '\n')

    def assert_quarantined(self):
        self.assertEqual(self.ctx.read(policy.BUDGET), '0')
        self.assertEqual(self.ctx.read(policy.STATE + '/disable'), '1')

    def test_enables_only_cpu0_and_leaves_transient_entry_guards_to_kernel(self):
        calls = []
        original = policy._write
        def write(ctx, name, value):
            calls.append((name, value))
            original(ctx, name, value)
        with patch.object(policy, '_write', write):
            result = policy.apply(self.ctx)
        self.assertEqual(result['state'], 'Enabled')
        self.assertEqual(calls, [(policy.BUDGET, '-1'), (policy.STATE + '/disable', '0')])
        for state in ('state0', 'state1'):
            self.assertEqual(self.ctx.read('/sys/devices/system/cpu/cpu0/cpuidle/' + state + '/disable'), '0')
        self.assertEqual(self.ctx.read('/sys/devices/system/cpu/cpu1/cpuidle/state2/disable'), '1')
        self.assertEqual(self.ctx.json(policy.RECORD)['boot_id'], 'qualified-boot')

    def test_each_missing_or_failed_foundation_keeps_safe_fallback(self):
        for name in policy.FOUNDATIONS:
            with self.subTest(name=name):
                self.ctx.path(policy.LOCK).unlink(missing_ok=True)
                lines = ['prerequisite_' + n + '=' + ('0' if n == name else '1')
                         for n in policy.FOUNDATIONS]
                self.put(policy.PREFLIGHT, '\n'.join(lines))
                result = policy.apply(self.ctx)
                self.assertIn('foundation:' + name, result['reasons'])
                self.assert_quarantined()

    def test_explicit_deep_idle_off_missing_state_and_unknown_identity_stay_off(self):
        for name, value in ((policy.STATE + '/name', 'unavailable'),
                            ('/proc/sys/kernel/tainted', '1'),
                            ('/proc/sys/kernel/osrelease', 'wrong-kernel'),
                            ('/sys/devices/system/cpu/cpuidle/current_driver', 'other')):
            with self.subTest(name=name):
                old = self.ctx.read(name)
                self.put(name, value)
                self.ctx.path(policy.LOCK).unlink(missing_ok=True)
                self.assertEqual(policy.apply(self.ctx)['state'], 'Skipped')
                self.assert_quarantined()
                self.put(name, old)

    def test_later_manual_disable_is_not_reenabled_by_repeated_start(self):
        self.assertEqual(policy.apply(self.ctx)['state'], 'Enabled')
        self.put(policy.BUDGET, '0')
        self.put(policy.STATE + '/disable', '1')
        self.assertIn('already_applied_this_boot', policy.apply(self.ctx)['reasons'])
        self.assert_quarantined()

    def test_existing_bounded_owner_trial_is_not_replaced(self):
        self.put(policy.BUDGET, '1')
        self.assertIn('existing_owner_runtime_policy', policy.apply(self.ctx)['reasons'])
        self.assertEqual(self.ctx.read(policy.BUDGET), '1')
        self.assertEqual(self.ctx.read(policy.STATE + '/disable'), '1')

    def test_readback_mismatch_or_enable_write_failure_rolls_back(self):
        original = policy._write
        for fault in ('budget_readback', 'state_write'):
            with self.subTest(fault=fault):
                self.ctx.path(policy.LOCK).unlink(missing_ok=True)
                def write(ctx, name, value):
                    if fault == 'budget_readback' and name == policy.BUDGET and value == '-1':
                        return
                    if fault == 'state_write' and name == policy.STATE + '/disable' and value == '0':
                        raise OSError('injected state write failure')
                    original(ctx, name, value)
                with patch.object(policy, '_write', write):
                    result = policy.apply(self.ctx)
                self.assertEqual(result['state'], 'Failed')
                self.assertEqual(result['rollback_errors'], [])
                self.assert_quarantined()

    def test_stop_quarantines_and_cannot_be_undone_by_start(self):
        self.put(policy.BUDGET, '-1')
        self.put(policy.STATE + '/disable', '0')
        self.assertEqual(policy.apply(self.ctx, 'stop')['state'], 'Disabled')
        self.assert_quarantined()
        self.assertIn('already_applied_this_boot', policy.apply(self.ctx)['reasons'])

    def test_unavailable_preflight_is_not_interpreted_as_ready(self):
        self.ctx.path(policy.PREFLIGHT).unlink()
        self.assertEqual(len(policy.apply(self.ctx)['reasons']), len(policy.FOUNDATIONS))
        self.assert_quarantined()

    def test_lost_boot_receipt_fails_closed_after_controls_were_enabled(self):
        with patch.object(policy, 'atomic_json', side_effect=OSError('injected receipt failure')):
            with self.assertRaises(OSError):
                policy.apply(self.ctx)
        self.assert_quarantined()

    def test_failed_stop_readback_does_not_report_disabled(self):
        self.put(policy.BUDGET, '-1')
        original = policy._write
        def write(ctx, name, value):
            if name != policy.BUDGET:
                original(ctx, name, value)
        with patch.object(policy, '_write', write):
            result = policy.apply(self.ctx, 'stop')
        self.assertEqual(result['state'], 'Failed')
        self.assertIn(policy.BUDGET + ':readback', result['reasons'])
