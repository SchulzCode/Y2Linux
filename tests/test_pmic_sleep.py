"""POWER-FINAL-18: stock MT6323 suspend fields with exact restore.

Registers start at 17's real sleep-entry values (archive 112, boot de37f6bc,
pmic_golden): 0x102=0x8043, 0x108=0x03bf, 0x10e=0, 0x70c=0x1c32, 0x776=0,
0x778=0. Field names: ledger F15/F16.
"""
from pathlib import Path
import unittest

from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'


class PmicSleep(unittest.TestCase):
    def test_apply_reaches_stock_and_restore_is_exact(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "pmic-sleep-policy.h"
#include "pmic-golden-policy.h"
#include "policy.h"
#define G18 (Y2_PS_AUDIO | Y2_PS_CLOCKS | Y2_PS_VREF)   /* 18's groups; VSYS: next test */
struct reg { unsigned reg, value; };
static struct reg regs[] = { {0x102, 0x8043}, {0x108, 0x03bf}, {0x10e, 0}, {0x70c, 0x1c32}, {0x776, 0}, {0x778, 0} };
#define N (sizeof regs / sizeof regs[0])
static unsigned writes, fail_reg, stuck_reg;
static struct reg *find(unsigned r) { for (unsigned i = 0; i < N; i++) if (regs[i].reg == r) return &regs[i]; return 0; }
static int rd(void *c, unsigned r, unsigned *v) { (void)c; struct reg *p = find(r); assert(p); *v = p->value; return 0; }
static int upd(void *c, unsigned r, unsigned m, unsigned v) {
 (void)c; struct reg *p = find(r); assert(p);
 /* Every write must pass the production PMIC write policy. */
 assert(!(m & ~y2_pmic_write_mask(r)));
 assert(y2_pmic_value_allowed(r, (p->value & ~m) | (v & m)));
 if (r == fail_reg) return -ETIMEDOUT;
 writes++;
 if (r != stuck_reg) p->value = (p->value & ~m) | (v & m);
 return 0;
}
static int golden(unsigned r) {
 for (unsigned i = 0; i < Y2_PMIC_GOLDEN_ROWS; i++)
  if (y2_pmic_golden[i].reg == r) return (find(r)->value & y2_pmic_golden[i].mask) == y2_pmic_golden[i].golden;
 assert(0); return 0;
}
int main(void) {
 struct reg saved[N]; memcpy(saved, regs, sizeof regs);
 struct y2_ps_io io = { 0, rd, upd };
 struct y2_ps_state s;
 assert(!y2_ps_apply(&io, G18, &s));
 /* Every touched row now matches the stock suspend golden. */
 assert(golden(0x102) && golden(0x10e) && golden(0x776) && golden(0x778));
 /* 0x108: only accdet (bit 9) still differs; Linux gates it, stock does not. */
 assert(((find(0x108)->value ^ 0x15ff) & 0xffff) == 0x200);
 assert(!(find(0x70c)->value & 0x10));                       /* adacck off */
 assert(find(0x70c)->value == 0x1c22);                       /* nothing else in 0x70c */
 assert(!y2_ps_restore(&io, &s) && !s.changed);
 assert(!memcmp(saved, regs, sizeof regs));                  /* bit-exact */
 /* Group 1 only: audio clocks, nothing else. */
 assert(!y2_ps_apply(&io, Y2_PS_AUDIO, &s));
 assert(!(find(0x102)->value & 1) && !(find(0x70c)->value & 0x10) && find(0x108)->value == 0x03bf);
 assert(find(0x776)->value == 0);
 assert(!y2_ps_restore(&io, &s)); assert(!memcmp(saved, regs, sizeof regs));
 /* Fields already at stock are not written. */
 find(0x776)->value = 0x8000; find(0x778)->value = 0x8000; writes = 0;
 assert(!y2_ps_apply(&io, Y2_PS_VREF, &s) && !writes && !s.changed);
 find(0x776)->value = 0; find(0x778)->value = 0;
 /* A failed write undoes everything already written. */
 fail_reg = 0x10e;
 assert(y2_ps_apply(&io, G18, &s) == -ETIMEDOUT && !s.changed);
 assert(!memcmp(saved, regs, sizeof regs));
 /* A readback that does not follow is refused and undone. */
 fail_reg = 0; stuck_reg = 0x776;
 assert(y2_ps_apply(&io, G18, &s) == -EIO);
 assert(!memcmp(saved, regs, sizeof regs));
 /* Group 0: nothing written. */
 stuck_reg = 0; writes = 0;
 assert(!y2_ps_apply(&io, 0, &s) && !writes);
 /* VEMC_3V3 enable stays closed: the root eMMC is retained across suspend. */
 assert(!(y2_pmic_write_mask(0x508) & 0x4000));
 return 0;
}
''')

    def test_vsys_group_reaches_stock_without_touching_the_voltage(self):
        # Live 26 values (archive 20261014/124 live-reads): 0x238=0x0003,
        # 0x23c=0, 0x242=0x0505, 0x256=0x0001, selectors 0x244..0x24a = 0x40.
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "pmic-sleep-policy.h"
#include "pmic-golden-policy.h"
#include "policy.h"
struct reg { unsigned reg, value; };
static struct reg regs[] = { {0x102, 0x8042}, {0x108, 0x15ff}, {0x10e, 0x20}, {0x70c, 0x1c22}, {0x776, 0x8000}, {0x778, 0x8000},
 {0x238, 0x0003}, {0x23c, 0}, {0x242, 0x0505}, {0x256, 0x0001}, {0x244, 0x40}, {0x246, 0x40}, {0x248, 0x40}, {0x24a, 0x40} };
#define N (sizeof regs / sizeof regs[0])
static unsigned order[16], writes;
static struct reg *find(unsigned r) { for (unsigned i = 0; i < N; i++) if (regs[i].reg == r) return &regs[i]; return 0; }
static int rd(void *c, unsigned r, unsigned *v) { (void)c; struct reg *p = find(r); assert(p); *v = p->value; return 0; }
static int upd(void *c, unsigned r, unsigned m, unsigned v) {
 (void)c; struct reg *p = find(r); assert(p);
 assert(!(m & ~y2_pmic_write_mask(r)));
 assert(y2_pmic_value_allowed(r, (p->value & ~m) | (v & m)));
 order[writes++] = r; p->value = (p->value & ~m) | (v & m); return 0;
}
int main(void) {
 struct reg saved[N]; memcpy(saved, regs, sizeof regs);
 struct y2_ps_io io = { 0, rd, upd };
 struct y2_ps_state s;
 assert(y2_ps_vsys_ready(0, 0x40, 0x40, 0x40, 0x40));
 assert(!y2_ps_vsys_ready(2, 0x40, 0x40, 0x40, 0x40));      /* already hardware mode */
 assert(!y2_ps_vsys_ready(0, 0x40, 0x40, 0x30, 0x40));      /* would change voltage asleep */
 assert(!y2_ps_vsys_ready(0, 0x40, 0x48, 0x40, 0x40));      /* would change voltage awake */
 assert(!y2_ps_apply(&io, Y2_PS_VSYS, &s));
 /* Stock PMIC_INIT values, mode bit written last. */
 assert(find(0x242)->value == 0x9123 && find(0x256)->value == 0x0113);
 assert((find(0x238)->value & 0x33) == 0x23 && find(0x23c)->value == 2);
 assert(writes == 4 && order[3] == 0x23c);
 for (unsigned i = 0; i < Y2_PMIC_GOLDEN_ROWS; i++) if (y2_pmic_golden[i].reg == 0x256)
  assert((find(0x256)->value & y2_pmic_golden[i].mask) == y2_pmic_golden[i].golden);
 /* No selector moved. */
 assert(find(0x244)->value == 0x40 && find(0x246)->value == 0x40 && find(0x248)->value == 0x40);
 assert(!y2_ps_restore(&io, &s)); assert(!memcmp(saved, regs, sizeof regs));
 assert(order[4] == 0x23c);                                  /* software mode back first */
 /* Every selector stays unwritable. */
 assert(!y2_pmic_write_mask(0x244) && !y2_pmic_write_mask(0x246) && !y2_pmic_write_mask(0x248) && !y2_pmic_write_mask(0x24a));
 return 0;
}
''')

    def test_wiring_order_and_switch(self):
        spm = (PLATFORM / 'spm.c').read_text()
        enter = spm[spm.index('static int y2_spm_enter('):spm.index('int y2_spm_dormant_idle(')]
        # Apply after the VPROC step, capture after both, restore before VPROC.
        self.assertLess(enter.index('y2_pmic_vproc_sleep_begin();'), enter.index('y2_pmic_sleep_begin();'))
        self.assertLess(enter.index('y2_pmic_sleep_begin();'), enter.index('y2_pmic_golden_capture();'))
        self.assertLess(enter.index('y2_pmic_golden_capture();'), enter.index('y2_spm_suspend_arm('))
        self.assertLess(enter.index('y2_pmic_sleep_end();'), enter.index('y2_pmic_vproc_sleep_end();'))
        pwrap = (PLATFORM / 'pwrap.c').read_text()
        self.assertIn('module_param_named(pmic_sleep, pmic_sleep_groups, uint, 0644);', pwrap)
        self.assertIn('static unsigned pmic_sleep_groups;', pwrap)   # kernel default off
        begin = pwrap[pwrap.index('void y2_pmic_sleep_begin(void)'):pwrap.index('void y2_pmic_sleep_end(void)')]
        self.assertLess(begin.index('y2_ps_vsys_ready('), begin.index('y2_ps_apply('))
        self.assertIn('groups &= ~Y2_PS_VSYS;', begin)


if __name__ == '__main__':
    unittest.main()
