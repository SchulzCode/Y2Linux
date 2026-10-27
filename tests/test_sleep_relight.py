"""F47: a UART that never acknowledges the SPM clock-off request refused every sleep (report 66e5feea).
Synthetic, no hardware."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
SPM = (ROOT / 'kernel/platform/spm.c').read_text()
PINCTRL = (ROOT / 'kernel/platform/pinctrl.c').read_text()
DTS = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()


class UartBusy(unittest.TestCase):
    def test_busy_uart_is_dumped_like_stock_and_gets_its_sleep_enable_back(self):
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#define __iomem
#define BIT(n) (1U << (n))
#define dsb(x) ((void)0)
#define SPM_PCM_REG13_DATA 1
#define SPM_PWR_STATUS 2
#define SPM_PWR_STATUS_S 3
#define R13_UART_CLK_OFF_ACK (1U << 20)
#define Y2_SPM_SECONDARY_CPU_MASK 0x00000e00U
#define Y2_UART_SLEEP_EN 0x48U
static unsigned spm_regs[4], pdn, uart[0x80 / 4], gpio_mode, reads;
static int pdn_status;
static void *spm_base = spm_regs;
static void __iomem *uart0_regs = uart, *uart0_rx_mode = &gpio_mode;
static unsigned spm_read(void *b, unsigned r) { return ((unsigned *)b)[r]; }
static unsigned y2_dcm_read(void *c, unsigned phys) { assert(phys == 0x10003018); *(int *)c = pdn_status; return pdn; }
static unsigned readl(const void *a) { reads++; return *(const unsigned *)a; }
static void writel(unsigned v, void *a) { *(unsigned *)a = v; }
static unsigned uart_busy_refusals, uart_busy_recoveries, uart_busy_pdn, uart_busy_rx_mode;
static unsigned uart_busy_lcr, uart_busy_ier, uart_busy_lsr, uart_busy_dma, uart_busy_sleep;
''' + function(SPM, 'y2_spm_uart_busy') + r'''
int main(void) {
 (void)uart_busy_recoveries;
 spm_regs[SPM_PCM_REG13_DATA] = R13_UART_CLK_OFF_ACK;       /* ACK came: not a UART refusal */
 assert(!y2_spm_uart_busy() && !uart_busy_refusals);
 spm_regs[SPM_PCM_REG13_DATA] = 0x8040000;                  /* 66e5feea: bit 20 missing */
 spm_regs[SPM_PWR_STATUS] = 0x200;                          /* a secondary CPU still on */
 assert(!y2_spm_uart_busy() && !uart_busy_refusals);
 spm_regs[SPM_PWR_STATUS] = 0;
 pdn = 0x02feffff; gpio_mode = 1U << 9;                     /* UART0 clocked, RX pad = URXD1 */
 uart[0x0c / 4] = 0x03; uart[0x04 / 4] = 0; uart[0x14 / 4] = 0x61; uart[0x4c / 4] = 0; uart[0x48 / 4] = 0;
 assert(y2_spm_uart_busy() && uart_busy_refusals == 1);
 assert(uart_busy_pdn == 0x02feffff && uart_busy_rx_mode == 1 && uart_busy_lcr == 3 && uart_busy_lsr == 0x61);
 assert(uart_busy_sleep == 0 && uart[0x48 / 4] == 1);       /* sleep enable set again for the retry */
 uart[0x0c / 4] = 0x83;                                     /* divisor bank: IER/LSR alias, not read */
 assert(y2_spm_uart_busy() && uart_busy_ier == ~0U && uart_busy_lsr == ~0U && uart_busy_sleep == 1);
 pdn |= BIT(16); reads = 0; uart[0x48 / 4] = 0;             /* UART0 gated: never read unclocked */
 assert(y2_spm_uart_busy() && reads == 1 && uart_busy_lcr == ~0U && uart[0x48 / 4] == 0);
 pdn_status = -6; pdn = 0;                                  /* PERI unmapped: same */
 assert(y2_spm_uart_busy() && uart_busy_pdn == ~0U && uart_busy_lcr == ~0U);
 assert(uart_busy_refusals == 4);
 return 0;
}
''')

    def test_one_retry_only_after_a_uart_refusal_then_refuse_as_stock(self):
        enter = function(SPM, 'y2_spm_enter')
        arm = enter.index('ret = y2_spm_suspend_arm(&spm_io, pcm_address, infra);')
        retry = enter.index('if (ret == -EBUSY && y2_spm_uart_busy()) {')
        self.assertLess(arm, retry)
        self.assertEqual(enter.count('y2_spm_suspend_arm('), 2)
        self.assertLess(retry, enter.index('y2_spm_journal_snapshot();'))
        self.assertIn('pr_warn("Y2SPM: UART busy', enter)
        # The write side stays out of the read-only block before y2_spm_enter.
        self.assertGreater(SPM.index('static bool y2_spm_uart_busy(void)\n{'), SPM.index('static int y2_spm_enter('))
        self.assertIn('suspend_uart_busy=%u suspend_uart_recoveries=%u', SPM)


class RxPadSleepState(unittest.TestCase):
    def test_uart0_rx_pad_goes_to_gpio_in_system_sleep_like_stock(self):
        # Stock mtk_uart_suspend (0xc038af20) -> mtk_uart_switch_rx_to_gpio: GPIO108 mode 0;
        # mtk_uart_switch_to_rx: mode 1. Mainline 8250_mtk selects "sleep"/"default" itself.
        self.assertIn('static const unsigned uart0_rx_pins[] = {108};', PINCTRL)
        names = re.search(r'group_names\[\] = \{(.*?)\};', PINCTRL, re.S)[1]
        self.assertEqual([n.strip(' "\n\t') for n in names.split(',')][8], 'uart0-rx')
        functions = re.search(r'function_names\[\] = \{(.*?)\};', PINCTRL, re.S)[1]
        functions = [n.strip(' "\n\t') for n in functions.split(',')]
        self.assertEqual(functions[7:9], ['uart0', 'uart0-sleep'])
        self.assertIn('pin_mode(p, pin, f >= 2 && f != Y2_UART0_RX_SLEEP ? 1 : 0);', PINCTRL)
        self.assertIn('if (f < 2 || f == Y2_UART0_RX_SLEEP)\n\t\t\tpin_bit(p, 0, pin, false);', PINCTRL)
        uart = DTS[DTS.index('uart0: serial@11002000 {'):]
        uart = uart[:uart.index('};')]
        self.assertIn('pinctrl-names = "default", "sleep";', uart)
        self.assertIn('pinctrl-0 = <&uart0_state>; pinctrl-1 = <&uart0_sleep_state>;', uart)
        self.assertIn('uart0_sleep_state: uart0-sleep-state { groups = "uart0-rx"; function = "uart0-sleep"; };', DTS)
        mtk = (ROOT / '.cache/sources/linux-6.18/drivers/tty/serial/8250/8250_mtk.c')
        if mtk.exists():
            suspend = function(mtk.read_text(), 'mtk8250_suspend')
            self.assertIn('pinctrl_pm_select_sleep_state(dev);', suspend)


if __name__ == '__main__':
    unittest.main()
