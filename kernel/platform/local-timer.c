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
#include "timer-policy.h"
#include "idle-completion-policy.h"
#include <linux/of_address.h>
#include <linux/smp.h>
#include <linux/cpu_pm.h>
#include <linux/percpu.h>
#include <linux/module.h>
#include <asm/arch_timer.h>

static bool y2_timer_ready, events_ready;
static int registration_error;
module_param(events_ready, bool, 0400);
module_param(registration_error, int, 0400);
static bool y2_timer_disabled;
static void __iomem *timer_base;
static char broadcast_admission[64] = "gpt1_foundation_fallback";
module_param_string(broadcast_admission, broadcast_admission, sizeof(broadcast_admission), 0400);
static char admission[64] = "not_attempted";
static unsigned pre_control, pre_clock, pre_irq, pre_gpt4_control, pre_gpt4_clock;
static unsigned reference_ticks, gpt6_ticks;
static unsigned long long counter_ticks;
static bool counter_adopted;
module_param_string(admission, admission, sizeof(admission), 0400);
module_param(pre_control, uint, 0400);
module_param(pre_clock, uint, 0400);
module_param(pre_irq, uint, 0400);
module_param(pre_gpt4_control, uint, 0400);
module_param(pre_gpt4_clock, uint, 0400);
module_param(reference_ticks, uint, 0400);
module_param(gpt6_ticks, uint, 0400);
module_param(counter_ticks, ullong, 0400);
module_param(counter_adopted, bool, 0400);
module_param_named(ready, y2_timer_ready, bool, 0400);
static DEFINE_PER_CPU(unsigned, verified_cntfrq);
static DEFINE_PER_CPU(int, cpu_admission_error);
static unsigned handoff_checks, handoff_rejects, handoff_count, handoff_compare;
static unsigned handoff_control, handoff_clock, handoff_irq, handoff_pending;
static unsigned long long handoff_remaining_ns;
static unsigned context_saves, context_restores, context_failures;
module_param(handoff_checks, uint, 0400);
module_param(handoff_rejects, uint, 0400);
module_param(handoff_count, uint, 0400);
module_param(handoff_compare, uint, 0400);
module_param(handoff_control, uint, 0400);
module_param(handoff_clock, uint, 0400);
module_param(handoff_irq, uint, 0400);
module_param(handoff_pending, uint, 0400);
module_param(handoff_remaining_ns, ullong, 0400);
module_param(context_saves, uint, 0400);
module_param(context_restores, uint, 0400);
module_param(context_failures, uint, 0400);

int y2_local_timer_dormant_check(void)
{
	if (!timer_base || !y2_local_events_ready() ||
	    strcmp(broadcast_admission, "gpt4_sole_owner")) return -ENODEV;
	handoff_checks++;
	handoff_control = readl(timer_base + 0x40);
	handoff_clock = readl(timer_base + 0x44);
	handoff_irq = readl(timer_base);
	handoff_pending = readl(timer_base + 4);
	handoff_compare = readl(timer_base + 0x4c);
	/* Read the advancing count last, making the deadline check conservative. */
	handoff_count = readl(timer_base + 0x48);
	handoff_remaining_ns = handoff_compare > handoff_count ?
		y2_gpt_ticks_ns(handoff_compare - handoff_count) : 0;
	if (!y2_dormant_gpt_ready(handoff_control, handoff_clock, handoff_irq,
			handoff_pending, handoff_count, handoff_compare)) {
		handoff_rejects++;
		return -ETIME;
	}
	return 0;
}
bool y2_local_timer_context_ok(void) { return !READ_ONCE(context_failures); }


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

bool y2_local_events_ready(void) { return READ_ONCE(events_ready); }
void y2_local_timer_registration(int result)
{
	registration_error = result;
	events_ready = !result;
	pr_info("Y2TIMER: PPI29 registration result=%d, events_ready=%u\n", result, events_ready);
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
	this_cpu_write(verified_cntfrq, observed);
	if (observed != frequency) {
		this_cpu_write(cpu_admission_error, -EIO);
		pr_err("Y2TIMER: CPU%u CNTFRQ requested=%u readback=%u\n",
			smp_processor_id(), frequency, observed);
		return -EIO;
	}
	start = y2_physical_count();
	for (n = 0; n < 10000; n++) {
		if (y2_physical_count() != start) {
			this_cpu_write(cpu_admission_error, 0);
			return 0;
		}
		cpu_relax();
	}
	this_cpu_write(cpu_admission_error, -ETIMEDOUT);
	pr_err("Y2TIMER: CPU%u physical counter stopped at %llu\n", smp_processor_id(), start);
	return -ETIMEDOUT;
}

bool __init y2_local_timer_resource(struct device_node *node)
{
	struct resource res;
	if (!of_machine_is_compatible("innioasis,y2")) return true;
	if (of_address_to_resource(node, 0, &res) || res.start != 0x10008000 ||
	    resource_size(&res) < 0x80) {
		strscpy(admission, "dt_gpt_resource_identity", sizeof(admission));
		pr_err("Y2TIMER: wrong GPT DT resource: %pOF\n", node);
		return false;
	}
	return true;
}
void __init y2_local_timer_prepare(void __iomem *gpt, unsigned long rate)
{
	u32 reference, delta = 0, pfr1, gpt6_begin, gpt6_delta;
	u64 begin, elapsed;
	unsigned int tries;
	const char *reason;
	bool changed = false;

	timer_base = gpt;
	pre_control = readl(gpt + 0x60); pre_clock = readl(gpt + 0x64);
	pre_irq = readl(gpt); pre_gpt4_control = readl(gpt + 0x40);
	pre_gpt4_clock = readl(gpt + 0x44);
	if (!IS_ENABLED(CONFIG_ARM_ARCH_TIMER) || y2_timer_disabled || y2_cpu_safe()) {
		strscpy(admission, "disabled_by_config_or_boot_policy", sizeof(admission));
		return;
	}
	if (!of_machine_is_compatible("innioasis,y2")) return;
	asm volatile("mrc p15, 0, %0, c0, c1, 1" : "=r" (pfr1));
	reason = y2_gpt6_predicate(rate, pfr1, pre_control, pre_clock);
	pr_info("Y2TIMER: inherited rate=%lu PFR1=%#x GPT6=%#x/%#x GPT4=%#x/%#x IRQ=%#x\n",
		rate, pfr1, pre_control, pre_clock, pre_gpt4_control, pre_gpt4_clock, pre_irq);
	if (reason) goto rejected;
	/* This is the sole DT GPT owner, before any CNTP event is exposed. Only
	 * GPT6's inherited IRQ is masked; GPT4 belongs to the broadcast owner.
	 * A legitimate running counter is never reset or briefly stopped. */
	writel(pre_irq & ~BIT(5), gpt);
	if (readl(gpt) & BIT(5)) {
		reason = "gpt6_irq_mask_readback"; goto fallback;
	}
	counter_adopted = y2_gpt6_adopt(pre_control, pre_clock);
	if (!counter_adopted) {
		writel(pre_control & ~1U, gpt + 0x60);
		writel(0, gpt + 0x64);
		writel(0x31, gpt + 0x60);
		changed = true;
	}
	if (!y2_gpt6_adopt(readl(gpt + 0x60), readl(gpt + 0x64))) {
		reason = "gpt6_control_clock_readback"; goto fallback;
	}
	if ((readl(gpt + 0x20) & ~2U) != 0x31 || readl(gpt + 0x24)) {
		reason = "gpt2_reference_control_clock"; goto fallback;
	}
	gpt6_begin = readl(gpt + 0x68);
	reference = readl(gpt + 0x28);
	begin = y2_physical_count();
	for (tries = 0; tries < 100000; tries++) {
		delta = readl(gpt + 0x28) - reference;
		if (delta >= 26000) break;
		cpu_relax();
	}
	elapsed = y2_physical_count() - begin;
	gpt6_delta = readl(gpt + 0x68) - gpt6_begin;
	reference_ticks = delta; counter_ticks = elapsed; gpt6_ticks = gpt6_delta;
	if (tries == 100000) { reason = "gpt2_reference_timeout"; goto fallback; }
	if (!y2_gpt_rate_matches(delta, gpt6_delta)) {
		reason = "gpt6_gpt2_rate_mismatch"; goto fallback;
	}
	if (!y2_gpt_rate_matches(delta, elapsed)) {
		reason = "cntp_gpt2_rate_mismatch"; goto fallback;
	}
	y2_timer_ready = true;
	strscpy(admission, counter_adopted ? "adopted_13mhz" : "prepared_13mhz", sizeof(admission));
	pr_info("Y2TIMER: %s GPT2=%u GPT6=%u CNTP=%llu, GPT4 inherited independently\n", admission, delta, gpt6_delta, elapsed);
	return;
fallback:
	if (changed) {
		writel(readl(gpt + 0x60) & ~1U, gpt + 0x60);
		writel(pre_clock, gpt + 0x64);
		writel(pre_control, gpt + 0x60);
	}
	writel(pre_irq, gpt);
rejected:
	strscpy(admission, reason, sizeof(admission));
	pr_warn("Y2TIMER: admission=%s GPT6=%#x/%#x GPT2=%#x/%#x IRQ=%#x reference=%u gpt6=%u counter=%llu; legacy fallback\n",
		admission, readl(gpt + 0x60), readl(gpt + 0x64), readl(gpt + 0x20),
		readl(gpt + 0x24), readl(gpt), reference_ticks, gpt6_ticks, counter_ticks);
}
bool __init y2_local_timer_broadcast_prepare(void __iomem *gpt)
{
	u32 control = readl(gpt + 0x40), clock = readl(gpt + 0x44);
	u32 irq = readl(gpt), compare = readl(gpt + 0x4c);
	if ((control & ~0x33U) || (clock & ~0x1fU)) {
		strscpy(broadcast_admission, "gpt4_reserved_bits", sizeof(broadcast_admission));
		pr_warn("Y2TIMER: GPT4 reserved bits ctrl=%#x clock=%#x; GPT1 broadcast fallback\n", control, clock);
		return false;
	}
	/* Firmware GPT4 is reclaimed by this same GPT owner, not a second IRQ
	 * or resource claimant. GPT1 is retired only after GPT4 readback passes. */
	writel(irq & ~BIT(3), gpt);
	writel(0, gpt + 0x40); writel(0, gpt + 0x44); writel(0, gpt + 0x4c);
	if (readl(gpt + 0x40) || readl(gpt + 0x44) || readl(gpt + 0x4c)) {
		writel(clock, gpt + 0x44); writel(compare, gpt + 0x4c);
		writel(control, gpt + 0x40); writel(irq, gpt);
		strscpy(broadcast_admission, "gpt4_readback_failed", sizeof(broadcast_admission));
		pr_warn("Y2TIMER: GPT4 prepare readback failed; GPT1 broadcast fallback\n");
		return false;
	}
	writel(readl(gpt + 0x10) & ~1U, gpt + 0x10);
	writel(readl(gpt) & ~BIT(0), gpt);
	writel(BIT(0) | BIT(3), gpt + 8);
	strscpy(broadcast_admission, "gpt4_sole_owner", sizeof(broadcast_admission));
	pr_info("Y2TIMER: GPT4 sole broadcast owner, GPT1 stopped/masked\n");
	return true;
}
static int timer_registers_get(char *buf, const struct kernel_param *kp)
{
	if (!timer_base) return sysfs_emit(buf, "unmapped\n");
	return sysfs_emit(buf, "gpt6_control=%#x gpt6_clock=%#x gpt6_low=%#x gpt6_high=%#x gpt4_control=%#x gpt4_clock=%#x irq_enable=%#x irq_status=%#x\n",
		readl(timer_base + 0x60), readl(timer_base + 0x64), readl(timer_base + 0x68),
		readl(timer_base + 0x78), readl(timer_base + 0x40), readl(timer_base + 0x44),
		readl(timer_base), readl(timer_base + 4));
}
static const struct kernel_param_ops registers_ops = { .get = timer_registers_get };
module_param_cb(registers, &registers_ops, NULL, 0400);
static int timer_cpus_get(char *buf, const struct kernel_param *kp)
{
	unsigned cpu; int n = 0;
	for_each_possible_cpu(cpu)
		n += sysfs_emit_at(buf, n, "cpu%u_cntfrq=%u cpu%u_error=%d ", cpu,
			per_cpu(verified_cntfrq, cpu), cpu, per_cpu(cpu_admission_error, cpu));
	n += sysfs_emit_at(buf, n, "\n");
	return n;
}
static const struct kernel_param_ops cpus_ops = { .get = timer_cpus_get };
module_param_cb(cpus, &cpus_ops, NULL, 0400);

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
struct y2_timer_context { u64 compare; u32 control, frequency; bool valid; };
static DEFINE_PER_CPU(struct y2_timer_context, timer_context);
static u32 y2_cntfrq_read(void)
{
	u32 value;
	asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (value));
	return value;
}
static void y2_cntfrq_write(u32 value)
{
	asm volatile("mcr p15, 0, %0, c14, c0, 0" : : "r" (value));
	isb();
}
static int y2_timer_cpu_pm(struct notifier_block *nb, unsigned long action, void *unused)
{
	struct y2_timer_context *ctx = this_cpu_ptr(&timer_context);
	if (!y2_local_timer_ready()) return NOTIFY_OK;
	if (action == CPU_PM_ENTER) {
		ctx->valid = false;
		ctx->compare = y2_cntp_compare();
		ctx->control = y2_cntp_control();
		ctx->frequency = y2_cntfrq_read();
		if (ctx->frequency != 13000000) return NOTIFY_BAD;
		ctx->valid = true;
		context_saves++;
	} else if (action == CPU_PM_EXIT || action == CPU_PM_ENTER_FAILED) {
		u32 frequency;
		/* ENTER_FAILED also reaches notifiers which did not save context. */
		if (!ctx->valid) return NOTIFY_OK;
		y2_cntp_set_control(0);
		y2_cntfrq_write(ctx->frequency);
		y2_cntp_set_compare(ctx->compare);
		y2_cntp_set_control(ctx->control);
		isb();
		frequency = y2_cntfrq_read();
		if (frequency != ctx->frequency || y2_cntp_compare() != ctx->compare ||
		    ((y2_cntp_control() ^ ctx->control) & 3)) context_failures++;
		context_restores++;
		ctx->valid = false;
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
