// SPDX-License-Identifier: GPL-2.0-only
/* Exact Y2 stock mt_gpt_init/ca7_timer contract, physically measured by the
 * bounded GPT6/PPI29 diagnostic. GPT1/2 and their interrupt bits are untouched.
 * Preparation runs through the existing GPT register owner before ARM timer
 * discovery. A failed counter check leaves the original GPT/dummy fallback.
 */
#include <linux/kernel.h>
#include <linux/math64.h>
#include <linux/of.h>
#include "local-timer.h"
#include "cpu-options.h"
#include <linux/cpu_pm.h>
#include <linux/percpu.h>
#include <linux/module.h>
#include <asm/arch_timer.h>

static bool y2_timer_ready;
static bool y2_timer_disabled;

static int __init y2_timer_option(char *value)
{
	if (!strcmp(value, "off"))
		y2_timer_disabled = true;
	else if (strcmp(value, "on")) {
		y2_timer_disabled = true;
		pr_warn("Y2TIMER: invalid option, retaining GPT fallback\n");
	}
	return 1;
}
__setup("y2.local_timer=", y2_timer_option);

bool y2_local_timer_ready(void)
{
	return y2_timer_ready;
}

static u64 y2_physical_count(void)
{
	u32 low, high;
	isb();
	asm volatile("mrrc p15, 0, %0, %1, c14" : "=r" (low), "=r" (high));
	return ((u64)high << 32) | low;
}

int y2_local_timer_cpu_init(void)
{
	u64 start;
	u32 frequency = 13000000, observed;
	unsigned n;
	if (!y2_local_timer_ready()) return -ENODEV;
	/* Retained generic_timer_setup writes CNTFRQ on every CPU. Linux's DT
	 * rate alone does not configure this reset-lost CP15 register. */
	asm volatile("mcr p15, 0, %0, c14, c0, 0" : : "r" (frequency));
	isb();
	asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (observed));
	if (observed != frequency) return -EIO;
	start = y2_physical_count();
	for (n = 0; n < 10000; n++) {
		if (y2_physical_count() != start) return 0;
		cpu_relax();
	}
	return -ETIMEDOUT;
}

void __init y2_local_timer_prepare(void __iomem *gpt, unsigned long rate)
{
	u32 saved, clock, reference, delta = 0, pfr1;
	u64 begin, elapsed;
	unsigned int tries;

	if (!IS_ENABLED(CONFIG_ARM_ARCH_TIMER) ||
	    !of_machine_is_compatible("innioasis,y2") || y2_timer_disabled || y2_cpu_safe())
		return;
	asm volatile("mrc p15, 0, %0, c0, c1, 1" : "=r" (pfr1));
	saved = readl(gpt + 0x60);
	clock = readl(gpt + 0x64);
	if (rate != 13000000 || ((pfr1 >> 16) & 15) != 1 ||
	    saved || clock || (readl(gpt) & (BIT(5) | BIT(3))) ||
	    readl(gpt + 0x40) || readl(gpt + 0x44)) {
		pr_warn("Y2TIMER: GPT6 preflight rejected, retaining GPT fallback\n");
		return;
	}
	/* Original stock GPT_FREE_RUN / SYS / DIV1, no reset or GPT IRQ. */
	writel(0x30, gpt + 0x60);
	writel(0, gpt + 0x64);
	writel(0x31, gpt + 0x60);
	if (readl(gpt + 0x60) != 0x31 || readl(gpt + 0x64))
		goto fallback;
	reference = readl(gpt + 0x28); /* Existing 13 MHz GPT2 clocksource. */
	begin = y2_physical_count();
	/* Early boot: do not depend on interrupts, jiffies or calibrated udelay. */
	for (tries = 0; tries < 100000; tries++) {
		delta = readl(gpt + 0x28) - reference;
		if (delta >= 26000)
			break;
		cpu_relax();
	}
	elapsed = y2_physical_count() - begin;
	if (tries == 100000 || elapsed < div_u64(delta * 98ULL, 100) ||
	    elapsed > div_u64(delta * 102ULL, 100))
		goto fallback;
	y2_timer_ready = true;
	pr_info("Y2TIMER: GPT6 physical counter ready at 13000000 Hz, reference=%u counter=%llu\n",
		delta, elapsed);
	return;
fallback:
	writel(saved, gpt + 0x60);
	writel(clock, gpt + 0x64);
	pr_warn("Y2TIMER: counter validation failed, retaining GPT fallback\n");
}

/* arm_arch_timer CPU PM saves CNTKCTL only. CPU power removal also loses
 * CNTP compare/control and CNTFRQ; save them here, once, on the affected CPU.
 * The tick broadcast core owns deadline migration/reprogramming. */
static u64 y2_cntp_compare(void)
{
	u32 lo, hi;
	asm volatile("mrrc p15, 2, %0, %1, c14" : "=r" (lo), "=r" (hi));
	return ((u64)hi << 32) | lo;
}
static u32 y2_cntp_control(void)
{
	u32 value;
	asm volatile("mrc p15, 0, %0, c14, c2, 1" : "=r" (value));
	return value;
}
static void y2_cntp_set_control(u32 value)
{
	asm volatile("mcr p15, 0, %0, c14, c2, 1" : : "r" (value));
	isb();
}
static void y2_cntp_set_compare(u64 value)
{
	asm volatile("mcrr p15, 2, %0, %1, c14" : : "r" ((u32)value), "r" ((u32)(value >> 32)));
	isb();
}
struct y2_timer_context { u64 compare; u32 control, frequency; };
static DEFINE_PER_CPU(struct y2_timer_context, timer_context);
static int y2_timer_cpu_pm(struct notifier_block *nb, unsigned long action, void *unused)
{
	struct y2_timer_context *ctx = this_cpu_ptr(&timer_context);
	if (!y2_local_timer_ready()) return NOTIFY_OK;
	if (action == CPU_PM_ENTER) {
		ctx->compare = y2_cntp_compare();
		ctx->control = y2_cntp_control();
		asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (ctx->frequency));
	} else if (action == CPU_PM_EXIT || action == CPU_PM_ENTER_FAILED) {
		y2_cntp_set_control(0);
		asm volatile("mcr p15, 0, %0, c14, c0, 0" : : "r" (ctx->frequency));
		y2_cntp_set_compare(ctx->compare);
		y2_cntp_set_control(ctx->control);
		isb();
	}
	return NOTIFY_OK;
}
static struct notifier_block y2_timer_pm = { .notifier_call = y2_timer_cpu_pm };
static int __init y2_timer_pm_init(void)
{
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	return cpu_pm_register_notifier(&y2_timer_pm);
}
subsys_initcall(y2_timer_pm_init);
