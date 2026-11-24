"""kernel/platform/connectivity/stp-psm.c: STP power saving (power plan 2026-11, phase 7).

Source-level checks of the orders that keep the BTIF clock-gating safe; behaviour is checked on the
device with stp_psm.enable=Y (counters in /sys/devices/platform/18070000.connectivity/psm).
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONN = ROOT / 'kernel/platform/connectivity'
PSM = (CONN / 'stp-psm.c').read_text()


def function(source, name):
    match = re.search(r'\n[^\n]*\b%s\([^;{]*\)\n\{' % re.escape(name), source)
    assert match, name
    depth, i = 0, source.index('{', match.start())
    for j in range(i, len(source)):
        depth += {'{': 1, '}': -1}.get(source[j], 0)
        if depth == 0:
            return source[match.start():j + 1]
    raise AssertionError(name)


class StpPsm(unittest.TestCase):
    def test_off_by_default_and_bounded(self):
        self.assertIn('static bool enable;', PSM)
        self.assertIn('static unsigned idle_ms = 5000;', PSM)
        self.assertIn('ms < 100 || ms > 20000', PSM)
        self.assertIn('stp-psm.o', (CONN / 'Makefile').read_text())

    def test_stock_wire_messages(self):
        self.assertIn('sleep_cmd[] = {1, 3, 1, 0, 1}', PSM)
        self.assertIn('sleep_evt[] = {2, 3, 2, 0, 0, 1}', PSM)
        self.assertIn('wakeup_evt[] = {2, 3, 2, 0, 0, 3}', PSM)

    def test_sleep_gates_only_after_the_event_and_with_the_wake_line_armed(self):
        idle = function(PSM, 'idle_work')
        handshake = idle.index('y2_conn_wmt_awake(c, sleep_cmd')
        failed = idle.index('if (ret) {', handshake)
        self.assertLess(failed, idle.index('gate(c);'))
        self.assertLess(idle.index('wake_arm(c);'), idle.index('gate(c);'))
        self.assertIn('sleep_refusal(c)', idle)

    def test_gate_stops_the_data_irqs_before_checking_quiet(self):
        gate = function(PSM, 'gate')
        self.assertLess(gate.index('data_irqs(c, false);'), gate.index('mutex_lock(&c->dma_tx);'))
        self.assertLess(gate.index('quiet(c)'), gate.index('clk_bulk_disable('))

    def test_wake_ungates_before_the_btif_wak_pulse(self):
        wake = function(PSM, 'host_wake')
        self.assertLess(wake.index('ungate(c);'), wake.index('writel(0, c->btif + BTIF_WAK);'))
        self.assertLess(wake.index('writel(0, c->btif + BTIF_WAK);'), wake.index('usleep_range(64, 96);'))
        self.assertLess(wake.index('usleep_range(64, 96);'), wake.index('writel(1, c->btif + BTIF_WAK);'))

    def test_transport_stop_restores_clocks_before_btif_stop(self):
        core = (CONN / 'core.c').read_text()
        power_off = function(core, 'power_off')
        self.assertLess(power_off.index('y2_psm_stop(c);'), power_off.index('y2_btif_stop(c)'))
        self.assertIn('{ y2_psm_start(c); return 0; }', core)

    def test_every_sender_holds_the_chip_awake(self):
        wmt = (CONN / 'wmt.c').read_text()
        self.assertIn('y2_psm_get(c)', function(wmt, 'y2_conn_wmt'))
        self.assertIn('y2_psm_get(c)', function((CONN / 'hci.c').read_text(), 'tx_work'))
        self.assertIn('y2_psm_get(fm->c)', function((CONN / 'fm.c').read_text(), 'fm_cmd'))
        send = function((CONN / 'btif.c').read_text(), 'y2_btif_send')
        self.assertIn('psm_send_while_gated++', send)

    def test_unexpected_power_saving_events_are_dropped_not_fatal(self):
        stp = function((CONN / 'stp.c').read_text(), 'deliver')
        self.assertLess(stp.index('psm_unsolicited++'), stp.index('completion_done(&c->response)'))


if __name__ == '__main__':
    unittest.main()
