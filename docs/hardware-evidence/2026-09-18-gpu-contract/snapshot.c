// SPDX-License-Identifier: GPL-2.0-only
/* Read-only observation of always-on clock registers. No GPU mapping/writes,
 * power transitions, resource claims or persistent driver registration. */
#include <linux/module.h>
#include <linux/io.h>
static int __init snapshot_init(void)
{
 void __iomem *top = ioremap(0x10000000, 0x100);
 void __iomem *pll = ioremap(0x10209000, 0x300);
 if (!top || !pll) { if (top) iounmap(top); if (pll) iounmap(pll); return -ENOMEM; }
 pr_info("Y2GPU_READ_ONLY: CLK_CFG_1=%08x MMPLL_CON0=%08x MMPLL_CON1=%08x UNIVPLL_CON0=%08x UNIVPLL_CON1=%08x PLL_HP_CON0=%08x\n", readl(top+0x50), readl(pll+0x230), readl(pll+0x234), readl(pll+0x220), readl(pll+0x224), readl(pll+0x14));
 iounmap(pll); iounmap(top);
 return -EAGAIN; /* Evidence printed; no module remains resident. */
}
module_init(snapshot_init);
MODULE_LICENSE("GPL");
