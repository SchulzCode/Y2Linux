"""POWER-FINAL-19: the AP's PMIC_WRAP clock (INFRA_PDN bit 23) gated for the PCM sleep only.

Live test 2026-10-13 (ledger F17): with bit 23 set the AP-side wrapper reads as 0
and every WACS2 transfer fails, so the gate must follow the last AP PMIC access
and the ungate must precede the first restore.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PwrapGate(unittest.TestCase):
    def setUp(self):
        self.spm = (ROOT / 'kernel/platform/spm.c').read_text()
        self.enter = self.spm[self.spm.index('static int y2_spm_enter('):self.spm.index('int y2_spm_dormant_idle(')]

    def test_gate_follows_every_pmic_access_and_precedes_the_kick(self):
        e = self.enter
        gate = e.index('y2_pwrap_clock_gate();')
        for before in ('y2_pmic_vproc_sleep_begin();', 'y2_pmic_sleep_begin();', 'y2_pmic_golden_capture();'):
            self.assertLess(e.index(before), gate, before)
        self.assertLess(gate, e.index('y2_spm_suspend_arm('))
        self.assertEqual(e.count('y2_pwrap_clock_gate();'), 1)

    def test_ungate_is_first_after_the_cpu_returns_and_precedes_every_restore(self):
        e = self.enter
        after = e[e.index('cpu_suspend(0, y2_spm_finish);'):]
        # Right after the backstop re-arm (pinned first by test_cpu_fix02), before any restore.
        self.assertEqual(after.split('\n')[1].strip(), 'y2_pm_backstop_pause(false);')
        self.assertTrue(after.split('\n')[2].strip().startswith('y2_pwrap_clock_ungate();'))
        self.assertLess(after.index('y2_pwrap_clock_ungate();'), after.index('y2_infra_pdn_restore()'))
        # The abort path (arm failure) also ungates before the PMIC restores.
        tail = e[e.rindex('y2_pwrap_clock_ungate();'):]
        self.assertLess(tail.index('y2_pwrap_clock_ungate();'), tail.index('y2_pmic_sleep_end();'))
        self.assertLess(tail.index('y2_pmic_sleep_end();'), tail.index('y2_pmic_vproc_sleep_end();'))

    def test_ungate_retries_and_never_forgets_a_gate(self):
        body = self.spm[self.spm.index('static void y2_pwrap_clock_ungate(void)\n{'):]
        body = body[:body.index('\n}\n')]
        self.assertIn('Y2_INFRA_PDN_CLR', body)
        self.assertIn('for (n = 0; n < 3; n++)', body)
        self.assertIn('return; /* keep pwrap_gated', body)
        gate = self.spm[self.spm.index('static void y2_pwrap_clock_gate(void)\n{'):]
        gate = gate[:gate.index('\n}\n')]
        # Never take ownership of a gate somebody else set; flag before readback.
        self.assertIn('(sta & Y2_INFRA_PMIC_WRAP)) return;', gate)
        self.assertLess(gate.index('pwrap_gated = true;'), gate.index('pwrap_gate_sta = '))
        self.assertIn('#define Y2_INFRA_PMIC_WRAP (1U << 23)', self.spm)
        self.assertIn('static bool pwrap_gate;', self.spm)   # kernel default off


if __name__ == '__main__':
    unittest.main()
