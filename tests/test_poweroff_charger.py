"""Power off with a charger attached restarts into LK's charging mode (F46)."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class PowerOffOnCharger(unittest.TestCase):
    def test_reset_mode_is_stock_charger_reset(self):
        run_c(r'''
#include <assert.h>
#include "poweroff-policy.h"
int main(void){
 assert(!y2_poweroff_to_charger(0)); assert(!y2_poweroff_to_charger(0xffdf));
 assert(y2_poweroff_to_charger(0x20)); assert(y2_poweroff_to_charger(0x0035));
 /* Our restart (patch 0037) leaves EN|EXRST|AUTO_START (0x15) set. The
  * charger reset clears AUTO_RESTART (boot reason 3, not 4), EN, IRQ and DUAL,
  * keeps the external PMIC reset and the other bits, and carries the key. */
 assert(y2_rgu_charger_mode(0x15) == 0x22000004);
 assert(y2_rgu_charger_mode(0x5f) == 0x22000006);
 assert(y2_rgu_charger_mode(0x104) == 0x22000104);
 assert(y2_rgu_charger_mode(0) == 0x22000004);
 for (unsigned m = 0; m < 0x400; m++) {
  unsigned v = y2_rgu_charger_mode(m);
  assert(!(v & 0x10) && !(v & 0x59) && (v & 0x04) && (v >> 24) == 0x22);
 }
}
''')

    def test_driver_checks_chrdet_then_resets_before_bbpu(self):
        source = (ROOT / 'kernel/platform/mt6323-charger.c').read_text()
        handler = function(source, 'y2_charger_power_off')
        # CHRDET first; without a charger (or a failed read) the BBPU power-off follows.
        self.assertRegex(handler, r'if \(regmap_read\(c->map, 0, &con0\) \|\| !y2_poweroff_to_charger\(con0\)\)\s*return NOTIFY_DONE;')
        order = [handler.index(s) for s in ('Y2_RGU_RESTART_KEY', 'y2_rgu_charger_mode(readl', 'mdelay(3)',
                                             'Y2_RGU_SWRST_KEY', 'mdelay(1000)')]
        self.assertEqual(order, sorted(order))
        self.assertTrue(handler.rstrip().endswith('return NOTIFY_DONE;\n}') or 'return NOTIFY_DONE;\n}' in handler)
        mapping = function(source, 'y2_charger_map_rgu')
        # Runs before mt6323-pwrc (SYS_OFF_PRIO_DEFAULT); no region request.
        self.assertIn('SYS_OFF_MODE_POWER_OFF, SYS_OFF_PRIO_HIGH', mapping)
        self.assertIn('devm_ioremap(', mapping)
        self.assertNotIn('devm_ioremap_resource', mapping)
        probe = function(source, 'y2_charger_probe')
        self.assertIn('y2_charger_map_rgu(c);', probe)
        dts = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()
        charger = dts[dts.index('charger { compatible = "innioasis,y2-mt6323-charger";'):]
        self.assertIn('innioasis,rgu = <&rgu>;', charger[:charger.index('};')])
        self.assertRegex(dts, r'rgu: watchdog@10007000 \{')


if __name__ == '__main__':
    unittest.main()
