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
        self.assertIn('y2_charger_rgu_reset(c);', handler)
        self.assertTrue(handler.rstrip().endswith('return NOTIFY_DONE;\n}') or 'return NOTIFY_DONE;\n}' in handler)
        reset = function(source, 'y2_charger_rgu_reset')
        order = [reset.index(s) for s in ('Y2_RGU_RESTART_KEY', 'y2_rgu_charger_mode(readl', 'mdelay(3)',
                                           'Y2_RGU_SWRST_KEY', 'mdelay(1000)')]
        self.assertEqual(order, sorted(order))
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


class PowerOffNeverReturns(unittest.TestCase):
    """F50: a BBPU power-off that returned let init exit into a panic=0 hang."""

    def test_last_handler_waits_for_power_release_then_resets_and_never_returns(self):
        source = (ROOT / 'kernel/platform/mt6323-charger.c').read_text()
        run_c(r'''
#include <assert.h>
#include <setjmp.h>
#include <stdbool.h>
#include "poweroff-policy.h"
#define NOTIFY_DONE 0
struct device; struct sys_off_data { void *cb_data; };
struct y2_charger { struct device *dev; void *map; };
static jmp_buf out;
static unsigned now, con0_at, held_until, resets, resets_to_fail, bbpu_writes, trigger_writes, logs;
static int fail_reads;
static int regmap_read(void *map, unsigned reg, unsigned *value)
{
 (void)map;
 if (fail_reads) return -5;
 if (reg == 0) *value = now >= con0_at ? 0x20 : 0;
 else if (reg == Y2_PMIC_CHRSTATUS) *value = now < held_until ? 0x00 : Y2_CHRSTATUS_PWRKEY_DEB;
 else if (reg == Y2_RTC_BBPU) *value = 0x4300;
 else assert(0);
 return 0;
}
static int regmap_write(void *map, unsigned reg, unsigned value)
{
 (void)map;
 if (reg == Y2_RTC_BBPU) { assert(value == Y2_RTC_BBPU_POWER_DOWN); bbpu_writes++; }
 else if (reg == Y2_RTC_WRTGR) { assert(value == 1); trigger_writes++; }
 else assert(0);
 return 0;
}
static void mdelay(unsigned ms) { assert(ms == 1000); now++; }
#define dev_emerg(dev, ...) ((void)(dev), logs++)
static void y2_charger_rgu_reset(struct y2_charger *c)
{
 (void)c;
 if (++resets > resets_to_fail) longjmp(out, 1);
 now++;
}
''' + function(source, 'y2_charger_power_off_last') + r'''
static unsigned run(unsigned charger_at, unsigned held, unsigned fail)
{
 struct y2_charger c = {0}; struct sys_off_data d = { &c };
 now = 0; con0_at = charger_at; held_until = held; resets = 0; resets_to_fail = fail;
 bbpu_writes = trigger_writes = logs = 0;
 if (!setjmp(out)) { y2_charger_power_off_last(&d); assert(!"returned"); }
 return now;
}
int main(void)
{
 /* Charger present: the stock charger reset at once. */
 assert(run(0, 0, 0) == 0 && bbpu_writes == 0 && logs == 2);
 /* Battery, Power released: one more power-down, then the reset after 1 s. */
 assert(run(~0U, 0, 0) == 1 && bbpu_writes == 1 && trigger_writes == 1);
 /* Power held: the power-down stays asserted, reset 2 s after the release. */
 assert(run(~0U, 6, 0) == 7 && bbpu_writes == 7);
 /* Held past the bound: reset anyway (LK may then boot; never a hang). */
 assert(run(~0U, 100, 0) == Y2_POWEROFF_LAST_SECONDS);
 /* A charger plugged in while waiting: reset at once. */
 assert(run(3, 100, 0) == 3);
 /* Failed resets are retried each second and never returned from; told once. */
 assert(run(~0U, 0, 3) == 1 + 3 * 2 && logs == 2);
 /* Unreadable PMIC: treated as released, no charger. */
 fail_reads = 1; assert(run(0, 100, 0) == 1); fail_reads = 0;
 return 0;
}
''')
        mapping = function(source, 'y2_charger_map_rgu')
        # After mt6323-pwrc (SYS_OFF_PRIO_DEFAULT): the last handler in the chain.
        self.assertRegex(mapping, r'SYS_OFF_MODE_POWER_OFF, SYS_OFF_PRIO_LOW,\s+y2_charger_power_off_last, c\)')

    def test_watchdog_reset_of_a_power_off_flips_only_auto_restart(self):
        run_c(r'''
#include <assert.h>
#include "poweroff-policy.h"
int main(void){
 /* Runtime mode after our restart: EN|EXRST|AUTO_RESTART (0x15). */
 assert(y2_rgu_poweroff_mode(0x15, 1) == 0x22000005);
 assert(y2_rgu_poweroff_mode(0x05, 0) == 0x22000015);
 for (unsigned m = 0; m < 0x10000; m++) {
  assert(y2_rgu_poweroff_mode(m, 1) == (0x22000000 | (m & ~0x10U)));
  assert(y2_rgu_poweroff_mode(m | 0x22000000, 0) == (0x22000000 | m | 0x10));
 }
}
''')
        source = (ROOT / 'kernel/platform/mt6323-charger.c').read_text()
        self.assertIn('&dev_attr_watchdog_powers_off.attr', source)
        store = function(source, 'watchdog_powers_off_store')
        self.assertIn('writel(y2_rgu_poweroff_mode(readl(c->rgu + Y2_RGU_MODE), value), c->rgu + Y2_RGU_MODE);', store)


if __name__ == '__main__':
    unittest.main()
