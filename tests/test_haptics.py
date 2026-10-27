"""Vibration motor (#28): regulator-haptic on the MT6323 VIBR LDO, as stock drives it."""
from pathlib import Path
import os
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def block(text, start):
    body = text[text.index(start):]
    return body[:body.index('};') + 2]


class Haptics(unittest.TestCase):
    def setUp(self):
        self.dts = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()

    def test_vibr_rail_never_exceeds_the_stock_voltage(self):
        # Stock vibr_power_set: VOSEL 5 = 2.8 V, never changed at run time.
        rail = block(self.dts, 'vibr: ldo_vibr {')
        self.assertIn('regulator-name = "vibr";', rail)
        self.assertIn('regulator-min-microvolt = <1800000>;', rail)
        self.assertIn('regulator-max-microvolt = <2800000>;', rail)
        self.assertNotIn('regulator-always-on', rail)
        self.assertNotIn('regulator-boot-on', rail)
        # A child of the MT6323 regulators node (before the PMIC's I2C neighbour).
        order = [self.dts.index(s) for s in ('regulators { compatible = "mediatek,mt6323-regulator";',
                                             'vibr: ldo_vibr {', 'i2c0: i2c@11007000 {')]
        self.assertEqual(order, sorted(order))

    def test_haptics_node_uses_the_rail_within_its_range(self):
        node = block(self.dts, 'haptics { compatible = "regulator-haptic";')
        self.assertIn('haptic-supply = <&vibr>;', node)
        self.assertIn('min-microvolt = <1800000>; max-microvolt = <2800000>;', node)

    def test_config_builds_the_driver_in(self):
        config = (ROOT / 'kernel/config/production.config').read_text()
        for option in ('CONFIG_INPUT_MISC', 'CONFIG_INPUT_FF_MEMLESS', 'CONFIG_INPUT_REGULATOR_HAPTIC',
                       'CONFIG_REGULATOR_MT6323'):
            self.assertRegex(config, '(?m)^' + option + '=y$')

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'), 'needs a built kernel')
    def test_built_kernel_and_dtb_carry_it(self):
        build = Path(os.environ['Y2_ARTIFACT_TEST_ROOT'])
        config = (build / 'kernel/.config').read_text()
        self.assertRegex(config, r'(?m)^CONFIG_INPUT_REGULATOR_HAPTIC=y$')
        dtb = (build / 'y2.dtb').read_bytes()
        self.assertIn(b'regulator-haptic\0', dtb)
        self.assertTrue(re.search(rb'vibr\0', dtb))


if __name__ == '__main__':
    unittest.main()
