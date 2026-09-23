import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.power import Coordinator, LowBattery, load_policy


class PowerContract(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ctx = Context(self.tmp.name)
        self.put('/proc/sys/kernel/random/boot_id', 'one')
        self.now = 10**9
        self.calls = []
        def runner(argv, **kwargs):
            self.calls.append((argv, kwargs))
            return {'ok': False, 'reason': 'injected_failure', 'output': None}
        self.ctx.runner = runner

    def put(self, name, value):
        p = self.ctx.path(name)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(value)

    def test_ignored_app_and_failed_sync_do_not_block_final_init_request(self):
        c = Coordinator(self.ctx, lambda: self.now)
        intent = c.request('poweroff', 'user', 3)
        self.assertFalse(c.due())
        self.now += 3 * 10**9
        self.assertTrue(c.due())
        # Same request cannot extend grace; conflicting intent is rejected.
        self.assertEqual(c.request('poweroff', 'user', 30)['deadline_monotonic_ns'], intent['deadline_monotonic_ns'])
        with self.assertRaises(ValueError):
            c.request('reboot', 'service')
        c.execute()
        self.assertEqual(self.calls[-1][0], ['/bin/busybox', 'poweroff'])
        self.assertTrue(all(kw['timeout'] <= 5 for _, kw in self.calls))
        self.assertEqual(c.intent['state'], 'Failed')
        self.assertFalse(c.due())

    def test_ack_requires_same_request_pid_start_time_and_boot(self):
        self.put('/run/reborn/process.pid', '123')
        self.put('/proc/123/stat', '123 (reborn) S ' + ' '.join(['0'] * 18 + ['900']))
        self.ctx.path('/proc/123/exe').symlink_to(self.ctx.path('/usr/bin/reborn'))
        c = Coordinator(self.ctx, lambda: self.now)
        intent = c.request('reboot', 'update')
        with self.assertRaises(ValueError):
            c.ack('old', 123, 'Ready')
        with self.assertRaises(ValueError):
            c.ack(intent['id'], 124, 'Ready')
        self.put('/proc/123/stat', '123 (reborn) S ' + ' '.join(['0'] * 18 + ['901']))
        with self.assertRaises(ValueError):
            c.ack(intent['id'], 123, 'Ready')
        self.put('/proc/123/stat', '123 (reborn) S ' + ' '.join(['0'] * 18 + ['900']))
        c.ack(intent['id'], 123, 'Ready')
        self.assertTrue(c.due())
        resumed = Coordinator(self.ctx, lambda: self.now)
        self.assertEqual(resumed.intent['id'], intent['id'])
        self.put('/proc/sys/kernel/random/boot_id', 'two')
        self.assertIsNone(Coordinator(self.ctx).intent)

    def test_thresholds_disabled_missing_sensor_and_synthetic_debounce(self):
        self.assertEqual(load_policy(self.ctx), (None, 'thresholds_disabled_PHYSICAL_GATE'))
        # Deliberately artificial units/values: no hardware threshold recommendation.
        policy = dict(schema=1, enabled=True, critical_uv=10, low_uv=20, recover_uv=30,
                      critical_samples=2, grace_seconds=3, qualification_reference='synthetic test')
        self.put('/data/system/platform/power-policy.json', json.dumps(policy))
        configured, reason = load_policy(self.ctx)
        self.assertIsNone(reason)
        battery = LowBattery()
        self.assertEqual(battery.observe(configured, None, 1), 'Unavailable')
        self.assertEqual(battery.observe(configured, 31, 1), 'Normal')
        self.assertEqual(battery.observe(configured, 19, 1), 'Low')
        self.assertEqual(battery.observe(configured, 22, 1), 'Low')
        self.assertEqual(battery.observe(configured, 9, 1), 'Critical')
        self.assertEqual(battery.observe(configured, 9, 1), 'ShutdownPending')
        self.assertEqual(battery.observe(None, 9, 1), 'Unavailable')


if __name__ == '__main__':
    unittest.main()
