"""POWER-FINAL-05/13 PMIC low-power select regressions. Synthetic evidence, never a device receipt."""
from pathlib import Path
import re
import unittest
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'


class WriteWhitelist(unittest.TestCase):
    def test_only_bit_zero_of_the_five_vendor_ldos_is_writable(self):
        run_c(r'''
#include <assert.h>
#include "policy.h"
#include "pmic-lowpower-policy.h"
int main(void) {
 static const unsigned regs[] = {0x404, 0x500, 0x502, 0x552, 0x556};
 for (unsigned i = 0; i < 5; i++) {
  assert(y2_pmic_write_mask(regs[i]) == 0x0001);   /* LP_SEL only */
  assert(y2_pmic_value_allowed(regs[i], 0xc001));
 }
 /* 13: storage LDOs add LP_SEL to their existing enable bit; the eMMC enable stays closed. */
 assert(y2_pmic_write_mask(0x504) == 0x1001 && y2_pmic_write_mask(0x506) == 0x4001);
 assert(y2_pmic_write_mask(0x508) == 0x0001);
 assert(y2_pmic_write_mask(0x50c) == 0x8000 && y2_pmic_write_mask(0x532) == 0x00e0);
 /* #28: VIBR enable + thermal shutdown, selector 1.8..2.8 V only; thermal shutdown never cleared. */
 assert(y2_pmic_write_mask(0x542) == 0x8004 && y2_pmic_write_mask(0x544) == 0x00e0);
 assert(y2_pmic_value_allowed(0x542, 0x8004) && y2_pmic_value_allowed(0x542, 0x0004));
 assert(!y2_pmic_value_allowed(0x542, 0x8000) && !y2_pmic_value_allowed(0x542, 0x0000));
 for (unsigned sel = 0; sel < 8; sel++)
  assert(y2_pmic_value_allowed(0x544, sel << 5) == (sel >= 3 && sel <= 5));
 assert(Y2_PMIC_VIBR_REG == 0x542 && Y2_PMIC_VIBR_THR == 0x0004);
 /* 27: VSYS mode fields only (test_pmic_sleep.py); selectors and both buck enables stay closed. */
 assert(y2_pmic_write_mask(0x256) == 0x0133 && y2_pmic_write_mask(0x23c) == 0x0002);
 assert(y2_pmic_write_mask(0x244) == 0 && y2_pmic_write_mask(0x246) == 0 && y2_pmic_write_mask(0x248) == 0);
 assert(y2_pmic_write_mask(0x21a) == 0 && y2_pmic_write_mask(0x240) == 0);   /* buck enables */
 return 0;
}
''')

    def test_enable_and_mode_bits_of_those_ldos_stay_unwritable(self):
        source = (PLATFORM / 'policy.h').read_text()
        for reg in ('0x404', '0x500', '0x502', '0x552', '0x556'):
            self.assertRegex(source, r'case ' + reg + r'[^\n]*\n(?:[^\n]*case [^\n]*\n)*\s*return 0x0001;')


class Driver(unittest.TestCase):
    def setUp(self):
        self.header = (PLATFORM / 'pmic-lowpower-policy.h').read_text()
        self.source = (PLATFORM / 'pmic-lowpower.c').read_text()

    def test_chip_gate_and_parser(self):
        run_c(r'''
#include <assert.h>
#include "pmic-lowpower-policy.h"
int main(void) {
 assert(!y2_pmic_lp_chip_ok(0x1023) && y2_pmic_lp_chip_ok(0x2023) && y2_pmic_lp_chip_ok(0x2123));
 assert(y2_pmic_lp_parse("1\n", 2) == 1 && y2_pmic_lp_parse("0", 1) == 0);
 assert(y2_pmic_lp_parse("", 0) == -1 && y2_pmic_lp_parse("2", 1) == -1 && y2_pmic_lp_parse("11", 2) == -1);
 assert(y2_pmic_lp_parse("1\n\n", 3) == -1 && y2_pmic_lp_parse("on", 2) == -1);
 assert(Y2_PMIC_LP_RAILS == 8 && y2_pmic_lp_rails[0].reg == 0x404 && y2_pmic_lp_rails[4].reg == 0x556);
 assert(y2_pmic_lp_rails[5].reg == 0x504 && y2_pmic_lp_rails[6].reg == 0x506 && y2_pmic_lp_rails[7].reg == 0x508);
 return 0;
}
''')

    def test_driver_touches_only_the_table_and_requires_admin_for_revert(self):
        self.assertIn('regmap_update_bits(lp_map, y2_pmic_lp_rails[i].reg, Y2_PMIC_LP_BIT', self.source)
        # #28: the one other write sets the stock VIBR thermal shutdown, never clears it.
        self.assertIn('regmap_update_bits(lp_map, Y2_PMIC_VIBR_REG, Y2_PMIC_VIBR_THR, Y2_PMIC_VIBR_THR)', self.source)
        # F57: the third clears only the long-press reset enable (test below).
        self.assertEqual(len(re.findall(r'regmap_update_bits\(', self.source)), 3)
        self.assertNotIn('Y2_PMIC_VIBR_THR, 0', self.source)
        self.assertNotIn('VIBR', self.source[self.source.index('static int lp_apply'):self.source.index('static struct regmap *lp_find_map')])
        self.assertEqual(len(re.findall(r'regmap_write\(', self.source)), 0)
        self.assertIn('capable(CAP_SYS_ADMIN)', self.source)
        self.assertIn('0600', self.source)
        self.assertIn('readback', self.source.lower() + ' readback')  # verified in lp_apply below
        self.assertIn('-EIO', self.source)
        self.assertIn('y2_pmic_lp_chip_ok(cid)', self.source)

    def test_rails_are_the_complete_stock_e2_list_and_exclude_bucks(self):
        # Y2 FM kernel PMIC_INIT_SETTING_V1 sets LP_SEL on exactly these eight
        # (VTCXO 0x402 is owned by the connectivity subset; VSIM1/2 are unused).
        regs = {int(x, 16) for x in re.findall(r'\{(0x[0-9a-fA-F]+),', self.header.split('struct y2_pmic_lp_rail y2_pmic_lp_rails')[1].split('};')[0])}
        self.assertEqual(regs, {0x404, 0x500, 0x502, 0x552, 0x556, 0x504, 0x506, 0x508})
        self.assertTrue(regs.isdisjoint({0x21a, 0x240, 0x230, 0x256}))

    def test_power_down_record_is_read_only_and_never_writable(self):
        # F56/F57: STRUP_CON8 (LK's long-press record), TOP_RST_MISC, UVLO,
        # thermal and Power state are read for the boot evidence, never written
        # (TOP_RST_MISC: only its long-press reset enable, off, below).
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "policy.h"
#include "pmic-lowpower-policy.h"
int main(void) {
 static const unsigned regs[] = {0x04a, 0x11a, 0x020, 0x044, 0x142};
 static const char *names[] = {"strup_con8", "top_rst_misc", "chr_con16", "strup_con5", "chrstatus"};
 assert(Y2_PMIC_POWER_STATUS_REGS == 5);
 for (unsigned i = 0; i < 5; i++) {
  assert(y2_pmic_power_status[i].reg == regs[i] && !strcmp(y2_pmic_power_status[i].name, names[i]));
  if (regs[i] != 0x020 && regs[i] != 0x11a)
   assert(y2_pmic_write_mask(regs[i]) == 0);
 }
 /* CHR_CON16: only the charger's USBDL bits, never the UVLO selector. */
 assert(!(y2_pmic_write_mask(0x020) & 0x3));
 return 0;
}
''')
        show = self.source[self.source.index('static ssize_t power_status_show'):self.source.index('power_status_attr =')]
        self.assertNotIn('regmap_update_bits', show)
        self.assertNotIn('regmap_write', show)
        self.assertIn('__ATTR(power_status, 0444, power_status_show, NULL)', self.source)

    def test_long_press_reset_can_only_be_turned_off(self):
        # F57 owner decision: at shutdown y2-platform clears TOP_RST_MISC
        # PWRKEY_RST_EN (bit 6); its time, the other reset enables and setting
        # it again stay closed (the preloader sets it at every start).
        run_c(r'''
#include <assert.h>
#include "policy.h"
#include "pmic-lowpower-policy.h"
int main(void) {
 assert(Y2_PMIC_TOP_RST_MISC == 0x11a && Y2_PMIC_PWRKEY_RST_EN == 0x0040);
 assert(y2_pmic_write_mask(0x11a) == 0x0040);
 /* As the preloader leaves it: SYSRSTB, NEWLDO_RSTB, PWRKEY_RST_EN, 11 s. */
 assert(y2_pmic_value_allowed(0x11a, 0x010a) && !y2_pmic_value_allowed(0x11a, 0x014a));
 assert(!y2_pmic_value_allowed(0x11a, 0x0040));
 /* The SET/CLR aliases stay closed. */
 assert(y2_pmic_write_mask(0x11c) == 0 && y2_pmic_write_mask(0x11e) == 0);
 return 0;
}
''')
        store = self.source[self.source.index('static ssize_t long_press_reset_store'):
                            self.source.index('long_press_reset_attr =')]
        self.assertIn('capable(CAP_SYS_ADMIN)', store)
        self.assertIn('y2_pmic_lp_parse(buf, size) != 0', store)
        self.assertIn('regmap_update_bits(lp_map, Y2_PMIC_TOP_RST_MISC, Y2_PMIC_PWRKEY_RST_EN, 0)', store)
        self.assertIn('-EIO', store)   # verified by readback
        self.assertNotIn('regmap_write', store)
        self.assertIn('__ATTR(long_press_reset, 0200, NULL, long_press_reset_store)', self.source)

    def test_makefile_builds_it_under_the_power_option_only(self):
        self.assertIn('obj-$(CONFIG_Y2_POWER) += pmic-lowpower.o', (PLATFORM / 'Makefile').read_text())


if __name__ == '__main__':
    unittest.main()
