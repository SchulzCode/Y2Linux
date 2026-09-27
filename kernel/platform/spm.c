// SPDX-License-Identifier: GPL-2.0-only
/* Y2 SPM owner. Register/CPU sequence: Copyright (C) 2011-2014 MediaTek Inc.
 * Stock FM binary reconciliation: docs/knowledge/m4-end-user-power.md. */
#include <linux/io.h>
#include <linux/delay.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/spinlock.h>
#include <linux/cpu_pm.h>
#include <linux/dma-mapping.h>
#include <linux/interrupt.h>
#include <linux/suspend.h>
#include <linux/pm_domain.h>
#include <linux/clk.h>
#include <asm/cacheflush.h>
#include <asm/cp15.h>
#include <asm/proc-fns.h>
#include <asm/suspend.h>
#include "spm.h"
#include "spm-policy.h"
#include "spm-suspend-policy.h"
#include "spm-pcm.h"
#include "spm-idle-pcm.h"
#include "spm-idle-policy.h"
#include "cirq.h"
#include "clocks.h"
#include "local-timer.h"
#include "cpu-options.h"
#include <linux/cpufreq.h>
#include <linux/ktime.h>
#include "shared.h"
#include "connectivity/domain.h"
#include "gpu-policy.h"
#include "cpu-dvfs.h"

static void __iomem *spm_base;
static DEFINE_RAW_SPINLOCK(spm_lock);
static bool spm_broken;
static void __iomem *spm_biu, *spm_cache;
static dma_addr_t idle_address;
static unsigned dormant_entries, dormant_resumes, dormant_aborts, dormant_failures;
static int dormant_result;
static bool dormant_broken;
static char *dormant_stage = "never_entered";
static u64 resume_restore_ns;
static dma_addr_t pcm_address, normal_address;
static unsigned entries, resumes, aborts, last_wake, last_ticks, last_debug, last_event, last_r13;
static int last_result;
static struct generic_pm_domain y2_mfg_domain;
static bool mfg_broken;
static struct clk *mfg_source;

static unsigned spm_read(void *context, unsigned reg)
{
	return readl(context + reg);
}
static void spm_write(void *context, unsigned reg, unsigned value)
{
	writel(value, context + reg);
	/* Stock mt65xx_reg_sync_writel ordering, including command strobes. */
	dsb(sy);
}
static void spm_delay(unsigned us) { udelay(us); }
static struct y2_spm_io spm_io = { .read = spm_read, .write = spm_write, .delay = spm_delay };

int y2_spm_cpu_voltage_request(unsigned slot)
{
	unsigned long flags;
	unsigned n, pointer, length;
	int ret = 0;
	if (slot > 2) return -EINVAL;
	if (!smp_load_acquire(&spm_base)) return -EPROBE_DEFER;
	raw_spin_lock_irqsave(&spm_lock, flags);
	pointer = spm_read(spm_base, SPM_PCM_IM_PTR);
	length = spm_read(spm_base, SPM_PCM_IM_LEN);
	/* The suspend PCM includes the normal loop after wake. A timer/WDT
	 * still armed means it has not returned to the runtime contract. */
	if (READ_ONCE(spm_broken) ||
	    !((pointer == normal_address && length == 27) ||
	      (pointer == pcm_address && length == 596)) ||
	    (spm_read(spm_base, SPM_PCM_CON1) & (CON1_PCM_WDT_EN | CON1_PCM_TIMER_EN))) {
		ret = -EBUSY; goto out;
	}
	spm_write(spm_base, 0x604, (spm_read(spm_base, 0x604) & ~7U) | slot);
	udelay(5);
	for (n = 0; n <= 100; n++) {
		unsigned value = spm_read(spm_base, 0x604);
		if ((value & 7) != slot) { ret = -EIO; goto out; }
		if (value & BIT(31)) goto out;
		if (n != 100) udelay(5);
	}
	ret = -ETIMEDOUT;
out:
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	return ret;
}

int y2_spm_mfg_status(void)
{
	unsigned a, b;
	if (!smp_load_acquire(&spm_base)) return -EPROBE_DEFER;
	if (READ_ONCE(mfg_broken)) return -EIO;
	a = readl(spm_base + 0x60c) & BIT(4);
	b = readl(spm_base + 0x610) & BIT(4);
	return a != b ? -EIO : !!a;
}
static int y2_mfg_power_on(struct generic_pm_domain *domain)
{
	unsigned long flags;
	int ret;
	if (mfg_broken) return -EIO;
	/* Stock clkmgr enables the source before MTCMOS and keeps it running
	 * through SRAM/power acknowledgements. Lima separately owns G3D. */
	ret = clk_enable(mfg_source);
	if (ret) return ret;
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = y2_mfg_sequence(&spm_io, 1);
	/* Never expose partially powered hardware after an acknowledgement fault. */
	if (ret) mfg_broken = true;
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	/* Retain the source on an ambiguous power state; report the fault. */
	return ret;
}
static int y2_mfg_power_off(struct generic_pm_domain *domain)
{
	unsigned long flags;
	int ret;
	if (mfg_broken) return -EIO;
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = y2_mfg_sequence(&spm_io, 0);
	/* genpd retains its ON state on failure. Restore a usable ON state too;
	 * if restoration fails, refuse future GPU use, not a whole-system reset. */
	if (ret && y2_mfg_sequence(&spm_io, 1)) mfg_broken = true;
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	if (!ret) clk_disable(mfg_source);
	return ret;
}

static int y2_mfg_domain_register(struct platform_device *pdev)
{
	int ret, on = y2_spm_mfg_status();
	if (on < 0) return on;
	if (on) {
		ret = clk_enable(mfg_source);
		if (ret) return ret;
	}
	y2_mfg_domain.name = "y2-mfg";
	y2_mfg_domain.power_on = y2_mfg_power_on;
	y2_mfg_domain.power_off = y2_mfg_power_off;
	ret = pm_genpd_init(&y2_mfg_domain, NULL, !on);
	if (ret) goto out;
	ret = of_genpd_add_provider_simple(pdev->dev.of_node, &y2_mfg_domain);
	if (!ret) return 0;
	pm_genpd_remove(&y2_mfg_domain);
out:
	/* Keep an inherited ON domain's source if provider registration failed;
	 * the state cannot safely be handed to a GPU consumer in that case. */
	mfg_broken = true;
	return ret;
}

int y2_spm_radio_status(unsigned domain)
{
	unsigned bit;
	if (!smp_load_acquire(&spm_base)) return -EPROBE_DEFER;
	if (domain > 1) return -EINVAL;
	bit = domain ? 1 : 2;
	unsigned a = readl(spm_base + 0x60c) & bit, b = readl(spm_base + 0x610) & bit;
	return a != b ? -EIO : !!a;
}
int y2_spm_unstarted_md_off(void)
{
	unsigned long flags; int ret;
	/* Only the MD owner may call this after proving LK left all remaps,
	 * CCIF control/busy/pending and the MD boot-enable register zero.
	 * No firmware or bus master has been started in that state. The
	 * retained MT6582 path can lack protection ACK until MD has run. */
	if (!smp_load_acquire(&spm_base)) return -EPROBE_DEFER;
	ret = y2_ccf_radio_protect(1, true);
	if (ret && ret != -ETIMEDOUT) return ret;
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = y2_radio_domain_sequence(&spm_io, 1, 0);
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	return ret;
}
int y2_spm_radio_power(unsigned domain, int on)
{
	unsigned long flags; int ret;
	if (domain > 1) return -EINVAL;
	if (!smp_load_acquire(&spm_base)) return -EPROBE_DEFER;
	if (!on) {
		ret = y2_ccf_radio_protect(domain, true);
		if (ret) return ret;
	}
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = y2_radio_domain_sequence(&spm_io, domain, on);
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	if (!ret && on) ret = y2_ccf_radio_protect(domain, false);
	return ret;
}

int y2_spm_cpu_disable(unsigned cpu)
{
	if (!cpu) return -EPERM;
	return cpu < 4 && smp_load_acquire(&spm_base) && !READ_ONCE(spm_broken) ? 0 : -EOPNOTSUPP;
}
int y2_spm_cpu_kill(unsigned cpu)
{
	unsigned long flags;
	int ret;
	if (y2_spm_cpu_disable(cpu)) return 0;
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = y2_spm_cpu_power(&spm_io, cpu, 0);
	if (ret) WRITE_ONCE(spm_broken, true);
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	pr_info("Y2SPM: CPU%u power-off result=%d\n", cpu, ret);
	return !ret;
}
int y2_spm_cpu_boot(unsigned cpu, unsigned long entry)
{
	unsigned long flags;
	unsigned bit;
	int ret;
	/* Initial secondary boot precedes platform probing and uses LK's already
	 * powered cores. Only subsequent hotplug transitions use MTCMOS. */
	if (!smp_load_acquire(&spm_base)) return 0;
	if (cpu < 1 || cpu > 3 || READ_ONCE(spm_broken)) return -EIO;
	bit = y2_spm_cpu_status_mask(cpu);
	raw_spin_lock_irqsave(&spm_lock, flags);
	if ((spm_read(spm_base, 0x60c) & bit) && (spm_read(spm_base, 0x610) & bit)) ret = 0;
	else {
		ret = y2_ccf_boot_vector(entry);
		if (!ret) ret = y2_spm_cpu_power(&spm_io, cpu, 1);
		if (ret) WRITE_ONCE(spm_broken, true);
	}
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	return ret;
}

static int y2_spm_finish(unsigned long unused)
{
	/* Linux has saved its architectural/MMU/VFP/GIC context. Flush all
	 * cache levels before the PCM may remove CPU0 and cluster power. */
	v7_exit_coherency_flush(all);
	asm volatile("clrex\n\tdsb sy\n\twfi" : : : "memory");
	/* A pending interrupt can abort WFI before power removal. Restore SMP
	 * and cache access before returning to Linux's suspend-abort path. */
	asm volatile("mrc p15, 0, r0, c1, c0, 1\n\t"
		"orr r0, r0, #64\n\tmcr p15, 0, r0, c1, c0, 1\n\tisb\n\t"
		"mrc p15, 0, r0, c1, c0, 0\n\t"
		"orr r0, r0, #4\n\tmcr p15, 0, r0, c1, c0, 0\n\tisb"
		: : : "r0", "memory");
	return -EBUSY;
}
static int y2_spm_prepare_suspend(void)
{
	if (!smp_load_acquire(&spm_base) || READ_ONCE(spm_broken)) return -EIO;
	if (y2_suspend_disabled() || !y2_cirq_ready()) return -EOPNOTSUPP;
	return y2_pmic_spm_prepare();
}
static int y2_spm_enter(suspend_state_t state)
{
	unsigned long flags;
	unsigned biu;
	u64 restore_start = 0;
	int ret;
	if (state != PM_SUSPEND_MEM || num_online_cpus() != 1 ||
	    smp_processor_id() != 0 || READ_ONCE(spm_broken)) return -EBUSY;
	raw_spin_lock_irqsave(&spm_lock, flags);
	entries++;
	ret = y2_ccf_boot_vector(__pa_symbol(cpu_resume_arm));
	if (ret) goto unlock;
	ret = y2_cirq_begin();
	if (ret) goto unlock;
	biu = readl(spm_biu); /* actual Y2 MCU_BIU_CONTROL retention context */
	ret = cpu_pm_enter();
	if (ret) goto exit_cirq;
	ret = cpu_cluster_pm_enter();
	if (ret) goto exit_cpu;
	ret = y2_spm_suspend_arm(&spm_io, pcm_address);
	if (!ret) {
		ret = cpu_suspend(0, y2_spm_finish);
		restore_start = ktime_get_mono_fast_ns();
		writel(biu, spm_biu);
		last_wake = spm_read(spm_base, SPM_PCM_REG9_DATA);
		last_ticks = spm_read(spm_base, SPM_PCM_TIMER_OUT);
		last_debug = spm_read(spm_base, SPM_PCM_REG_DATA_INI);
		last_event = spm_read(spm_base, SPM_PCM_EVENT_REG_STA);
		last_r13 = spm_read(spm_base, SPM_PCM_REG13_DATA);
		if (!ret) resumes++; /* only return through real cpu_resume counts */
		if (last_debug) { WRITE_ONCE(spm_broken, true); ret = -EIO; }
	}
	y2_spm_suspend_clean(&spm_io);
	y2_spm_normal(&spm_io, normal_address);
	cpu_cluster_pm_exit();
exit_cpu:
	cpu_pm_exit();
exit_cirq:
	y2_cirq_end();
unlock:
	if (ret) aborts++;
	last_result = ret;
	if (restore_start) resume_restore_ns = ktime_get_mono_fast_ns() - restore_start;
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	pr_info("Y2SPM: suspend result=%d resumes=%u wake=%#x ticks32k=%u event=%#x debug=%#x r13=%#x\n",
		ret, resumes, last_wake, last_ticks, last_event, last_debug, last_r13);
	return ret;
}
/* Runtime DORMANT uses its own stock PCM and leaves infrastructure/DDRPHY
 * powered. Only CPU0 with CPUs1-3 physically off can execute this program.
 * No automatic hot-unplug is used merely to force entry. */
int y2_spm_dormant_idle(void)
{
	unsigned power, biu, cache;
	int ret, restore;
	if (y2_deep_idle_disabled() || !y2_local_timer_ready() ||
	    !smp_load_acquire(&spm_base) || !y2_cirq_ready()) return -ENODEV;
	if (num_online_cpus() != 1 || smp_processor_id()) return -EBUSY;
	if (READ_ONCE(spm_broken) || READ_ONCE(dormant_broken)) return -EIO;
	/* Replaces Android early-suspend voltage admission. Low OPPs share the
	 * source-backed 1.15 V floor; CCF lock excludes an in-flight transition. */
	if (cpufreq_quick_get(0) > 747500) return -EBUSY;
	if (!raw_spin_trylock(&spm_lock)) return -EBUSY;
	dormant_stage = "domains";
	power = spm_read(spm_base, SPM_PWR_STATUS) | spm_read(spm_base, SPM_PWR_STATUS_S);
	/* Exact subsystem power bits from mt_clkmgr. Requiring these domains off
	 * is stricter than vendor clock masks and avoids reads of powered-off MMIO. */
	if (power & (Y2_SPM_SECONDARY_CPU_MASK | BIT(0) | BIT(1) | BIT(3) |
		     BIT(4) | BIT(5) | BIT(7))) { ret = -EBUSY; goto unlock_idle; }
	ret = y2_ccf_boot_vector(__pa_symbol(cpu_resume_arm));
	if (ret) goto unlock_idle;
	dormant_stage = "clocks";
	ret = y2_ccf_deep_idle_begin();
	if (ret) goto unlock_idle;
	dormant_stage = "cirq";
	ret = y2_cirq_begin();
	if (ret) goto restore_clocks;
	biu = readl(spm_biu);
	cache = readl(spm_cache);
	ret = cpu_pm_enter();
	if (ret) goto restore_cirq;
	ret = cpu_cluster_pm_enter();
	if (ret) goto restore_cpu;
	/* MT6582 retains L2 in DORMANT: suppress reset invalidation, restore on
	 * both reset-resume and aborted WFI. Linux owns CP15/MMU, VFP and GIC. */
	writel(cache | BIT(4), spm_cache);
	dsb(sy);
	if (readl(spm_cache) != (cache | BIT(4))) { ret = -EIO; goto restore_cluster; }
	dormant_stage = "uart_pcm";
	ret = y2_spm_idle_arm(&spm_io, idle_address);
	if (!ret) {
		dormant_stage = "cpu_suspend";
		dormant_entries++;
		ret = cpu_suspend(0, y2_spm_finish);
		writel(biu, spm_biu);
		last_wake = spm_read(spm_base, SPM_PCM_REG9_DATA);
		last_debug = spm_read(spm_base, SPM_PCM_REG_DATA_INI);
		if (!ret) dormant_resumes++;
		if (last_debug) ret = -EIO;
	}
	y2_spm_suspend_clean(&spm_io);
	y2_spm_normal(&spm_io, normal_address);
restore_cluster:
	writel(cache, spm_cache);
	dsb(sy);
	if (readl(spm_cache) != cache) ret = -EIO;
	cpu_cluster_pm_exit();
restore_cpu:
	cpu_pm_exit();
restore_cirq:
	y2_cirq_end();
restore_clocks:
	restore = y2_ccf_deep_idle_end();
	if (restore) ret = restore;
unlock_idle:
	if (!ret) dormant_stage = "resumed";
	dormant_result = ret;
	if (ret) dormant_aborts++;
	if (ret == -EIO || ret == -ETIMEDOUT) {
		dormant_failures++;
		WRITE_ONCE(dormant_broken, true);
	}
	raw_spin_unlock(&spm_lock);
	return ret;
}
static int y2_suspend_valid(suspend_state_t state)
{
	return !y2_suspend_disabled() && suspend_valid_only_mem(state);
}
static const struct platform_suspend_ops y2_suspend_ops = {
	.valid = y2_suspend_valid,
	.prepare = y2_spm_prepare_suspend,
	.enter = y2_spm_enter,
};
static irqreturn_t y2_spm_irq(int irq, void *context)
{
	unsigned long flags;
	unsigned status;
	raw_spin_lock_irqsave(&spm_lock, flags);
	status = spm_read(context, SPM_SLEEP_ISR_STATUS);
	spm_write(context, SPM_SLEEP_ISR_MASK, ISRM_ALL);
	spm_write(context, SPM_SLEEP_ISR_STATUS, status);
	spm_write(context, SPM_PCM_SW_INT_CLEAR, PCM_SW_INT0);
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	return IRQ_HANDLED;
}
static ssize_t state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	unsigned long flags;
	ssize_t ret;
	raw_spin_lock_irqsave(&spm_lock, flags);
	ret = sysfs_emit(buf, "mode=SPM_CPU_SHUTDOWN_INFRA_RETAINED broken=%u entries=%u resumes=%u aborts=%u result=%d wake=%#x ticks32k=%u event=%#x debug=%#x r13=%#x power=%#x/%#x\n",
		spm_broken, entries, resumes, aborts, last_result, last_wake, last_ticks,
		last_event, last_debug, last_r13, spm_read(spm_base, 0x60c), spm_read(spm_base, 0x610));
	ret += sysfs_emit_at(buf, ret, "dormant_entries=%u dormant_resumes=%u dormant_aborts=%u dormant_failures=%u dormant_result=%d dormant_broken=%u\n",
		dormant_entries, dormant_resumes, dormant_aborts, dormant_failures, dormant_result, dormant_broken);
	ret += sysfs_emit_at(buf, ret, "dormant_stage=%s resume_restore_ns=%llu\n", dormant_stage, resume_restore_ns);
	raw_spin_unlock_irqrestore(&spm_lock, flags);
	return ret;
}
static DEVICE_ATTR_RO(state);
static struct attribute *y2_spm_attrs[] = { &dev_attr_state.attr, NULL };
static const struct attribute_group y2_spm_group = { .attrs = y2_spm_attrs };

static int y2_spm_probe(struct platform_device *pdev)
{
	void __iomem *base;
	void *pcm;
	int irq, ret;
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	/* Prepare outside the domain's noirq callbacks; enable only around MFG. */
	mfg_source = devm_clk_get_prepared(&pdev->dev, "mfg");
	if (IS_ERR(mfg_source)) return PTR_ERR(mfg_source);
	base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(base)) return PTR_ERR(base);
	spm_biu = devm_platform_ioremap_resource(pdev, 1);
	if (IS_ERR(spm_biu)) return PTR_ERR(spm_biu);
	spm_cache = devm_platform_ioremap_resource(pdev, 2);
	if (IS_ERR(spm_cache)) return PTR_ERR(spm_cache);
	ret = dma_set_mask_and_coherent(&pdev->dev, DMA_BIT_MASK(32));
	if (ret) return ret;
	pcm = dmam_alloc_coherent(&pdev->dev, sizeof(y2_pcm_suspend) + sizeof(y2_pcm_normal) + sizeof(y2_pcm_dpidle),
		&pcm_address, GFP_KERNEL);
	if (!pcm) return -ENOMEM;
	memcpy(pcm, y2_pcm_suspend, sizeof(y2_pcm_suspend));
	memcpy(pcm + sizeof(y2_pcm_suspend), y2_pcm_normal, sizeof(y2_pcm_normal));
	normal_address = pcm_address + sizeof(y2_pcm_suspend);
	idle_address = normal_address + sizeof(y2_pcm_normal);
	memcpy(pcm + sizeof(y2_pcm_suspend) + sizeof(y2_pcm_normal), y2_pcm_dpidle, sizeof(y2_pcm_dpidle));
	dma_wmb();
	/* Mask inherited pending SPM interrupts before requesting this sole
	 * owner. PMIC/Power/RTC level wake remains owned by their IRQ hierarchy. */
	spm_write(base, SPM_POWERON_CONFIG_SET, 0x0b160001);
	spm_write(base, SPM_SLEEP_ISR_MASK, ISRM_ALL);
	spm_write(base, SPM_SLEEP_ISR_STATUS, ISRC_ALL);
	irq = platform_get_irq(pdev, 0);
	if (irq < 0) return irq;
	ret = devm_request_irq(&pdev->dev, irq, y2_spm_irq, IRQF_NO_SUSPEND, "y2-spm", base);
	if (ret) return ret;
	spm_io.context = base;
	y2_spm_normal(&spm_io, normal_address);
	/* Publish only after firmware and the shared IO context are ready. */
	smp_store_release(&spm_base, base);
	ret = devm_device_add_group(&pdev->dev, &y2_spm_group);
	if (ret) { WRITE_ONCE(spm_base, NULL); return ret; }
	/* Same SPM mapping/lock as CPU, radio and deep suspend. The MFG reset
	 * is intrinsic to the stock domain sequence, not an unrelated RGU bit. */
	ret = y2_mfg_domain_register(pdev);
	/* A GPU provider failure must not unmap the shared, published SPM owner
	 * used by CPU/radios or remove the established M4 suspend path. */
	if (ret) {
		WRITE_ONCE(mfg_broken, true);
		dev_err(&pdev->dev, "MFG domain unavailable: %d\n", ret);
	}
	suspend_set_ops(&y2_suspend_ops);
	y2_cpu_dvfs_ready();
	dev_info(&pdev->dev, "CPU1-3 MTCMOS and SPM CPU/cluster shutdown, infrastructure retained; power=%#x/%#x\n",
		spm_read(base, 0x60c), spm_read(base, 0x610));
	return 0;
}
static const struct of_device_id y2_spm_match[] = { { .compatible = "innioasis,y2-spm" }, {} };
static struct platform_driver y2_spm_driver = {
	.probe = y2_spm_probe,
	.driver = { .name = "y2-spm", .of_match_table = y2_spm_match, .suppress_bind_attrs = true },
};
module_platform_driver(y2_spm_driver);
MODULE_LICENSE("GPL");
