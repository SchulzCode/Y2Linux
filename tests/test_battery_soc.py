import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.battery import Soc, boot_kind, load_profile, sample
from y2_platform.common import Context
from y2_platform.power import LowBattery, load_policy

ROOT = Path(__file__).resolve().parents[1]
ETC = ROOT / 'buildroot/board/y2/production-overlay/etc/y2linux'


class BatterySoc(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.put('/etc/y2linux/battery-profile.json', (ETC / 'battery-profile.json').read_text())

    def put(self, path, text):
        dest = self.ctx.path(path)
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(text)

    def test_invalid_override_fails_closed_and_real_fg_has_priority(self):
        self.put('/data/system/platform/battery-profile.json', '{"schema":1,"voltage_soc":[]}')
        self.assertIsNone(load_profile(self.ctx))
        soc = Soc(None)
        self.assertIsNone(soc.observe(0, 3800000, 1, 'Discharging')['percent'])
        value = soc.observe(1, 3800000, 1, 'Discharging', 71)
        self.assertEqual((value['percent'], value['source'], value['confidence']), (71, 'fuel_gauge', 'hardware'))

    def test_no_fake_sensors_charge_limit_or_die_temperature(self):
        self.put('/sys/class/power_supply/BAT0/present', '1')
        self.put('/sys/class/power_supply/BAT0/status', 'Charging')
        self.put('/sys/class/power_supply/BAT0/voltage_now', '3800000')
        self.put('/sys/class/power_supply/USB/constant_charge_current', '450000')
        self.put('/sys/class/thermal/thermal_zone0/temp', '50000')
        value = sample(self.ctx, Soc(load_profile(self.ctx)), 0)
        self.assertEqual(value['source'], 'voltage_estimate')
        self.assertIsNone(value['current_ua'])
        self.assertIsNone(value['temperature_millicelsius'])
        self.assertIsNone(value['charge_counter_uah'])

    def test_sag_rebound_is_monotonic_and_charging_smoothing_bounded(self):
        soc = Soc(load_profile(self.ctx))
        first = soc.observe(0, 3900000, 1, 'Discharging')['percent']
        low = soc.observe(60, 3700000, 1, 'Discharging')['percent']
        rebound = soc.observe(120, 3950000, 1, 'Discharging')['percent']
        self.assertLessEqual(rebound, low)
        self.assertGreaterEqual(low, first - 1)
        charging = soc.observe(180, 4175000, 1, 'Charging')['percent']
        self.assertLessEqual(charging, rebound + 1)
        # 28 (stock 100Percent tracking): Full counts up one point per 10 s.
        self.assertEqual(soc.observe(181, 4175000, 1, 'Full')['percent'], charging + 1)
        self.assertEqual(soc.observe(185, 4175000, 1, 'Full')['percent'], charging + 1)
        now = 191
        while soc.observe(now, 4090000, 1, 'Full')['percent'] < 100:
            now += 10
        # Resting full pack of this charger (measured 4.09 V) is 100%, not 94%.
        self.assertEqual(soc.observe(now + 1, 4090000, 1, 'Full')['percent'], 100)
        # An implausible Full at a low voltage never forces 100%.
        self.assertLess(Soc(load_profile(self.ctx)).observe(0, 3900000, 1, 'Full')['percent'], 100)
        self.assertLessEqual(soc.observe(now + 2, 3390000, 1, 'Discharging')['percent'], 1)
        self.assertIsNone(soc.observe(now + 3, 3390000, 0, 'Unknown')['percent'])
        self.assertEqual(soc.observe(now + 4, 3800000, 1, 'Discharging')['percent'], 54)

    def test_rest_and_hybrid_require_actual_calibration(self):
        p = load_profile(self.ctx)
        p['calibrated_resistance_milliohm'] = 100
        soc = Soc(p)
        value = None
        for now in range(121):
            value = soc.observe(now, 3800000, 1, 'Discharging', current=-100000)
        self.assertTrue(value['resting'])
        self.assertEqual(value['source'], 'hybrid')

    def test_sleep_gap_exposes_voltage_loss_without_changing_curve_or_rate(self):
        self.put('/sys/class/power_supply/BAT0/present', '1')
        self.put('/sys/class/power_supply/BAT0/status', 'Discharging')
        self.put('/sys/class/power_supply/BAT0/voltage_now', '4000000')
        soc = Soc(load_profile(self.ctx))
        physical_time = iter([100., 700., 7900.])
        def clock(clock_id):
            return next(physical_time) if clock_id == time.CLOCK_BOOTTIME else 100.
        with patch('y2_platform.battery.time.clock_gettime', side_effect=clock):
            self.assertEqual(sample(self.ctx, soc)['percent'], 87)
            self.put('/sys/class/power_supply/BAT0/voltage_now', '3800000')
            after_ten_minutes = sample(self.ctx, soc)
            # The existing one-point-per-minute limit gets ten real minutes,
            # not zero suspended minutes or an artificial one-minute ceiling.
            self.assertEqual(after_ten_minutes['percent'], 77)
            self.assertLess(after_ten_minutes['filtered_voltage_uv'], 3810000)
            after_two_hours = sample(self.ctx, soc)
            self.assertEqual(after_two_hours['percent'], 54)
            self.assertFalse(after_two_hours['resting'])
            self.assertFalse(after_two_hours['provisional'])
            self.assertIsNone(after_two_hours['current_ua'])

    def test_sleep_time_alone_never_changes_soc_and_rebound_is_not_charge(self):
        soc = Soc(load_profile(self.ctx))
        self.assertEqual(soc.observe(0, 3800000, 1, 'Discharging')['percent'], 54)
        self.assertEqual(soc.observe(28800, 3800000, 1, 'Discharging')['percent'], 54)
        self.assertEqual(soc.observe(57600, 4000000, 1, 'Discharging')['percent'], 54)

    def test_low_soc_estimate_warns_but_shutdown_requires_trust_or_hard_floor(self):
        self.put('/etc/y2linux/power-policy.json', (ETC / 'power-policy.json').read_text())
        policy, reason = load_policy(self.ctx)
        self.assertIsNone(reason)
        guard = LowBattery()
        estimate = {'percent': 1, 'confidence': 'provisional'}
        for _ in range(10):
            self.assertEqual(guard.observe(policy, 3800000, 1, estimate), 'Critical')
        for _ in range(4):
            self.assertEqual(guard.observe(policy, 3300000, 1, estimate), 'Critical')
        self.assertEqual(guard.observe(policy, 3300000, 1, estimate), 'ShutdownPending')
        guard = LowBattery()
        for _ in range(5):
            state = guard.observe(policy, 3800000, 1, {'percent': 1, 'confidence': 'hardware'})
        self.assertEqual(state, 'ShutdownPending')


    def test_stock_profile_is_the_fm_kernel_table(self):
        """POWER-FINAL-16: fgauge_get_profile(25 C) and r_profile from the Y2 FM kernel."""
        p = load_profile(self.ctx)
        self.assertFalse(p['provisional'])
        self.assertEqual(p['design_capacity_uah'], 1300000)
        self.assertEqual(p['voltage_soc'][0], [3400000, 0])
        self.assertEqual(p['voltage_soc'][-1], [4175000, 100])
        self.assertEqual(len(p['voltage_soc']), 95)
        self.assertIn([3779000, 50], p['voltage_soc'])  # stock DOD 50 at 3779 mV
        self.assertEqual(len(p['voltage_resistance_milliohm']), 95)
        self.assertTrue(all(190 <= r <= 330 for _, r in p['voltage_resistance_milliohm']))
        value = Soc(p).observe(0, 3779000, 1, 'Discharging')
        self.assertEqual((value['percent'], value['confidence']), (50, 'calibrated_estimate'))
        self.assertEqual(value['remaining_uah_estimate'], 650000)

    def test_charging_uses_stock_oam_resistance_times_sense_current(self):
        p = load_profile(self.ctx)
        soc = Soc(p)
        # 450 mA * ~0.2 ohm ~= 90 mV: 3.89 V under charge reads as ~3.80 V OCV.
        value = soc.observe(0, 3890000, 1, 'Charging', charge_current=450000)
        self.assertEqual(value['source'], 'stock_oam')
        self.assertTrue(53 <= value['percent'] <= 57, value)
        # Without a sense current the fixed charging offset still applies.
        fallback = Soc(p).observe(0, 3890000, 1, 'Charging')
        self.assertEqual(fallback['source'], 'voltage_estimate')
        # An absurd current never moves the estimate by more than 300 mV.
        clipped = Soc(p).observe(0, 4100000, 1, 'Charging', charge_current=1200000)
        self.assertGreaterEqual(clipped['percent'], 49)
        # Discharging never uses the charger-path current.
        self.assertEqual(Soc(p).observe(0, 3800000, 1, 'Discharging', charge_current=450000)['source'],
                         'voltage_estimate')

    # POWER-FINAL-28: the owner's case. On the charger 100 %, after a restart
    # about 80 %; wanted 100 % when full, then 99, 98, 97 ... (ledger F28-F30).
    def full_soc(self, rest_uv=4150000):
        soc = Soc(load_profile(self.ctx))
        now = 0
        soc.observe(now, 4100000, 1, 'Charging', charge_current=70000)
        while soc.observe(now, rest_uv, 1, 'Full')['percent'] < 100:
            now += 10
        return soc, now

    def test_charging_shows_at_most_99_until_full(self):
        soc = Soc(load_profile(self.ctx))
        for now in range(0, 3600, 10):
            value = soc.observe(now, 4170000, 1, 'Charging')
            self.assertLessEqual(value['percent'], 99)
        # 28 finish stage at the target, 70 mA: the estimate reaches 100, shown 99.
        for now in range(3600, 7200, 10):
            value = soc.observe(now, 4175000, 1, 'Charging', charge_current=10000)
        self.assertEqual((value['estimate_percent'], value['percent']), (100, 99))
        self.assertEqual(soc.observe(7200, 4150000, 1, 'Full')['percent'], 100)

    def test_full_stays_100_on_the_charger_then_counts_down_one_by_one(self):
        soc, now = self.full_soc()
        anchor = soc.anchor
        self.assertTrue(98 <= anchor <= 99.5, anchor)
        # Maintenance hold drains and recharges on the charger: still 100.
        for uv, status in ((4120000, 'Full'), (4105000, 'Not charging'), (4140000, 'Charging')):
            now += 600
            self.assertEqual(soc.observe(now, uv, 1, status)['percent'], 100)
        # Unplugged: 100, then single steps as the pack really discharges.
        shown = []
        uv = 4150000
        for _ in range(400):
            now += 30
            uv -= 1000
            shown.append(soc.observe(now, uv, 1, 'Discharging')['percent'])
        self.assertEqual(shown[0], 100)
        steps = [a - b for a, b in zip(shown, shown[1:])]
        self.assertTrue(all(step in (0, 1) for step in steps), steps)
        self.assertIn(99, shown)
        self.assertIn(98, shown)
        self.assertIn(97, shown)

    def test_restart_keeps_the_display_instead_of_the_boot_sag(self):
        soc, now = self.full_soc()
        state = soc.state()
        self.assertEqual(state['ui_percent'], 100)
        # 27 boot 5919b4fe: first sample 4.002 V under boot load (87 % raw).
        cold = Soc(load_profile(self.ctx)).observe(0, 4002000, 1, 'Discharging')
        self.assertEqual(cold['percent'], 87)
        # Power-on OCV alone (4.062 V latched before load) already says ~92 %.
        ocv = Soc(load_profile(self.ctx)).observe(0, 4002000, 1, 'Discharging', boot_ocv=4062000)
        self.assertEqual(ocv['percent'], 92)
        # With the saved display value (stock RTC spare) the restart shows 100.
        again = Soc(load_profile(self.ctx), state).observe(0, 4002000, 1, 'Discharging', boot_ocv=4150000)
        self.assertEqual(again['percent'], 100)
        self.assertEqual(again['anchor_percent'], state['anchor_percent'])

    def test_saved_value_is_refused_beyond_40_points_and_charger_moved_ocv(self):
        state = {'schema': 1, 'ui_percent': 100, 'anchor_percent': 98.8}
        stale = Soc(load_profile(self.ctx), state).observe(0, 3700000, 1, 'Discharging', boot_ocv=3700000)
        self.assertLess(stale['percent'], 60)                     # 40-point rule
        # Booted on a charger: an OCV 31+ points away from the live sample loses.
        p = load_profile(self.ctx)
        moved = Soc(p).observe(0, 3800000, 1, 'Charging', boot_ocv=4150000)
        self.assertLess(moved['percent'], 70)
        # Garbage in the saved file is ignored.
        junk = Soc(p, {'ui_percent': 'x', 'anchor_percent': 7}).observe(0, 3779000, 1, 'Discharging')
        self.assertEqual((junk['percent'], junk['anchor_percent']), (50, None))

    def test_start_on_a_charger_after_a_low_battery_poweroff_runs(self):
        # F36 (97f11bb1): turned off at 5 %, charged on the charging screen for
        # an hour, then every start showed 5 % and turned off again at 4.03 V.
        self.put('/etc/y2linux/power-policy.json', (ETC / 'power-policy.json').read_text())
        policy, _ = load_policy(self.ctx)
        saved = {'schema': 1, 'ui_percent': 5, 'anchor_percent': None}
        for now, voltage, ocv in ((3850, 4027148, 3620000), (17, 3714257, 3700000)):
            soc, guard = Soc(load_profile(self.ctx), saved), LowBattery()
            for tick in range(6):
                value = soc.observe(now + tick, voltage, 1, 'Charging', charge_current=450000, boot_ocv=ocv)
                state = guard.observe(policy, voltage, 1, value)
            self.assertGreater(value['percent'], 30)
            self.assertNotEqual(state, 'ShutdownPending')
        # The stale power-on OCV alone no longer decides a late first sample.
        late = Soc(load_profile(self.ctx)).observe(3850, 3779000, 1, 'Discharging', boot_ocv=3850000)
        fresh = Soc(load_profile(self.ctx)).observe(10, 3779000, 1, 'Discharging', boot_ocv=3850000)
        self.assertLess(late['percent'], fresh['percent'])
        # F54: an OCV below the loaded voltage predates a charge and is ignored.
        below = Soc(load_profile(self.ctx)).observe(10, 3779000, 1, 'Discharging', boot_ocv=3500000)
        self.assertEqual(below['percent'], late['percent'])
        # On battery a saved 5 % still turns a drained player off.
        soc, guard = Soc(load_profile(self.ctx), saved), LowBattery()
        for tick in range(6):
            state = guard.observe(policy, 3627026, 1, soc.observe(10 + tick, 3627026, 1, 'Discharging'))
        self.assertEqual(state, 'ShutdownPending')
        # A charger that cannot carry the load is still caught by the voltage floor.
        guard = LowBattery()
        for _ in range(5):
            state = guard.observe(policy, 3390000, 1, {'percent': 40, 'confidence': 'calibrated_estimate',
                                                       'charging': True, 'status': 'Charging'})
        self.assertEqual(state, 'ShutdownPending')

    def test_warm_restart_keeps_the_saved_percent_not_the_old_power_on_ocv(self):
        # F54, 98499743 (8.38): cold start near empty, charged to 95 %, then an
        # update restart (boot_reason=4) read the old 0x724 OCV and showed 13 %.
        self.assertEqual(boot_kind('valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 panel=gc9503v'),
                         'warm')
        self.assertEqual(boot_kind('valid=1 boot_mode=8 boot_reason=3 offline=1 recovery_image=0'), 'charged')
        self.assertEqual(boot_kind('valid=1 boot_mode=0 boot_reason=0 offline=0'), 'cold')
        self.assertIsNone(boot_kind('valid=0 boot_mode=-1 boot_reason=-1 offline=0'))
        self.assertIsNone(boot_kind(None))
        saved = {'schema': 1, 'ui_percent': 95, 'anchor_percent': None}
        for boot in ('warm', None):
            value = Soc(load_profile(self.ctx), saved).observe(14, 4021655, 1, 'Discharging',
                                                               boot_ocv=3545000, boot=boot)
            self.assertEqual(value['percent'], 95)
        self.assertEqual(value['start'], 'unknown/saved')
        # Played down since the cold start: the old OCV is plausible by voltage
        # alone, so only the boot reason tells it apart.
        saved = {'schema': 1, 'ui_percent': 40, 'anchor_percent': None}
        warm = Soc(load_profile(self.ctx), saved).observe(14, 3700000, 1, 'Discharging',
                                                          boot_ocv=3950000, boot='warm')
        self.assertEqual((warm['percent'], warm['start']), (40, 'warm/saved'))
        cold = Soc(load_profile(self.ctx), saved).observe(14, 3700000, 1, 'Discharging',
                                                          boot_ocv=3950000, boot='cold')
        self.assertEqual(cold['start'], 'cold/ocv')
        self.assertGreater(cold['percent'], 70)
        # Stock dod_init keeps the saved value beyond 40 points after a warm
        # restart; only a live estimate 40+ above it proves it wrong.
        low = Soc(load_profile(self.ctx), {'schema': 1, 'ui_percent': 13}).observe(
            14, 4021655, 1, 'Discharging', boot='warm')
        self.assertEqual((low['percent'], low['start']), (89, 'warm/live'))
        high = Soc(load_profile(self.ctx), {'schema': 1, 'ui_percent': 95}).observe(
            14, 3700000, 1, 'Discharging', boot='warm')
        self.assertEqual(high['percent'], 95)

    def test_warm_restart_on_a_charger_keeps_the_lower_saved_percent(self):
        # F54, c0a4f35d (8.41): update restart while charging at 28 % showed 42 %,
        # then 37, then 29 at unplug.
        saved = {'schema': 1, 'ui_percent': 28, 'anchor_percent': 96.74}
        value = Soc(load_profile(self.ctx), saved).observe(14, 3633837, 1, 'Charging',
                                                           boot_ocv=3720000, boot='warm')
        self.assertEqual(value['percent'], 28)
        # F36 is kept: after LK's charging screen (offline=1) or a cold start on
        # a charger, a lower saved value still gives way to the live one.
        saved = {'schema': 1, 'ui_percent': 5, 'anchor_percent': None}
        for boot, status in (('charged', 'Discharging'), ('charged', 'Charging'), ('cold', 'Charging'), (None, 'Charging')):
            value = Soc(load_profile(self.ctx), saved).observe(17, 3714257, 1, status, boot=boot)
            self.assertGreater(value['percent'], 20, (boot, status))
            self.assertTrue(value['start'].endswith('/live'), (boot, status))

    def test_sample_reads_the_boot_reason_once(self):
        self.put('/sys/class/power_supply/BAT0/present', '1')
        self.put('/sys/class/power_supply/BAT0/status', 'Discharging')
        self.put('/sys/class/power_supply/BAT0/voltage_now', '4021655')
        self.put('/sys/class/power_supply/BAT0/voltage_ocv', '3545000')
        self.put('/sys/firmware/y2_boot/metadata', 'valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0')
        soc = Soc(load_profile(self.ctx), {'schema': 1, 'ui_percent': 95})
        self.assertEqual(sample(self.ctx, soc, 14)['start'], 'warm/saved')
        self.assertEqual(sample(self.ctx, soc, 15)['percent'], 95)

    def test_awake_load_follows_the_drain_not_the_loaded_voltage(self):
        # 28 boot 8f43c71e: started at 89 % from the power-on OCV, then the old
        # estimator pulled toward the loaded 3.95-3.98 V and showed 85 within
        # 3.5 minutes. Stock oam_run integrates the model current instead.
        soc = Soc(load_profile(self.ctx))
        value = soc.observe(0, 3946069, 1, 'Discharging', boot_ocv=4022000)
        self.assertEqual(value['percent'], 89)
        for now in range(1, 211):
            value = soc.observe(now, 3970000, 1, 'Discharging')
        self.assertGreaterEqual(value['percent'], 88)
        # That model current is a real drain: sustained load still discharges.
        for now in range(211, 3811):
            value = soc.observe(now, 3940000, 1, 'Discharging')
        self.assertLess(value['percent'], 85)
        # A rested voltage after a suspend gap is still taken directly.
        after = soc.observe(3811 + 1800, 3800000, 1, 'Discharging')
        self.assertLess(after['estimate_percent'], value['estimate_percent'])

    def test_service_persists_the_display_value(self):
        source = (ROOT / 'tools/platform/y2_platform/power.py').read_text()
        self.assertIn("soc = Soc(load_profile(ctx), ctx.json(UI_STATE, None))", source)
        self.assertIn("atomic_json(ctx.path(UI_STATE), ui_state, durable=True)", source)
        battery = (ROOT / 'tools/platform/y2_platform/battery.py').read_text()
        self.assertIn("ctx.integer(base + 'voltage_ocv')", battery)


if __name__ == '__main__':
    unittest.main()
