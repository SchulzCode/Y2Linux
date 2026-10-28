import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

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

    def test_backlight_is_off_before_init_powers_down_even_without_reborn(self):
        self.put('/sys/class/backlight/panel/bl_power', '0\n')
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('reboot', 'user', 3)
        self.now += 3 * 10**9
        c.execute()
        self.assertEqual(self.ctx.read('/sys/class/backlight/panel/bl_power'), '4')
        self.assertEqual(c.intent['stop_results']['backlight'], {'ok': True, 'reason': None})
        self.assertEqual(self.calls[-1][0], ['/bin/busybox', 'reboot'])

    def test_unacknowledged_shutdown_blanks_the_panel_before_reborn_is_killed(self):
        # Reborn hung or crashed: the deadline fires without an acknowledgement.
        order = []
        self.put('/sys/class/backlight/panel/bl_power', '0\n')
        def runner(argv, **kwargs):
            order.append(('stop' if argv[-1] == 'stop' else argv[0], self.ctx.read('/sys/class/backlight/panel/bl_power')))
            return {'ok': True, 'reason': None, 'output': None}
        self.ctx.runner = runner
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('poweroff', 'user', 3)
        self.now += 3 * 10**9
        c.execute()
        first_stop = next(i for i, (name, _) in enumerate(order) if name == 'stop')
        self.assertEqual(order[first_stop][1], '4', 'panel is already dark when Reborn is stopped')
        self.assertEqual(c.intent['stop_results']['backlight_early'], {'ok': True, 'reason': None})

    def test_acknowledged_shutdown_leaves_reborns_dark_frame_alone_until_the_end(self):
        self.put('/run/reborn/process.pid', '123')
        self.put('/proc/123/stat', '123 (reborn) S ' + ' '.join(['0'] * 18 + ['900']))
        self.ctx.path('/proc/123/exe').symlink_to(self.ctx.path('/usr/bin/reborn'))
        self.put('/sys/class/backlight/panel/bl_power', '4\n')
        c = Coordinator(self.ctx, lambda: self.now)
        intent = c.request('poweroff', 'user', 3)
        c.ack(intent['id'], 123, 'Ready')
        c.execute()
        self.assertNotIn('backlight_early', c.intent['stop_results'])
        self.assertEqual(c.intent['stop_results']['backlight'], {'ok': True, 'reason': None})

    def test_missing_backlight_is_recorded_not_fatal(self):
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('poweroff', 'user', 3)
        self.now += 3 * 10**9
        c.execute()
        self.assertEqual(c.intent['stop_results']['backlight']['reason'], 'no_backlight')
        self.assertEqual(self.calls[-1][0], ['/bin/busybox', 'poweroff'])

    def test_shutdown_steps_reach_pmsg_and_stage_writes_get_three_seconds(self):
        # F39 (471831b6): an update restart left no stage and no trail.
        self.put('/dev/pmsg0', '')
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('reboot', 'update', 3)
        self.now += 3 * 10**9
        c.execute()
        lines = self.ctx.read('/dev/pmsg0').splitlines()
        steps = [line.split(None, 2)[2] for line in lines]
        # F39 (8.39-8.41): every step is announced before it runs, so a stall
        # names the step it happened in; the radio stop is split and comes
        # after the sync, before shutdown_complete.
        self.assertEqual(steps, [
            'execute reboot update', 'watchdog watchdog_unavailable',
            'begin journal_pending', 'journal_pending injected_failure',
            'begin S90reborn', 'S90reborn injected_failure', 'begin kill', 'begin backlight',
            'begin time_remember', 'time_remember injected_failure', 'begin sync', 'sync injected_failure',
            'begin S41_wifi', 'S41_wifi injected_failure', 'begin S41_wifi_removed', 'S41_wifi_removed ok',
            'begin S41_bluetooth', 'S41_bluetooth injected_failure',
            'begin S41_daemon', 'S41_daemon injected_failure',
            'begin init_reboot', 'init_reboot injected_failure'])
        stages = [kw['timeout'] for argv, kw in self.calls if 'boot-stage' in argv]
        self.assertEqual(stages, [3])
        self.assertEqual([argv for argv, _ in self.calls if argv[0] == '/etc/init.d/S41y2-connectivity'],
                         [['/etc/init.d/S41y2-connectivity', 'stop-' + n] for n in ('wifi', 'bluetooth', 'daemon')])

    def test_radio_stop_runs_after_sync_and_before_shutdown_complete(self):
        def runner(argv, **kwargs):
            self.calls.append((argv, kwargs))
            return {'ok': True, 'reason': None, 'output': None}
        self.ctx.runner = runner
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('poweroff', 'user', 3)
        self.now += 3 * 10**9
        c.execute()
        order = [argv[-1] for argv, _ in self.calls]
        self.assertEqual(order, ['shutdown_pending', 'stop', 'time-remember', 'sync', 'stop-wifi',
                                 'stop-bluetooth', 'stop-daemon', 'shutdown_complete', 'poweroff'])
        self.assertTrue(all(kw['timeout'] <= 5 for _, kw in self.calls))
        for name in ('S41_wifi', 'S41_wifi_removed', 'S41_bluetooth', 'S41_daemon'):
            self.assertEqual(c.intent['stop_results'][name], {'ok': True, 'reason': None})

    def test_wifi_removal_is_waited_for_but_bounded(self):
        self.put('/sys/class/net/wlan0/operstate', 'up')
        self.put('/dev/pmsg0', '')
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('reboot', 'update', 3)
        clock = [100.0]
        def sleep(seconds):
            clock[0] += seconds
        with mock.patch('y2_platform.power.time.sleep', side_effect=sleep) as slept, \
                mock.patch('y2_platform.power.time.monotonic', side_effect=lambda: clock[0]):
            c.execute()
        self.assertEqual(c.intent['stop_results']['S41_wifi_removed'], {'ok': False, 'reason': 'still_present'})
        self.assertGreaterEqual(slept.call_count, 19)
        self.assertAlmostEqual(clock[0], 102.0, delta=.15)
        self.assertIn('shutdown S41_wifi_removed still_present', self.ctx.read('/dev/pmsg0'))

    def test_shutdown_arms_the_watchdog_and_a_power_off_reset_powers_off(self):
        # F39: a stalled shutdown (3b2364ca drained a full battery while it
        # looked off) ends in the SoC watchdog reset. For a power-off the
        # reset must not start the system: the loader's reason 3 charges or
        # powers off. A restart keeps AUTO_RESTART (update: MISC -> recovery).
        for action, reason, expected in (('poweroff', 'user', '1\n'), ('poweroff', 'low_battery', '1\n'),
                                         ('reboot', 'update', None), ('reboot', 'user', None)):
            with self.subTest(action=action, reason=reason):
                self.put('/dev/watchdog', '')
                self.put('/sys/class/power_supply/BAT0/watchdog_powers_off', '0\n')
                self.put('/run/y2/shutdown.json', 'null')
                c = Coordinator(self.ctx, lambda: self.now)
                c.request(action, reason, 3)
                c.execute()
                self.assertEqual(c.intent['stop_results']['watchdog'], {'ok': True, 'reason': None})
                self.assertEqual(self.ctx.read('/sys/class/power_supply/BAT0/watchdog_powers_off'),
                                 (expected or '0\n').strip())
                # Fed before every step; never closed with the magic 'V'.
                fed = self.ctx.read('/dev/watchdog')
                self.assertGreaterEqual(len(fed), 10)
                self.assertEqual(set(fed), {'.'})
                os.close(c.watchdog)

    def test_power_off_without_the_kernel_switch_still_arms_the_watchdog(self):
        self.put('/dev/watchdog', '')
        c = Coordinator(self.ctx, lambda: self.now)
        c.request('poweroff', 'user', 3)
        c.execute()
        self.assertEqual(c.intent['stop_results']['watchdog'], {'ok': False, 'reason': 'powers_off_unavailable'})
        self.assertTrue(self.ctx.read('/dev/watchdog').startswith('.'))
        os.close(c.watchdog)

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

    def test_idle_waits_for_observation_but_never_past_shutdown_deadline(self):
        c = Coordinator(self.ctx, lambda: self.now)
        # One observation wake each second instead of four accept timeouts.
        self.assertEqual(c.wait_seconds(self.now + 10**9), 1)
        intent = c.request('poweroff', 'low_battery', 3)
        self.now = intent['deadline_monotonic_ns'] - 125000000
        self.assertEqual(c.wait_seconds(self.now + 10**9), .125)
        self.now += 125000000
        self.assertEqual(c.wait_seconds(self.now + 10**9), 0)
        self.assertTrue(c.due())
        c.execute()
        # A failed init request must not turn an expired deadline into a spin.
        self.assertEqual(c.wait_seconds(self.now + 10**9), 1)
        self.assertEqual(c.wait_seconds(self.now - 1), 0)


if __name__ == '__main__':
    unittest.main()
