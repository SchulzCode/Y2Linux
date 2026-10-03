// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 inherited-rate CCF providers. Register provenance: pinned vendor
 * mt_clkmgr.c sdm_pll_vco_calc_op/muxs/grps and mt_pm_init.c mt_get_bus_freq.
 * M4 adds the BSP ARMPLL transition through MAINPLL/2; no other PLL retune.
 * Unknown muxes report zero.
 * CLK_IGNORE_UNUSED preserves loader consumers not yet modeled in Linux.
 */
#include "clocks.h"
#include "idle-clock-policy.h"
#include "policy.h"
#include "power-math.h"
#include "shared.h"
#include "spm.h"
#include "cpu-dvfs.h"
#include "cpu-dvfs-policy.h"
#include <linux/clk-provider.h>
#include <linux/clk.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/iopoll.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>
#include <asm/proc-fns.h>
struct y2_clock {
	struct clk_hw hw;
	void __iomem *pll, *top, *infra, *gate;
	unsigned id, bit;
	bool inherited;
};
static void __iomem *y2_clock_bases[4];
static DEFINE_SPINLOCK(y2_clk_lock);
static struct y2_clock *y2_usb_clock;
/* Only the MUSB owner calls this after claiming an idle DMA/controller.
 * Failed probes keep LK's gate protected; successful runtime PM may gate it. */
int y2_ccf_usb_claim(void)
{
	unsigned long flags;
	if (!smp_load_acquire(&y2_usb_clock)) return -EPROBE_DEFER;
	spin_lock_irqsave(&y2_clk_lock, flags);
	y2_usb_clock->inherited = false;
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return 0;
}
static unsigned slow_blockers, deep_peri_blockers, deep_infra_blockers, slow_restore_failures;
module_param(slow_restore_failures, uint, 0400);
module_param(slow_blockers, uint, 0400);
module_param(deep_peri_blockers, uint, 0400);
module_param(deep_infra_blockers, uint, 0400);
/* SLIDLE refusal attribution (CPU0 is the only writer). Topology refusals
 * happen before any clock read, so slow_blockers alone cannot prove idle
 * clocks; slow_blockers_single retains the last mask read with one CPU. */
static unsigned long slow_reject_topology, slow_reject_lock, slow_reject_bus, slow_reject_clock;
static unsigned slow_blockers_single, slow_blocker_bits[24];
module_param(slow_reject_topology, ulong, 0400);
module_param(slow_reject_lock, ulong, 0400);
module_param(slow_reject_bus, ulong, 0400);
module_param(slow_reject_clock, ulong, 0400);
module_param(slow_blockers_single, uint, 0400);
static int slow_bits_get(char *buf, const struct kernel_param *kp)
{
 unsigned bit; int length = 0;
 for (bit = 0; bit < ARRAY_SIZE(slow_blocker_bits); bit++)
  if (slow_blocker_bits[bit])
   length += scnprintf(buf + length, PAGE_SIZE - length, "bit%u=%u ", bit, READ_ONCE(slow_blocker_bits[bit]));
 return length + scnprintf(buf + length, PAGE_SIZE - length, "\n");
}
static const struct kernel_param_ops slow_bits_ops = {.get = slow_bits_get};
module_param_cb(slow_blocker_counts, &slow_bits_ops, NULL, 0400);

/* PERI0 group bits from exact MT6582 mt_clkmgr.h, not adjacent SoCs. */
static const char *const slow_owners[24] = {
 [11]="APDMA:connectivity/I2C-DMA", [12]="MSDC0:eMMC", [13]="MSDC1:SD",
 [14]="MSDC2:unused-controller", [20]="BTIF:connectivity",
 [21]="I2C0:wheel", [22]="I2C1:DAC", [23]="I2C2:unused-controller"
};
static int slow_names_get(char *buf, const struct kernel_param *kp)
{
 unsigned mask = READ_ONCE(slow_blockers), bit; int length = 0;
 for (bit=0;bit<ARRAY_SIZE(slow_owners);bit++)
  if ((mask & BIT(bit)) && slow_owners[bit])
   length += scnprintf(buf+length,PAGE_SIZE-length,"bit%u=%s ",bit,slow_owners[bit]);
 return length + scnprintf(buf+length,PAGE_SIZE-length,"\n");
}
static const struct kernel_param_ops slow_names_ops={.get=slow_names_get};
module_param_cb(slow_blocker_names,&slow_names_ops,NULL,0400);

/* Exact stock SLIDLE bus DCM sequence, serialized with every CCF gate/mux.
 * The caller keeps IRQs disabled; the local clockevent bounds WFI residency.
 * No PCM or context loss, and no activation on an unknown inherited value. */
int y2_ccf_slow_idle(void)
{
	void __iomem *top = y2_clock_bases[0], *peri = y2_clock_bases[1];
	unsigned saved;
	int ret = 0;
	if (!top || !peri) return -ENODEV;
	if (smp_processor_id() != 0) return -EBUSY;
	if (num_online_cpus() != 1) { slow_reject_topology++; return -EBUSY; }
	if (!spin_trylock(&y2_clk_lock)) { slow_reject_lock++; return -EBUSY; }
	saved = readl(top + 4);
	/* Stock PERI blocker mask includes APDMA and I2C; additionally retain
	 * every MSDC blocker. CG bits read as one when the clock is disabled. */
	slow_blockers = (~readl(peri + 0x18)) & (0x00f00800 | 0x00007800);
	slow_blockers_single = slow_blockers;
	if (slow_blockers) {
		unsigned bit;
		for (bit = 0; bit < ARRAY_SIZE(slow_blocker_bits); bit++)
			if (slow_blockers & BIT(bit)) slow_blocker_bits[bit]++;
		slow_reject_clock++;
		ret = -EBUSY; goto out;
	}
	if (!y2_bus_dcm_baseline(saved)) {
		slow_reject_bus++;
		ret = -EBUSY; goto out;
	}
	writel(Y2_BUS_DCM_IDLE, top + 4);
	if (readl(top + 4) != Y2_BUS_DCM_IDLE) { ret = -EIO; goto restore; }
	dsb(sy);
	cpu_do_idle();
restore:
	writel(saved, top + 4);
	dsb(sy);
	if (readl(top + 4) != saved) { slow_restore_failures++; ret = -EIO; }
out:
	spin_unlock(&y2_clk_lock);
	return ret;
}

/* Deep idle transaction retains the clock-owner lock through WFI. No new
 * clock consumer can start DMA/AFE while system clocks are handed to PCM. */
static unsigned idle_bus, idle_audio;
static unsigned deep_disp0_blockers, deep_disp1_blockers;
module_param(deep_disp0_blockers, uint, 0400);
module_param(deep_disp1_blockers, uint, 0400);
int y2_ccf_deep_idle_begin(void)
{
	void __iomem *top = y2_clock_bases[0], *peri = y2_clock_bases[1];
	void __iomem *infra = y2_clock_bases[2];
	if (!top || !peri || !infra) return -ENODEV;
	if (!spin_trylock(&y2_clk_lock)) return -EBUSY;
	/* Exact stock PERI/INFRA/DISP masks, plus all MSDC and AFE blockers.
	 * Other groups are checked as powered-off domains by SPM. */
	deep_peri_blockers = ~readl(peri + 0x18) & (0x02fe87fdU | 0x7800U);
	deep_infra_blockers = ~readl(infra + 0x40) & (0x0000a080U | BIT(5));
	if (deep_peri_blockers || deep_infra_blockers) goto busy;
	if (y2_mm_idle_blockers(&deep_disp0_blockers, &deep_disp1_blockers) ||
	    deep_disp0_blockers || deep_disp1_blockers) goto busy;
	idle_bus = readl(top + 4);
	idle_audio = readl(top + 0x70);
	if (!y2_bus_dcm_baseline(idle_bus)) goto busy;
	writel(Y2_BUS_DCM_IDLE, top + 4);
	writel(idle_audio & 0xf8ffffff, top + 0x70);
	dsb(sy);
	if (readl(top + 4) != Y2_BUS_DCM_IDLE || readl(top + 0x70) != (idle_audio & 0xf8ffffff)) {
		y2_ccf_deep_idle_end();
		return -EIO;
	}
	return 0;
busy:
	spin_unlock(&y2_clk_lock);
	return -EBUSY;
}
/* Read-only DORMANT clock prerequisites (same masks as deep_idle_begin);
 * never takes ownership, writes a gate or holds the lock beyond the reads. */
int y2_ccf_deep_idle_blockers(unsigned *peri, unsigned *infra, unsigned *bus)
{
	void __iomem *top = y2_clock_bases[0], *p = y2_clock_bases[1], *i = y2_clock_bases[2];
	unsigned long flags;
	if (!top || !p || !i) return -ENODEV;
	spin_lock_irqsave(&y2_clk_lock, flags);
	*peri = ~readl(p + 0x18) & (0x02fe87fdU | 0x7800U);
	*infra = ~readl(i + 0x40) & (0x0000a080U | BIT(5));
	*bus = readl(top + 4);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return 0;
}
int y2_ccf_deep_idle_end(void)
{
	void __iomem *top = y2_clock_bases[0];
	int ret = 0;
	writel(idle_audio, top + 0x70);
	writel(idle_bus, top + 4);
	dsb(sy);
	if (readl(top + 4) != idle_bus || readl(top + 0x70) != idle_audio) ret = -EIO;
	spin_unlock(&y2_clk_lock);
	return ret;
}

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
void y2_ccf_boot_state(unsigned *vector, unsigned *enable)
{
	void __iomem *infra = y2_clock_bases[2];
	*vector = infra ? readl(infra + 0x800) : 0;
	*enable = infra ? readl(infra + 0x804) : 0;
}
static const char *const y2_clk_names[] = {
    "y2-armpll", "y2-mainpll", "y2-univpll", "y2-mmpll",	"y2-msdcpll",
    "y2-axi",	 "y2-i2c0",    "y2-i2c1",    "y2-apdma",	"y2-pwrap",
    "y2-kp",	 "y2-msdc0",   "y2-msdc1",   "y2-msdc0-source", "y2-msdc1-source",
    "y2-audintbus", "y2-audio", "y2-infra-audio", "y2-cpu", "y2-therm", "y2-auxadc", "y2-efuse",
    "y2-connmcu", "y2-btif", "y2-mfg-source", "y2-msdc2-unused", "y2-i2c2-unused",
    "y2-nfi-unused", "y2-pwm1-unused", "y2-pwm2-unused", "y2-pwm3-unused",
    "y2-pwm4-unused", "y2-pwm5-unused", "y2-pwm6-unused", "y2-pwm7-unused",
    "y2-pwm-unused", "y2-nli-unused", "y2-uart1-unused", "y2-uart2-unused",
    "y2-uart3-unused", "y2-spi0-unused", "y2-l2c-sram-unused",
    "y2-trng-unused", "y2-cpum-unused", "y2-usb0"};

/* Normal CCF unused-clock ownership, with exact BSP activity checks. No DT
 * consumer exists for these engines on Y2; active/unknown LK state is retained.
 * INFRA bits7/13/15 are the BSP cg_bootup_pdn 0xb0e0 idle/test clocks, not
 * CA7_CACHE_CONFIG, CPU power, coherency or the cache's SRAM retention control. */
static const unsigned y2_unused_bits[] = {0,2,3,4,5,6,7,8,9,15,17,18,19,25,7,13,15};
/* Capture the boot/latest clocked recheck, never a later unclocked read. These
 * unsupported engines have no DT/Linux consumer; their configured IRQ/DMA or
 * unknown loader state remains protected. UART0 console is never sampled. */
struct y2_unused_handoff {
	unsigned sampled, line_sampled, lcr, ier, lsr, dma, command, status;
	int busy;
};
static struct y2_unused_handoff unused_handoff[4];
static void __iomem *unused_registers[4];
static struct y2_clock *unused_hw[4];
static struct clk *unused_clk[4];
static struct device *unused_dev;
static bool unused_ready;
static DEFINE_MUTEX(unused_handoff_lock);
static unsigned unused_rechecks, unused_reclaimed, unused_failures;
module_param(unused_rechecks, uint, 0400);
module_param(unused_reclaimed, uint, 0400);
module_param(unused_failures, uint, 0400);
static int unused_handoff_get(char *buf, const struct kernel_param *kp)
{
	unsigned i;
	int n = 0;
	mutex_lock(&unused_handoff_lock);
	for (i = 0; i < ARRAY_SIZE(unused_handoff); i++) {
		struct y2_unused_handoff *h = &unused_handoff[i];
		if (!h->sampled) {
			n += scnprintf(buf + n, PAGE_SIZE - n,
				"%s%u sampled=0 reason=already_gated retained=0\n",
				i < 3 ? "UART" : "SPI", i < 3 ? i + 1 : 0);
			continue;
		}
		if (i < 3 && (h->lcr & 0x80))
			n += scnprintf(buf + n, PAGE_SIZE - n,
				"UART%u sampled=1 lcr=%#x reason=alternate_bank ier=unavailable lsr=unavailable dma=unavailable retained=1\n",
				i + 1, h->lcr);
		else if (i < 3 && !h->line_sampled)
			n += scnprintf(buf + n, PAGE_SIZE - n,
				"UART%u sampled=1 lcr=%#x ier=%#x lsr=unavailable dma=%#x reason=%s retained=1\n",
				i + 1, h->lcr, h->ier, h->dma,
				h->ier ? "irq_enabled" : h->dma & 3 ? "dma_enabled" : "unknown_dma_bits");
		else if (i < 3)
			n += scnprintf(buf + n, PAGE_SIZE - n,
				"UART%u sampled=1 lcr=%#x ier=%#x lsr=%#x dma=%#x reason=%s retained=%d\n",
				i + 1, h->lcr, h->ier, h->lsr, h->dma,
				h->ier ? "irq_enabled" : h->dma & 3 ? "dma_enabled" :
				h->dma & ~7U ? "unknown_dma_bits" : h->lsr & 1 ? "rx_data" :
				(h->lsr & 0x60) != 0x60 ? "tx_not_drained" :
				h->lsr != 0x60 ? "line_or_unknown_status" : "quiet", h->busy);
		else
			n += scnprintf(buf + n, PAGE_SIZE - n,
				"SPI0 sampled=1 command=%#x status1=%#x reason=%s retained=%d\n",
				h->command, h->status, h->command & 0xc17 ? "command_or_dma" :
				!h->status ? "not_idle" : h->status != 1 ? "unknown_status" :
				"quiet", h->busy);
	}
	mutex_unlock(&unused_handoff_lock);
	return n;
}
static const struct kernel_param_ops unused_handoff_ops = {.get = unused_handoff_get};
module_param_cb(unused_handoff, &unused_handoff_ops, NULL, 0400);
static int y2_unused_busy(struct device *dev, unsigned index, unsigned peri)
{
	void __iomem *r;
	unsigned bit = y2_unused_bits[index];
	if (index >= 14) return 0; /* unconsumed BSP idle/test clocks */
	if (!index || index == 9) {
		/* NAND accesses require both stock NFI/NLI clocks. An unknown
		 * partial loader handoff is retained, never read unclocked. */
		if (peri & (BIT(0) | BIT(15)))
			return 1; /* retain this gate; keep the shared provider alive */
		r = devm_ioremap(dev, 0x1100d000, 0x214);
		if (!r) return -ENOMEM;
		return y2_unused_nfi_busy(readl(r + 8), readl(r + 0x60),
			readl(r + 0x210), readl(r + 0x64));
	}
	if (index <= 8) {
		if (peri & BIT(9)) return 1; /* retain: PWM register clock is off */
		r = devm_ioremap(dev, 0x11006000, 0x210);
		if (!r) return -ENOMEM;
		/* Include sequencer/test/3D modes; never break a live PWM source. */
		return readl(r) || readl(r + 0x1d0);
	}
	if (index <= 12) {
		struct y2_unused_handoff *h = &unused_handoff[index - 10];
		r = unused_registers[index - 10];
		if (!r)
			r = devm_ioremap(dev, 0x11002000 + (bit - 16) * 0x1000, 0x80);
		if (!r) return -ENOMEM;
		unused_registers[index - 10] = r;
		h->line_sampled = 0;
		h->lcr = readl(r + 0xc);
		if (h->lcr & 0x80) {
			/* +4 aliases DLH; the enhanced bank also aliases LSR. */
			h->sampled = 1;
			h->busy = 1;
			return 1;
		}
		h->ier = readl(r + 4);
		h->dma = readl(r + 0x4c);
		if (h->ier || (h->dma & ~4U)) {
			/* LSR reads clear line errors. Do not disturb a live IRQ/DMA owner. */
			h->sampled = 1;
			h->busy = 1;
			return 1;
		}
		h->lsr = readl(r + 0x14);
		h->line_sampled = 1;
		h->busy = y2_unused_uart_busy(h->lcr, h->ier, h->lsr, h->dma);
		h->sampled = 1;
		return h->busy;
	}
	r = unused_registers[3];
	if (!r) r = devm_ioremap(dev, 0x1100a000, 0x30);
	if (!r) return -ENOMEM;
	unused_registers[3] = r;
	unused_handoff[3].command = readl(r + 0x18);
	unused_handoff[3].status = readl(r + 0x20);
	unused_handoff[3].busy = y2_unused_spi_busy(unused_handoff[3].command,
		unused_handoff[3].status);
	unused_handoff[3].sampled = 1;
	return unused_handoff[3].busy;
}

/* Process-context handoff from the existing deferrable idle worker. A
 * transient loader TX/SPI operation must not permanently pin its clock.
 * Borrow/release through CCF; only a clock still retained and physically on
 * can be sampled. No IRQ/FIFO/DMA/reset writes, no extra polling timer. */
void y2_ccf_reclaim_unused(void)
{
	unsigned i, peri;
	unsigned long flags;
	if (!smp_load_acquire(&unused_ready) ||
	    y2_spm_radio_status(0) != 0 || y2_spm_radio_status(1) != 0) return;
	mutex_lock(&unused_handoff_lock);
	for (i = 0; i < ARRAY_SIZE(unused_hw); i++) {
		struct y2_clock *c = unused_hw[i];
		int busy;
		if (!c || !c->inherited || !unused_clk[i] || !unused_registers[i]) continue;
		peri = readl(y2_clock_bases[1] + 0x18);
		if (peri & BIT(c->bit)) continue; /* no unclocked MMIO or re-enable */
		if (clk_prepare_enable(unused_clk[i])) { unused_failures++; continue; }
		spin_lock_irqsave(&y2_clk_lock, flags);
		unused_rechecks++;
		busy = y2_unused_busy(unused_dev, i + 10, peri);
		if (!busy) c->inherited = false;
		spin_unlock_irqrestore(&y2_clk_lock, flags);
		clk_disable_unprepare(unused_clk[i]);
		if (!busy && !__clk_get_enable_count(unused_clk[i])) {
			if (readl(y2_clock_bases[1] + 0x18) & BIT(c->bit)) unused_reclaimed++;
			else {
				/* Do not repeatedly write a gate that failed readback. */
				unused_failures++;
				unused_clk[i] = NULL;
			}
		}
	}
	mutex_unlock(&unused_handoff_lock);
}

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
		switch ((r >> shift) & 7) {
		case 0: return 26000000;
		case 1: return y2_pll_rate(c->pll, Y2_CLK_MSDCPLL) / 2;
		default: return 0;
		}
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
	unsigned long flags;
	spin_lock_irqsave(&y2_clk_lock, flags);
	if (c->gate) writel(BIT(c->bit), c->gate + 4); /* infra CLR */
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return 0;
}
static void y2_disable(struct clk_hw *hw)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned long flags;
	/* Retain only genuinely unmodeled loader consumers. */
	spin_lock_irqsave(&y2_clk_lock, flags);
	if (c->gate && !c->inherited) writel(BIT(c->bit), c->gate);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
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
	unsigned long flags;
	spin_lock_irqsave(&y2_clk_lock, flags);
	writel(BIT(c->bit), c->gate + 8);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
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
static int y2_cpu_pll_set(struct clk_hw *hw, unsigned long rate, unsigned long parent)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	unsigned pcw = y2_cpu_pcw(rate), mux, old;
	unsigned long flags;
	int ret;
	if (!IS_ENABLED(CONFIG_Y2_POWER) || !pcw) return -EINVAL;
	ret = 0;
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
	/* Never remove MAINPLL's /2 divider before ARMPLL ownership is
	 * confirmed: failed mux restoration must retain the 546-MHz fallback. */
	if ((readl(c->infra) & 12) != 4) { ret = -EIO; goto out; }
	writel(0, c->infra + 8);
	if (readl(c->infra + 8) || (readl(c->infra) & 12) != 4)
		ret = -EIO;
out:
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	return ret;
}
static int y2_dvfs_voltage_get(void *context) { return y2_pmic_cpu_voltage_get(); }
static int y2_dvfs_voltage_set(void *context, unsigned value) { return y2_pmic_cpu_voltage_set(value); }
static int y2_dvfs_clock_set(void *context, unsigned long rate) { return y2_cpu_pll_set(context, rate, 0); }
static unsigned long y2_dvfs_clock_get(void *context) { return y2_rate(context, 0); }
static void y2_dvfs_fault(void *context) { y2_cpu_dvfs_fault(); }
static int y2_cpu_set(struct clk_hw *hw, unsigned long rate, unsigned long parent)
{
	struct y2_clock *c = container_of(hw, struct y2_clock, hw);
	struct y2_dvfs_io io = { hw, y2_dvfs_voltage_get, y2_dvfs_voltage_set,
		y2_dvfs_clock_set, y2_dvfs_clock_get, y2_dvfs_fault };
	/* Reject an unowned PLL or unknown fallback before raising any rail.
	 * The PLL operation repeats these checks under its register lock. */
	if ((readl(c->pll + 0x14) & BIT(0)) || (readl(c->infra) & 12) != 4 ||
	    readl(c->infra + 8) || y2_pll_rate(c->pll, 1) != 1092000000)
		return -EOPNOTSUPP;
	return y2_dvfs_transition(&io, rate, y2_cpu_dvfs_max());
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
 * Source 0 is the 26 MHz crystal; source 1 is the retained stock selection. */
int y2_msdc_source(unsigned id, bool highspeed)
{
	unsigned long flags;
	unsigned off = id ? 0x70 : 0x60, shift = id ? 0 : 24, v, selector = 0;
	unsigned long pll;
	if (!y2_clock_bases[0] || id > 1)
		return -EPROBE_DEFER;
	/* Retained Y2 LK 81e155ac..81e155d4 selects 1 for MSDC0/1.
	 * Exact stock host hclks[0]=200 MHz; inherited MSDCPLL is 400 MHz.
	 * No PLL retune. An unexpected PLL retains the known crystal fallback. */
	pll = y2_pll_rate(y2_clock_bases[3], Y2_CLK_MSDCPLL);
	if (highspeed && pll >= 398000000 && pll <= 402000000)
		selector = 1;
	spin_lock_irqsave(&y2_clk_lock, flags);
	v = readl(y2_clock_bases[0] + off);
	v &= ~(0x87U << shift);
	v |= selector << shift;
	writel(v, y2_clock_bases[0] + off);
	spin_unlock_irqrestore(&y2_clk_lock, flags);
	if (((readl(y2_clock_bases[0] + off) >> shift) & 0x87) != selector)
		return -EIO;
	pr_info("Y2MSDC%u: source=%s inherited_pll=%lu\n", id,
		selector ? "stock-msdcpll-div2" : "26MHz-crystal", pll);
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
		if (i >= Y2_CLK_UNUSED_START && i < Y2_CLK_USB0) {
			unsigned index = i - Y2_CLK_UNUSED_START;
			bool infra = index >= 14;
			int busy = 0;
			c->bit = y2_unused_bits[index];
			c->gate = infra ? base[2] + 0x40 : base[1] + 8;
			init.ops = infra ? &y2_infra_ops : &y2_peri_ops;
			if (!(readl(infra ? base[2] + 0x40 : base[1] + 0x18) & BIT(c->bit)))
				busy = y2_unused_busy(dev, index, readl(base[1] + 0x18));
			if (busy < 0) return busy;
			c->inherited = busy;
			if (!busy) init.flags &= ~CLK_IGNORE_UNUSED;
			else dev_warn(dev, "%s retained: active/unknown loader engine\n", init.name);
		}
		if (i == Y2_CLK_CPU) { init.ops = &y2_cpu_ops; parent = "y2-armpll"; }
		if (i == Y2_CLK_USB0) {
			c->gate = base[1] + 8; c->bit = 10; c->inherited = true;
			init.ops = &y2_peri_ops;
			parent = "y2-usb-inherited-rate-unresolved"; /* PHY owner checks actual source */
		}
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
		if (i == Y2_CLK_MSDC2 || i == Y2_CLK_I2C2) {
			void __iomem *controller, *dma = NULL;
			bool busy = false;
			c->gate = base[1] + 8;
			c->bit = i == Y2_CLK_MSDC2 ? 14 : 23;
			parent = "y2-axi"; init.ops = &y2_peri_ops;
			if (!(readl(base[1] + 0x18) & BIT(c->bit))) {
				controller = devm_ioremap(dev, i == Y2_CLK_MSDC2 ? 0x11250000 : 0x11009000, 0x100);
				if (!controller) return -ENOMEM;
				if (i == Y2_CLK_I2C2) {
					dma = devm_ioremap(dev, 0x11000300, 0x80);
					if (!dma) return -ENOMEM;
					busy = y2_unused_clock_busy(0, readl(controller + 0x24), readl(dma + 8));
				} else busy = y2_unused_clock_busy(1, readl(controller + 0x3c), readl(controller + 0x9c));
			}
			/* No DT/Linux consumer on Y2. Preserve a genuinely active LK
			 * engine; never reset DMA merely to remove an idle blocker. */
			if (!busy) init.flags &= ~CLK_IGNORE_UNUSED;
			else dev_warn(dev, "%s retained: inherited controller/DMA active\n", init.name);
			c->inherited = busy;
		}
		if (c->gate && i < Y2_CLK_UNUSED_START && i != Y2_CLK_MSDC2 && i != Y2_CLK_I2C2)
			c->inherited = init.ops->is_enabled(&c->hw);
		/* Linux now owns these controllers, including their complete DMA and
		 * runtime-PM lifecycle. Retaining the loader gate after CCF reaches
		 * zero leaked every I2C transaction and MSDC autosuspend. */
		if (i == Y2_CLK_CONNMCU || i == Y2_CLK_BTIF ||
		    i == Y2_CLK_I2C0 || i == Y2_CLK_I2C1 || i == Y2_CLK_APDMA ||
		    i == Y2_CLK_MSDC0 || i == Y2_CLK_MSDC1 ||
		    i == Y2_CLK_AUDIO || i == Y2_CLK_AUDINTBUS ||
		    i == Y2_CLK_INFRA_AUDIO) c->inherited = false;
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
	unused_dev = dev;
	for (i = 0; i < ARRAY_SIZE(unused_hw); i++) {
		struct clk_hw *hw = data->hws[Y2_CLK_UNUSED_START + 10 + i];
		unused_hw[i] = container_of(hw, struct y2_clock, hw);
		unused_clk[i] = devm_clk_hw_get_clk(dev, hw, "idle-handoff");
		if (IS_ERR(unused_clk[i])) {
			unused_failures++;
			unused_clk[i] = NULL; /* keep provider and inherited protection */
		}
	}
	smp_store_release(&unused_ready, true);
	smp_store_release(&y2_usb_clock, container_of(data->hws[Y2_CLK_USB0], struct y2_clock, hw));
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
