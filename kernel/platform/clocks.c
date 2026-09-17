// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 inherited-rate CCF providers. Register provenance: pinned vendor
 * mt_clkmgr.c sdm_pll_vco_calc_op/muxs/grps and mt_pm_init.c mt_get_bus_freq.
 * M4 adds the BSP ARMPLL transition through MAINPLL/2; no other PLL retune.
 * Unknown muxes report zero.
 * CLK_IGNORE_UNUSED preserves loader consumers not yet modeled in Linux.
 */
#include "clocks.h"
#include "policy.h"
#include "power-math.h"
#include "shared.h"
#include "spm.h"
#include <linux/clk-provider.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/iopoll.h>
#include <linux/module.h>
#include <linux/platform_device.h>
struct y2_clock {
	struct clk_hw hw;
	void __iomem *pll, *top, *infra, *gate;
	unsigned id, bit;
	bool inherited;
};
static void __iomem *y2_clock_bases[4];
static DEFINE_SPINLOCK(y2_clk_lock);

/* Same INFRACFG_AO owner as the CPU mux. Original Y2 secondary hotplug and
 * dormant code use +0x800/+0x804; never write a loader image or RTC word. */
int y2_ccf_boot_vector(unsigned long entry)
{
	unsigned long flags;
	void __iomem *infra = y2_clock_bases[2];
	int ret = 0;
	if (!infra) return -EPROBE_DEFER;
	if (entry < 0x80004000 || entry >= 0xbdf00000 || (entry & 3)) return -EINVAL;
	spin_lock_irqsave(&y2_clk_lock, flags);
	writel(entry, infra + 0x800);
	writel(readl(infra + 0x804) | BIT(31), infra + 0x804);
	if (readl(infra + 0x800) != entry || !(readl(infra + 0x804) & BIT(31))) ret = -EIO;
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return ret;
}
static const char *const y2_clk_names[] = {
    "y2-armpll", "y2-mainpll", "y2-univpll", "y2-mmpll",	"y2-msdcpll",
    "y2-axi",	 "y2-i2c0",    "y2-i2c1",    "y2-apdma",	"y2-pwrap",
    "y2-kp",	 "y2-msdc0",   "y2-msdc1",   "y2-msdc0-source", "y2-msdc1-source",
    "y2-audintbus", "y2-audio", "y2-infra-audio", "y2-cpu", "y2-therm", "y2-auxadc", "y2-efuse",
    "y2-connmcu", "y2-btif", "y2-mfg-source"};

/* Shared INFRACFG fields stay with this owner. Callers serialize complete
 * domain transitions; each RMW is protected against CPU/clock operations. */
int y2_ccf_radio_protect(unsigned domain, bool protect)
{
	void __iomem *infra = y2_clock_bases[2];
	unsigned mask, value; unsigned long flags;
	if (!infra) return -EPROBE_DEFER;
	if (domain > 1) return -EINVAL;
	mask = domain ? 0xb8 : 0x104; /* MD1 / CONN, stock mt_clkmgr */
	spin_lock_irqsave(&y2_clk_lock, flags);
	value = readl(infra + 0x220);
	writel(protect ? value | mask : value & ~mask, infra + 0x220);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return readl_poll_timeout(infra + 0x228, value,
		(value & mask) == (protect ? mask : 0), 10, 10000);
}
int y2_ccf_md_unconfigured(void)
{
	void __iomem *infra = y2_clock_bases[2];
	unsigned long flags; unsigned value;
	if (!infra) return -EPROBE_DEFER;
	spin_lock_irqsave(&y2_clk_lock, flags);
	value = readl(infra+0x300) | readl(infra+0x304) |
		readl(infra+0x308) | readl(infra+0x30c);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return !value;
}
int y2_ccf_radio_remap(unsigned domain)
{
	void __iomem *infra = y2_clock_bases[2];
	unsigned long flags; unsigned value;
	int ret = 0;
	if (!infra) return -EPROBE_DEFER;
	if (domain > 1) return -EINVAL;
	spin_lock_irqsave(&y2_clk_lock, flags);
	if (!domain) {
		/* Stock mtk_wcn_consys_hw_init accesses virtual 0xf0001310:
		 * physical 0x10001310, or +0x310 from this INFRACFG_AO base.
		 * +0x1310 is relative to the vendor's 0x10000000 TOPCKGEN
		 * base and would leave CONN's real EMI translation unchanged. */
		/* Replace the complete address field; OR-ing a previous loader
		 * address can redirect firmware into unrelated reserved memory. */
		value = (readl(infra + 0x310) & ~0x1fffU) | 0x1bdf;
		writel(value, infra + 0x310);
		/* Never release the remote MCU until its DRAM target is verified. */
		if ((readl(infra + 0x310) & 0x1fffU) != 0x1bdf) ret = -EIO;
	} else {
		/* MD1 ROM 0xbe000000 / shared memory 0xbf600000. Unused 32-MiB
		 * banks map beyond physical DRAM, as in the retained MD1 boot. */
		writel(0x53514f3f, infra + 0x300);
		writel(0x5b595755, infra + 0x304);
		writel(0x4543413f, infra + 0x308);
		writel(0x4d4b4947, infra + 0x30c);
	}
	mb();
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return ret;
}
static unsigned long y2_pll_rate(void __iomem *base, unsigned id)
{
	unsigned con0 = readl(base + 0x200 + id * 16), con1 = readl(base + 0x204 + id * 16);
	return y2_pll_decode(con0, con1, id == Y2_CLK_ARMPLL);
}
static unsigned long y2_axi_rate(struct y2_clock *c)
{
	unsigned mux = readl(c->top + 0x40) & 7;
	switch (mux) {
	case 0:
		return 26000000;
	case 1:
		return y2_pll_rate(c->pll, 1) / 4;
	case 2:
		return y2_pll_rate(c->pll, 1) / 5;
	case 3:
		return y2_pll_rate(c->pll, 1) / 8;
	case 4:
		return y2_pll_rate(c->pll, 2) / 5;
	case 5:
		return y2_pll_rate(c->pll, 2) / 6;
	default:
		return 0; /* DMPLL rate is not guessed. */
	}
}
static unsigned long y2_rate(struct clk_hw *hw, unsigned long parent)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	/* Own boot snapshot: CLK_CFG_1=0x00010100, MMPLL=500.5 MHz.
	 * The actual MT6582 stock Mali platform uses this inherited source;
	 * donor's MMPLL/2 and fixed 286 MHz are not the stock contract. */
	if (c->id == Y2_CLK_MFG_SRC)
		return ((readl(c->top + 0x50) >> 16) & 7) == 1 ? parent : 0;
	if (c->id == Y2_CLK_CPU) {
		unsigned mux = readl(c->infra) & 12, div = readl(c->infra + 8) & 31;
		unsigned long rate = mux == 4 ? y2_pll_rate(c->pll, 0) :
			mux == 8 ? y2_pll_rate(c->pll, 1) : mux == 0 ? 26000000 : 0;
		return div == 0 ? rate : div == 10 ? rate / 2 : 0;
	}
	if (c->id < 5)
		return y2_pll_rate(c->pll, c->id);
	if (c->id == Y2_CLK_AXI)
		return y2_axi_rate(c);
	if (c->id == Y2_CLK_MSDC0_SRC || c->id == Y2_CLK_MSDC1_SRC) {
		unsigned r = readl(c->top + (c->id == Y2_CLK_MSDC0_SRC ? 0x60 : 0x70));
		unsigned shift = c->id == Y2_CLK_MSDC0_SRC ? 24 : 0;
		return ((r >> shift) & 7) == 0 ? 26000000 : 0;
	}
	if (c->id == Y2_CLK_AUDINTBUS || c->id == Y2_CLK_AUDIO) {
		unsigned shift = c->id == Y2_CLK_AUDIO ? 16 : 24;
		unsigned mask = c->id == Y2_CLK_AUDIO ? 1 : 7;
		return ((readl(c->top + 0x70) >> shift) & mask) ? 0 : 26000000;
	}
	return parent;
}
static int y2_enable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	if (c->gate)
		writel(BIT(c->bit), c->gate + 4); /* CLR: infra44/peri10 */
	return 0;
}
static void y2_disable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	/* Do not remove inherited clocks from still-unmodeled loader consumers. */
	if (c->gate && !c->inherited)
		writel(BIT(c->bit), c->gate);
}
static int y2_enabled(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	return !c->gate || !(readl(c->gate + 8) & BIT(c->bit));
}
/* Peri has non-contiguous SET/CLR/STA, unlike infracfg. */
static int y2_peri_enable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	writel(BIT(c->bit), c->gate + 8);
	return 0;
}
static int y2_peri_enabled(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	return !(readl(c->gate + 16) & BIT(c->bit));
}
static const struct clk_ops y2_ro_ops = {.recalc_rate = y2_rate};
static long y2_cpu_round(struct clk_hw *hw, unsigned long rate, unsigned long *parent)
{
	return y2_cpu_pcw(rate) ? rate : -EINVAL;
}
static int y2_cpu_set(struct clk_hw *hw, unsigned long rate, unsigned long parent)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned pcw = y2_cpu_pcw(rate), mux, old;
	unsigned long flags;
	int ret;
	if (!IS_ENABLED(CONFIG_Y2_POWER) || !pcw) return -EINVAL;
	ret = y2_pmic_cpu_voltage_ready();
	if (ret) return ret;
	spin_lock_irqsave(&y2_clk_lock, flags);
	mux = readl(c->infra); old = readl(c->pll + 0x204);
	/* PLL_HP_CON0 bit0: ARMPLL FHCTL ownership. Never fight it. Also
	 * require the measured 1092 MHz fallback and the modeled divider state. */
	if ((readl(c->pll + 0x14) & BIT(0)) || (mux & 12) != 4 ||
	    readl(c->infra + 8) != 0 || y2_pll_rate(c->pll, 1) != 1092000000 ||
	    y2_pll_decode(readl(c->pll + 0x200), pcw, 1) != rate) {
		ret = -EOPNOTSUPP; goto out;
	}
	/* BSP non-FHCTL sequence, shared by all four CPUs. Divider first keeps
	 * MAINPLL at 546 MHz, below every enabled CPU operating point. */
	writel(0x0a, c->infra + 8);
	if (readl(c->infra + 8) != 0x0a) { ret = -EIO; goto out; }
	writel((mux & ~12) | 8, c->infra);
	if ((readl(c->infra) & 12) != 8) {
		writel(mux, c->infra);
		if ((readl(c->infra) & 12) == 4) writel(0, c->infra + 8);
		ret = -EIO; goto out;
	}
	writel(pcw, c->pll + 0x204);
	mb(); udelay(30);
	if ((readl(c->pll + 0x204) & 0x071fffff) != (pcw & 0x071fffff)) {
		writel(old | BIT(31), c->pll + 0x204);
		mb(); udelay(30);
		ret = -EIO;
		/* If restoring ARMPLL fails too, keep the verified MAINPLL/2
		 * fallback. Never switch CPUs onto an unverified PLL. */
		if ((readl(c->pll + 0x204) & 0x071fffff) != (old & 0x071fffff)) goto out;
	}
	writel(mux, c->infra);
	writel(0, c->infra + 8);
	readl(c->infra + 8);
out:
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return ret;
}
static const struct clk_ops y2_cpu_ops = {
	.recalc_rate = y2_rate, .round_rate = y2_cpu_round, .set_rate = y2_cpu_set,
};
static const struct clk_ops y2_infra_ops = {
    .recalc_rate = y2_rate, .enable = y2_enable, .disable = y2_disable, .is_enabled = y2_enabled};
static const struct clk_ops y2_peri_ops = {.recalc_rate = y2_rate,
					   .enable = y2_peri_enable,
					   .disable = y2_disable,
					   .is_enabled = y2_peri_enabled};
/* Vendor CLK_CFG_3 audio muxes. Select their documented crystal parent;
 * no approximate donor PLL factors and no writes to the adjacent SD muxes. */
static int y2_audio_enable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned shift = c->id == Y2_CLK_AUDIO ? 16 : 24;
	unsigned mask = (c->id == Y2_CLK_AUDIO ? 0x81U : 0x87U) << shift;
	unsigned long flags;
	spin_lock_irqsave(&y2_clk_lock, flags);
	writel(readl(c->top + 0x70) & ~mask, c->top + 0x70);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return 0;
}
static void y2_audio_disable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned bit = c->id == Y2_CLK_AUDIO ? 23 : 31;
	unsigned long flags;
	if (c->inherited)
		return;
	spin_lock_irqsave(&y2_clk_lock, flags);
	writel(readl(c->top + 0x70) | BIT(bit), c->top + 0x70);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
}
static int y2_audio_enabled(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	return !(readl(c->top + 0x70) & BIT(c->id == Y2_CLK_AUDIO ? 23 : 31));
}
static const struct clk_ops y2_audio_ops = {
	.recalc_rate = y2_rate, .enable = y2_audio_enable,
	.disable = y2_audio_disable, .is_enabled = y2_audio_enabled,
};
static int y2_mfg_source_enable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned long flags;
	unsigned value;
	int ret = 0;
	spin_lock_irqsave(&y2_clk_lock, flags);
	value = readl(c->top + 0x50);
	/* Preserve the proven selector and PLL. Unknown loader state fails
	 * GPU activation rather than applying an unproven frequency/voltage. */
	if (((value >> 16) & 7) != 1 || y2_pll_rate(c->pll, Y2_CLK_MMPLL) != 500500000)
		ret = -EINVAL;
	else {
		writel(value & ~BIT(23), c->top + 0x50);
		if (readl(c->top + 0x50) & BIT(23)) ret = -EIO;
	}
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return ret;
}
static void y2_mfg_source_disable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned long flags;
	spin_lock_irqsave(&y2_clk_lock, flags);
	writel(readl(c->top + 0x50) | BIT(23), c->top + 0x50);
	readl(c->top + 0x50);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
}
static int y2_mfg_source_enabled(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	return !(readl(c->top + 0x50) & BIT(23));
}
static const struct clk_ops y2_mfg_source_ops = {
	.recalc_rate = y2_rate, .enable = y2_mfg_source_enable,
	.disable = y2_mfg_source_disable, .is_enabled = y2_mfg_source_enabled,
};
/* Called only by the MT6582 MMC variant after checking inherited DMA idle.
 * Source 0 is the documented crystal, giving a real 26MHz rate contract. */
int y2_msdc_crystal(unsigned id)
{
	unsigned long flags;
	unsigned off = id ? 0x70 : 0x60, shift = id ? 0 : 24, v;
	if (!y2_clock_bases[0] || id > 1)
		return -EPROBE_DEFER;
	spin_lock_irqsave(&y2_clk_lock, flags);
	v = readl(y2_clock_bases[0] + off);
	v &= ~(0x87U << shift);
	writel(v, y2_clock_bases[0] + off);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return 0;
}
int y2_ccf_usb_read(unsigned address, unsigned *value)
{
	unsigned index, offset;
	switch (address) {
	case 0x10003018:
		index = 1;
		offset = 0x18;
		break;
	case 0x10000060:
		index = 0;
		offset = 0x60;
		break;
	case 0x10209220:
		index = 3;
		offset = 0x220;
		break;
	case 0x1020922c:
		index = 3;
		offset = 0x22c;
		break;
	default:
		return -EINVAL;
	}
	if (!y2_clock_bases[index])
		return -EPROBE_DEFER;
	*value = readl(y2_clock_bases[index] + offset);
	return 0;
}
static int y2_clocks_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct clk_hw_onecell_data *data;
	void __iomem *base[4];
	unsigned i;
	int ret;
	data = devm_kzalloc(dev, struct_size(data, hws, Y2_CLK_NR), GFP_KERNEL);
	if (!data)
		return -ENOMEM;
	for (i = 0; i < 4; i++) {
		base[i] = devm_platform_ioremap_resource(pdev, i);
		if (IS_ERR(base[i]))
			return PTR_ERR(base[i]);
	}
	data->num = Y2_CLK_NR;
	for (i = 0; i < Y2_CLK_NR; i++) {
		struct y2_clock *c = devm_kzalloc(dev, sizeof(*c), GFP_KERNEL);
		struct clk_init_data init = {.name = y2_clk_names[i],
					     .ops = &y2_ro_ops,
					     .flags = CLK_GET_RATE_NOCACHE | CLK_IGNORE_UNUSED};
		const char *parent = "clk26m";
		if (!c)
			return -ENOMEM;
		c->id = i;
		c->top = base[0];
		c->infra = base[2];
		c->pll = base[3];
		if (i == Y2_CLK_CPU) { init.ops = &y2_cpu_ops; parent = "y2-armpll"; }
		if (i == Y2_CLK_MFG_SRC) {
			init.ops = &y2_mfg_source_ops;
			parent = "y2-mmpll";
			init.flags = CLK_GET_RATE_NOCACHE;
		}
		if (i == Y2_CLK_THERM || i == Y2_CLK_AUXADC) {
			c->gate = base[1] + 8;
			c->bit = i == Y2_CLK_THERM ? 1 : 24;
			parent = "y2-axi";
			init.ops = &y2_peri_ops;
		}
		if (i == Y2_CLK_EFUSE) {
			c->gate = base[2] + 0x40;
			c->bit = 6;
			init.ops = &y2_infra_ops;
		}
		if (i == Y2_CLK_CONNMCU) {
			c->gate = base[2] + 0x40; c->bit = 12;
			init.ops = &y2_infra_ops;
		}
		if (i == Y2_CLK_BTIF) {
			c->gate = base[1] + 8; c->bit = 20;
			init.ops = &y2_peri_ops;
		}
		if (i >= 6 && i <= 8) {
			c->gate = base[1] + 8;
			c->bit = i == 8 ? 11 : i + 15;
			parent = "y2-axi";
			init.ops = &y2_peri_ops;
		}
		if (i == 9 || i == 10) {
			c->gate = base[2] + 0x40;
			c->bit = i == 9 ? 23 : 16;
			init.ops = &y2_infra_ops;
		}
		if (i == 11 || i == 12) {
			c->gate = base[1] + 8;
			c->bit = i + 1;
			parent = y2_clk_names[i + 2];
			init.ops = &y2_peri_ops;
		}
		if (i == Y2_CLK_AUDINTBUS || i == Y2_CLK_AUDIO) {
			init.ops = &y2_audio_ops;
			c->inherited = y2_audio_enabled(&c->hw);
		}
		if (i == Y2_CLK_INFRA_AUDIO) {
			c->gate = base[2] + 0x40;
			c->bit = 5;
			parent = "y2-audintbus";
			init.ops = &y2_infra_ops;
		}
		if (c->gate)
			c->inherited = init.ops->is_enabled(&c->hw);
		/* These two gates now have a complete connectivity owner. Their
		 * balanced CCF references may gate them after an inherited boot. */
		if (i == Y2_CLK_CONNMCU || i == Y2_CLK_BTIF) c->inherited = false;
		init.parent_names = &parent;
		init.num_parents = 1;
		c->hw.init = &init;
		ret = devm_clk_hw_register(dev, &c->hw);
		if (ret)
			return dev_err_probe(dev, ret, "clock %s\n", init.name);
		data->hws[i] = &c->hw;
	}
	ret = devm_of_clk_add_hw_provider(dev, of_clk_hw_onecell_get, data);
	if (ret)
		return ret;
	for (i = 0; i < 4; i++)
		y2_clock_bases[i] = base[i];
	dev_info(dev, "CCF inherited PLLs/AXI; guarded shared CPU clock; AXI=%lu Hz\n",
		 clk_hw_get_rate(data->hws[5]));
	return 0;
}
static const struct of_device_id y2_clocks_match[] = {{.compatible = "innioasis,y2-clocks"}, {}};
static struct platform_driver y2_clocks_driver = {.probe = y2_clocks_probe,
						  .driver = {.name = "y2-clocks",
							     .of_match_table = y2_clocks_match,
							     .suppress_bind_attrs = true}};
static int __init y2_clocks_init(void) { return platform_driver_register(&y2_clocks_driver); }
subsys_initcall(y2_clocks_init);
MODULE_LICENSE("GPL");
