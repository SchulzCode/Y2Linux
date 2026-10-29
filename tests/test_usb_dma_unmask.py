"""POWER-FINAL-16: Defect B, MUSB DMA completion unmask re-armed after every wake.

13/14/15 (archives 104, 106, 108, 109): after each SPM sleep the gadget
configured and RX (PIO) worked, while transmit DMA was programmed and its
completion interrupt never arrived (TX frozen, dma_aborts +6 per disconnect).
The DMA unmask (MAC+0x200 bits 31..24) was written once, at DMA creation; the
reconnect and restore paths re-opened L1INTM but never the DMA unmask, and the
diagnostics only sampled the low status byte.
"""
from pathlib import Path
import unittest

from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
USB = ROOT / 'kernel/platform/usb.c'
HEADER = ROOT / 'kernel/usb/dma-unmask.h'


class UsbDmaUnmask(unittest.TestCase):
    def test_policy(self):
        run_c(r'''
#include <assert.h>
''' + HEADER.read_text() + r'''
int main(void) {
    /* The re-arm writes only the set field: no status bit, no clear field. */
    assert(Y2_USB_DMA_INTR == 0x200);
    assert((Y2_USB_DMA_UNMASK_SET & 0x00ffffffU) == 0);
    /* Real 16 readback after the stock init write: unmask in bits 15..8. */
    assert(!y2_usb_dma_unmask_lost(0x0000ff00U));
    assert(!y2_usb_dma_unmask_lost(0x0000ff03U));
    /* Any missing channel unmask counts as lost. */
    assert(y2_usb_dma_unmask_lost(0));
    assert(y2_usb_dma_unmask_lost(0x0000fe00U));
    assert(y2_usb_dma_unmask_lost(0x00ff00ffU));
    return 0;
}
''')

    def test_rearm_runs_where_the_gate_opens(self):
        source = USB.read_text()
        rearm = function(source, 'y2_musb_dma_rearm')
        self.assertIn('writel(Y2_USB_DMA_UNMASK_SET,intr)', rearm)
        self.assertIn('if(!musb->dma_controller) return;', rearm)
        self.assertLess(rearm.index('before=readl(intr)'), rearm.index('writel(Y2_USB_DMA_UNMASK_SET'))
        enable = function(source, 'y2_musb_enable')
        self.assertLess(enable.index('y2_musb_dma_rearm(musb)'), enable.index('musb->mregs+0xa4'))
        after = source[source.index('void y2_musb_after_restore'):]
        after = after[:after.index('\n}\n')]
        self.assertLess(after.index('y2_musb_dma_rearm(musb)'), after.index('musb->mregs + 0xa4'))
        # A latched terminal fault is never revived by a re-arm.
        self.assertIn('if(!READ_ONCE(y2_live.result)) y2_musb_dma_rearm(musb);', enable)
        self.assertIn('if (!READ_ONCE(y2_live.result)) y2_musb_dma_rearm(musb);', after)

    def test_diagnostics_read_all_32_bits(self):
        source = USB.read_text()
        self.assertIn('f.dma_intr=readl(b+0x200);', source)
        self.assertNotIn('readb(b+0x200)', source)
        self.assertIn('dma_unmask_rearms=%u dma_unmask_lost=%u dma_intr_before=%08x dma_intr_after=%08x', source)


if __name__ == '__main__':
    unittest.main()
