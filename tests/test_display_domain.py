"""POWER-FINAL-16 G1: the DIS (display/MMSYS) power domain.

Stock (REA, FM kernel spm_mtcmos_ctrl_disp 0xc00350a8) powers DIS off at every
screen-off; Y2 15 never did and slept with DIS on (PWR_STATUS 0x314c).
"""
from pathlib import Path
import unittest

from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'


class DisplayDomain(unittest.TestCase):
    def test_exact_stock_sequence(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "disp-policy.h"
struct hardware { unsigned r[1024], log[64], n; int stuck; };
static unsigned read_reg(void *ctx, unsigned reg) { return ((struct hardware *)ctx)->r[reg / 4]; }
static void write_reg(void *ctx, unsigned reg, unsigned value) {
 struct hardware *h = ctx;
 assert(reg == 0 || reg == Y2_DIS_PWR_CON); /* never another domain */
 if (reg == 0) { assert(value == 0x0b160001); return; }
 assert(((h->r[reg / 4] ^ value) & ~0xf1fU) == 0); /* unrelated bits kept */
 h->r[reg / 4] = value; h->log[h->n++] = value;
 if (!h->stuck) {
  unsigned on = (value & 12) == 12 ? 8 : 0;
  h->r[0x60c / 4] = (h->r[0x60c / 4] & ~8U) | on;
  h->r[0x610 / 4] = (h->r[0x610 / 4] & ~8U) | on;
 }
}
static void delay(unsigned us) { (void)us; }
int main(void) {
 struct hardware h = {0};
 struct y2_spm_io io = {&h, read_reg, write_reg, delay};
 /* Loader-on DIS: PWR_ON|PWR_ON_S|RST_B, SRAM on; status 0x314c. */
 h.r[Y2_DIS_PWR_CON / 4] = 0xd; h.r[0x60c / 4] = h.r[0x610 / 4] = 0x314c;
 assert(!y2_dis_power_down(&io));
 /* Stock order (assembly): SRAM_PDN, ISO, RST_B=0+CLK_DIS, PWR_ON(_S)=0. */
 assert(h.n == 4 && h.log[0] == 0xf0d && h.log[1] == 0xf0f && h.log[2] == 0xf1e && h.log[3] == 0xf12);
 assert(h.r[0x60c / 4] == 0x3144 && h.r[0x610 / 4] == 0x3144);
 h.n = 0;
 assert(!y2_dis_power_up(&io));
 /* PWR_ON, PWR_ON_S, then CLK_DIS=0, ISO=0, RST_B=1, SRAM on. */
 assert(h.n == 6 && h.log[0] == 0xf16 && h.log[1] == 0xf1e && h.log[2] == 0xf0e &&
        h.log[3] == 0xf0c && h.log[4] == 0xf0d && h.log[5] == 0x00d);
 assert(h.r[0x60c / 4] == 0x314c);
 /* A stuck power-up status never releases isolation, reset or SRAM. */
 h.r[Y2_DIS_PWR_CON / 4] = 0xf12; h.r[0x60c / 4] = h.r[0x610 / 4] = 0x3144; h.stuck = 1; h.n = 0;
 assert(y2_dis_power_up(&io) == -ETIMEDOUT && h.n == 2 && h.r[Y2_DIS_PWR_CON / 4] == 0xf1e);
 /* A stuck power-down status is reported. */
 h.r[Y2_DIS_PWR_CON / 4] = 0xd; h.r[0x60c / 4] = h.r[0x610 / 4] = 0x314c;
 assert(y2_dis_power_down(&io) == -ETIMEDOUT);
 return 0;
}
''')

    def test_domain_wiring(self):
        spm = (PLATFORM / 'spm.c').read_text()
        # Off by default: armed only by userspace after the first frame.
        self.assertIn('static bool dis_pdn;', spm)
        self.assertIn('module_param(dis_pdn, bool, 0644);', spm)
        off = spm[spm.index('static int y2_dis_power_off'):spm.index('static int y2_dis_power_on')]
        # Readers are fenced before bus protection, protection before power-down.
        self.assertLess(off.index('dis_accessible = false'), off.index('y2_ccf_disp_protect(true)'))
        self.assertLess(off.index('y2_ccf_disp_protect(true)'), off.index('y2_dis_power_down'))
        on = spm[spm.index('static int y2_dis_power_on'):spm.index('static int y2_dis_domain_register')]
        self.assertLess(on.index('y2_dis_power_up'), on.index('y2_ccf_disp_protect(false)'))
        self.assertLess(on.index('y2_ccf_disp_protect(false)'), on.index('dis_accessible = true'))
        self.assertIn('pm_genpd_add_subdomain(&y2_dis_domain, &y2_mfg_domain)', spm)
        self.assertIn('of_genpd_add_provider_onecell', spm)
        clocks = (PLATFORM / 'clocks.c').read_text()
        protect = clocks[clocks.index('int y2_ccf_disp_protect'):clocks.index('int y2_ccf_md_unconfigured')]
        self.assertIn('readl_poll_timeout_atomic(infra + 0x228', protect)
        mm = (PLATFORM / 'mm-clocks.c').read_text()
        self.assertIn('.need_runtime_pm = true', mm)
        blockers = mm[mm.index('int y2_mm_idle_blockers'):mm.index('static bool y2_mm_unused_quiet')]
        self.assertLess(blockers.index('y2_spm_disp_access_begin'), blockers.index('readl('))
        dts = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()
        self.assertIn('#power-domain-cells = <1>;', dts)
        self.assertIn('power-domains = <&spm 0>;', dts)
        self.assertEqual(dts.count('power-domains = <&spm 1>'), 2)

    def test_a_restart_leaves_the_smi_gates_open_for_lk(self):
        # Beta 8.4 (POWER-FINAL-42 back): a watchdog restart keeps
        # MMSYS_CG_CON0; the last restart handler reopens SMI common/larb0
        # while DIS is powered, and the hold of 43 still covers Lima.
        mm = (PLATFORM / 'mm-clocks.c').read_text()
        handler = mm[mm.index('static int y2_mm_restart'):mm.index('static struct notifier_block y2_mm_restart_nb')]
        self.assertIn('.priority = 255, /* before the watchdog restart handler (128) */', mm)
        probe = mm[mm.index('static int y2_mm_probe'):]
        self.assertLess(probe.index('y2_mm_take_loader_hold(&pdev->dev);'),
                        probe.index('register_restart_handler(&y2_mm_restart_nb)'))
        hold = mm[mm.index('static void y2_mm_take_loader_hold'):mm.index('static int y2_mm_restart')]
        self.assertIn('SMI common gated before Linux touched it', hold)
        self.assertLess(hold.index('if (!__clk_is_enabled(smi))'), hold.index('loader_hold = true'))
        # 2026-10-20 report: after mtk_clk_pdev_probe()'s async pm_runtime_put()
        # the clock core called the open gate disabled; check with DIS held.
        wrapper = function(mm, 'y2_mm_take_loader_hold')
        self.assertLess(wrapper.index('pm_runtime_resume_and_get(mm_dev)'),
                        wrapper.index('y2_mm_take_loader_hold_active(dev);'))
        self.assertLess(wrapper.index('y2_mm_take_loader_hold_active(dev);'), wrapper.index('pm_runtime_put(mm_dev)'))
        run_c(r'''
#include <assert.h>
#define __iomem
#define NOTIFY_DONE 0
struct notifier_block;
static unsigned registers[0x12000 / 4], writes;
static void *mm_idle_base = registers;
static int powered;
static int y2_spm_disp_status(void) { return powered; }
static void writel(unsigned value, void *address) {
 assert((char *)address == (char *)registers + 0x108); /* MMSYS_CG_CLR0 only */
 assert(value == 0x3); writes++;
}
''' + handler + r'''
int main(void) {
 powered = 0; assert(y2_mm_restart(0, 0, 0) == NOTIFY_DONE && writes == 0); /* off: comes back open */
 powered = -19; y2_mm_restart(0, 0, 0); assert(writes == 0);
 powered = 1; y2_mm_restart(0, 0, 0); assert(writes == 1);
 mm_idle_base = 0; y2_mm_restart(0, 0, 0); assert(writes == 1);
 return 0;
}
''')


if __name__ == '__main__':
    unittest.main()
