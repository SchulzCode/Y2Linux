import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.battery import Soc, load_profile, sample
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
        self.assertEqual(soc.observe(181, 4175000, 1, 'Full')['percent'], 100)
        self.assertLessEqual(soc.observe(182, 3390000, 1, 'Discharging')['percent'], 1)
        self.assertIsNone(soc.observe(183, 3390000, 0, 'Unknown')['percent'])
        self.assertEqual(soc.observe(184, 3800000, 1, 'Discharging')['percent'], 45)

    def test_rest_and_hybrid_require_actual_calibration(self):
        p = load_profile(self.ctx)
        p['calibrated_resistance_milliohm'] = 100
        soc = Soc(p)
        value = None
        for now in range(121):
            value = soc.observe(now, 3800000, 1, 'Discharging', current=-100000)
        self.assertTrue(value['resting'])
        self.assertEqual(value['source'], 'hybrid')

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


if __name__ == '__main__':
    unittest.main()
