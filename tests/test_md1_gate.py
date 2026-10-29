"""POWER-FINAL-17: gate the never-used modem's (MD1) peripheral clock requests.

Register layout from the owner-authorised live test on 16 (ledger F11):
MD1_SET 0x20 / MD1_CLR 0x28 write-one, STA 0x30, 26 request bits.
"""
from pathlib import Path
import unittest

from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class Md1Gate(unittest.TestCase):
    def test_gate_release_and_regate_at_entry(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "md1-gate-policy.h"
static unsigned sta, writes, stuck;
static unsigned rd(void *c, unsigned phys) { (void)c; assert(phys == Y2_MD1_STA); return sta; }
static void wr(void *c, unsigned phys, unsigned v) {
 (void)c; writes++;
 if (stuck) return;
 if (phys == Y2_MD1_SET) sta |= v; else if (phys == Y2_MD1_CLR) sta &= ~v; else assert(0);
}
int main(void) {
 int status = 0; unsigned got;
 struct y2_dcm_io io = { &status, rd, wr };
 /* 16's real state: no request gated. */
 assert(y2_md1_gate(&io, 1, &got) == 0 && sta == Y2_MD1_ALL && got == Y2_MD1_ALL);
 /* Already gated at entry: read only. */
 writes = 0; assert(y2_md1_regate(&io, &got) == 0 && writes == 0);
 /* Lost (INFRA really powered down): re-gated once. */
 sta = 0; assert(y2_md1_regate(&io, &got) == 1 && sta == Y2_MD1_ALL && writes == 1);
 /* Owner A/B switch releases every request. */
 assert(y2_md1_gate(&io, 0, &got) == 0 && sta == 0);
 /* A readback that does not follow is an error, never silently accepted. */
 stuck = 1; assert(y2_md1_gate(&io, 1, &got) == -EIO);
 stuck = 0; status = -ENXIO; assert(y2_md1_gate(&io, 1, &got) == -ENXIO);
 return 0;
}
''')

    def test_spm_wiring(self):
        spm = (ROOT / 'kernel/platform/spm.c').read_text()
        self.assertIn('#include "md1-gate-policy.h"', spm)
        self.assertIn('module_param_cb(md1_gate, &md1_gate_ops, &md1_gate, 0644);', spm)
        # Kernel default off; userspace arms it like dis_pdn.
        self.assertIn('static bool md1_gate;', spm)
        # Re-checked at sleep entry before the golden capture records it.
        enter = spm[spm.index('static int y2_spm_enter('):]
        self.assertLess(enter.index('y2_golden_entry_apply();'), enter.index('y2_golden_capture();'))
        apply = spm[spm.index('static void y2_golden_entry_apply(void)'):]
        self.assertLess(apply.index('y2_md1_regate'), apply.index('if (!groups'))
        self.assertIn('md1_gate=%u md1_result=%d md1_sta=%#x md1_regates=%u', spm)


if __name__ == '__main__':
    unittest.main()
