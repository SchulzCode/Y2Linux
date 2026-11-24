"""POWER-FINAL-13 stock VPROC sleep voltage: policy, write admission and placement.

Synthetic evidence only. The stock values are the Y2 FM kernel's own
PMIC_INIT_SETTING_V1 (0xc04b81f8, E2 branch), read from the retained image.
"""
from pathlib import Path
import re
import unittest
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'

# (reg, value, mask, shift) exactly as the stock pmic_config_interface calls.
STOCK_VPROC = [
    (0x212, 0x2, 0x3, 4),     # QI_VPROC_VSLEEP
    (0x216, 0x1, 0x1, 1),     # VPROC_VOSEL_CTRL
    (0x21c, 0x17, 0x7f, 0),   # VPROC_SFCHG_FRATE
    (0x21c, 0x1, 0x1, 7),     # VPROC_SFCHG_FEN
    (0x21c, 0x1, 0x1, 15),    # VPROC_SFCHG_REN
    (0x222, 0x18, 0x7f, 0),   # VPROC_VOSEL_SLEEP
    (0x230, 0x3, 0x3, 0),     # VPROC_TRANSTD
    (0x230, 0x1, 0x1, 8),     # VPROC_VSLEEP_EN
]


class Policy(unittest.TestCase):
    def test_table_reproduces_the_stock_init_fields_exactly(self):
        header = (PLATFORM / 'vproc-sleep-policy.h').read_text()
        body = header.split('y2_vproc_sleep_fields[Y2_VPROC_SLEEP_STEPS] = {')[1].split('};')[0]
        table = [tuple(int(x, 0) for x in m) for m in
                 re.findall(r'\{(0x[0-9a-f]+), (0x[0-9a-f]+), (0x[0-9a-f]+|0|Y2_VPROC_SLEEP_SELECTOR)\}'
                            .replace('|Y2_VPROC_SLEEP_SELECTOR', ''), body)]
        expected = {}
        for reg, value, mask, shift in STOCK_VPROC:
            m, v = expected.get(reg, (0, 0))
            expected[reg] = (m | mask << shift, v | value << shift)
        rows = {reg: (mask, value) for reg, mask, value in table}
        rows[0x222] = (0x7f, 0x18)  # Y2_VPROC_SLEEP_SELECTOR
        for reg, (mask, value) in expected.items():
            self.assertEqual(rows[reg], (mask, value), hex(reg))
        # VOSEL_ON is preset to the live selector; the HW-mode bit is last.
        self.assertIn('{0x220, 0x007f, 0}', body)
        self.assertTrue(body.strip().rstrip(',').endswith('{0x216, 0x0002, 0x0002}'))

    def test_plan_requires_software_mode_at_the_proven_floor(self):
        run_c(r'''
#include <assert.h>
#include "vproc-sleep-policy.h"
int main(void) {
 struct y2_vproc_sleep s = {0};
 assert(y2_vproc_sleep_plan(&s, 0x0, 0x48, 0x48) == 0);        /* live Y2 12 dump values */
 assert(s.planned[0] == 72 && s.planned[2] == 0x18 && s.planned[5] == 0x2);
 assert(y2_vproc_sleep_plan(&s, 0x2, 0x48, 0x48) == -1);       /* already HW mode */
 assert(y2_vproc_sleep_plan(&s, 0x0, 0x58, 0x58) == -2);       /* 1.25 V OPP still active */
 assert(y2_vproc_sleep_plan(&s, 0x0, 0x48, 0x50) == -2);       /* bank disagrees */
 /* Restore runs in reverse: the mode bit returns to software first. */
 assert(y2_vproc_sleep_fields[y2_vproc_restore_index(0)].reg == 0x216);
 assert(y2_vproc_sleep_fields[y2_vproc_restore_index(Y2_VPROC_SLEEP_STEPS - 1)].reg == 0x220);
 assert(700000 + Y2_VPROC_SLEEP_SELECTOR * 6250 == 850000);
 return 0;
}
''')

    def test_write_policy_admits_only_these_fields_and_values(self):
        run_c(r'''
#include <assert.h>
#include "policy.h"
int main(void) {
 assert(y2_pmic_write_mask(0x212) == 0x0030 && y2_pmic_write_mask(0x216) == 0x0002);
 assert(y2_pmic_write_mask(0x21c) == 0x80ff && y2_pmic_write_mask(0x220) == 0x007f);
 assert(y2_pmic_write_mask(0x222) == 0x007f && y2_pmic_write_mask(0x230) == 0x0103);
 /* Buck enables, the software VOSEL Y2 DVFS owns, the readback bank and every
  * VSYS selector stay closed; 27 admits only VSYS mode/slew bits (test_pmic_sleep.py). */
 assert(!y2_pmic_write_mask(0x21a) && !y2_pmic_write_mask(0x240) && !y2_pmic_write_mask(0x21e));
 assert(!y2_pmic_write_mask(0x224) && y2_pmic_write_mask(0x23c) == 0x0002 && y2_pmic_write_mask(0x256) == 0x0133);
 assert(!y2_pmic_write_mask(0x244) && !y2_pmic_write_mask(0x248) && !y2_pmic_write_mask(0x246) && !y2_pmic_write_mask(0x24a));
 /* VOSEL_ON only at Y2's three DVFS selectors; sleep voltage 0.85..1.25 V. */
 assert(y2_pmic_value_allowed(0x220, 72) && y2_pmic_value_allowed(0x220, 80) && y2_pmic_value_allowed(0x220, 88));
 assert(!y2_pmic_value_allowed(0x220, 0) && !y2_pmic_value_allowed(0x220, 0x18) && !y2_pmic_value_allowed(0x220, 96));
 assert(y2_pmic_value_allowed(0x222, 0x18) && y2_pmic_value_allowed(0x222, 0x48));
 assert(!y2_pmic_value_allowed(0x222, 0x17) && !y2_pmic_value_allowed(0x222, 0x7f));
 return 0;
}
''')


class Placement(unittest.TestCase):
    def setUp(self):
        self.spm = (PLATFORM / 'spm.c').read_text()
        self.pwrap = (PLATFORM / 'pwrap.c').read_text()

    def test_armed_only_inside_enter_around_the_pcm_and_restored_before_normal_pcm(self):
        enter = self.spm[self.spm.index('static int y2_spm_enter('):self.spm.index('int y2_spm_dormant_idle(')]
        begin = enter.index('y2_pmic_vproc_sleep_begin();')
        self.assertLess(enter.index('y2_infra_pdn_save()'), begin)
        self.assertLess(begin, enter.index('y2_spm_suspend_arm(&spm_io, pcm_address, infra, Y2_SPM_TICKS(armed_seconds))'))
        end = enter.index('y2_pmic_vproc_sleep_end();')
        self.assertLess(enter.index('cpu_suspend(0, y2_spm_finish)'), end)
        self.assertLess(end, enter.index('restore = y2_spm_suspend_restore(&spm_io, normal_address);'))
        # Never in the runtime DORMANT path or any other platform callback.
        dormant = self.spm[self.spm.index('int y2_spm_dormant_idle('):]
        self.assertNotIn('vproc_sleep', dormant)
        self.assertEqual(self.spm.count('y2_pmic_vproc_sleep_begin'), 1)

    def test_owner_is_non_fatal_verified_and_switchable(self):
        begin = self.pwrap[self.pwrap.index('void y2_pmic_vproc_sleep_begin(void)'):
                           self.pwrap.index('void y2_pmic_vproc_sleep_end(void)')]
        self.assertNotIn('mutex_lock', begin)            # atomic ->enter() context
        self.assertIn('0x224, &actual', begin)            # live voltage verified after the switch
        self.assertIn('goto undo', begin)
        self.assertIn('module_param_named(vproc_sleep, vproc_sleep_enabled, bool, 0600)', self.pwrap)
        self.assertIn('module_param_cb(vproc_sleep_state', self.pwrap)
        self.assertRegex(self.pwrap, r'void y2_pmic_vproc_sleep_begin\(void\)')  # returns nothing: never aborts sleep


if __name__ == '__main__':
    unittest.main()
