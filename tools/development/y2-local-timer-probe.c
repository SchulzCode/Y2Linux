// SPDX-License-Identifier: GPL-2.0-only
/* Temporary exact-Y2 diagnostic, never a resident clockevent driver.
 * Stock mt_gpt_init starts GPT6 (13 MHz, free-running) for the CP15 counter.
 * Only unused GPT6 control/clock and inactive per-CPU CNTP are touched.
 * GPT1/2 continue to own Linux time; no clocksource is switched. Original
 * controls/compare values are restored, but the unused counter advances.
 * Always returns -EAGAIN after cleanup. An external-module taint persists
 * until normal reboot. See the hardware-ceiling admission audit before use.
 */
#include <linux/cpu.h>
#include <linux/delay.h>
#include <linux/interrupt.h>
#include <linux/io.h>
#include <linux/irq.h>
#include <linux/irqdomain.h>
#include <linux/ktime.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/smp.h>

#if IS_ENABLED(CONFIG_ARM_ARCH_TIMER)
#error This probe requires the Physical 01 kernel without an architectural timer owner
#endif

struct local_sample {
	u64 saved_compare, start_count, delta_count, start_ns, delta_ns, irq_ns;
	u32 saved_control, irq_control, count;
	bool saved;
};
static struct local_sample __percpu *sample;
static unsigned int probe_irq;

static u64 counter(void)
{
	u32 lo, hi;
	isb();
	asm volatile("mrrc p15, 0, %0, %1, c14" : "=r" (lo), "=r" (hi));
	return ((u64)hi << 32) | lo;
}

static void control(u32 value)
{
	asm volatile("mcr p15, 0, %0, c14, c2, 1" :: "r" (value) : "memory");
	isb();
}

static void compare(u64 value)
{
	u32 lo = value, hi = value >> 32;
	asm volatile("mcrr p15, 2, %0, %1, c14" :: "r" (lo), "r" (hi) : "memory");
	isb();
}

static void save_local(void *unused)
{
	struct local_sample *s = this_cpu_ptr(sample);
	u32 lo, hi;
	asm volatile("mrc p15, 0, %0, c14, c2, 1" : "=r" (s->saved_control));
	asm volatile("mrrc p15, 2, %0, %1, c14" : "=r" (lo), "=r" (hi));
	s->saved_compare = ((u64)hi << 32) | lo;
	s->saved = true;
}

static void sample_begin(void *unused)
{
	struct local_sample *s = this_cpu_ptr(sample);
	s->start_count = counter();
	s->start_ns = ktime_get_ns();
}

static void sample_end(void *unused)
{
	struct local_sample *s = this_cpu_ptr(sample);
	s->delta_count = counter() - s->start_count;
	s->delta_ns = ktime_get_ns() - s->start_ns;
}

static irqreturn_t local_irq(int irq, void *context)
{
	struct local_sample *s = context;
	asm volatile("mrc p15, 0, %0, c14, c2, 1" : "=r" (s->irq_control));
	control(2); /* Mask/stop at the first interrupt, including an unexpected one. */
	s->irq_ns = ktime_get_ns();
	s->count++;
	return IRQ_HANDLED;
}

static void arm_local(void *unused)
{
	struct local_sample *s = this_cpu_ptr(sample);
	control(2);
	irq_set_irqchip_state(probe_irq, IRQCHIP_STATE_PENDING, false);
	s->count = 0;
	s->irq_control = 0;
	s->irq_ns = 0;
	enable_percpu_irq(probe_irq, IRQ_TYPE_LEVEL_HIGH);
	s->start_ns = ktime_get_ns();
	compare(counter() + 130000); /* 10 ms at the measured 13 MHz. */
	control(1);
}

static void restore_local(void *unused)
{
	struct local_sample *s = this_cpu_ptr(sample);
	control(2);
	disable_percpu_irq(probe_irq);
	irq_set_irqchip_state(probe_irq, IRQCHIP_STATE_PENDING, false);
	compare(s->saved_compare);
	control(s->saved_control);
}

static int __init y2_local_timer_probe_init(void)
{
	struct device_node *gic;
	struct irq_domain *domain;
	struct irq_fwspec spec = { .param_count = 3, .param = { 1, 13, 0xf04 } };
	void __iomem *gpt;
	u32 saved_gpt_control, saved_gpt_clock;
	unsigned int cpu, trial;
	bool mapped = false, requested = false, started = false;
	int result = -ENODEV;

	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	gic = of_find_compatible_node(NULL, NULL, "arm,cortex-a7-gic");
	if (!gic)
		return -ENODEV;
	spec.fwnode = of_node_to_fwnode(gic);
	domain = irq_find_matching_fwspec(&spec, DOMAIN_BUS_ANY);
	if (!domain)
		goto put_node;
	/* Never take over an existing PPI mapping. */
	if (irq_find_mapping(domain, 29)) {
		result = -EBUSY;
		goto put_node;
	}
	gpt = ioremap(0x10008000, 0x80);
	if (!gpt)
		goto put_node;
	saved_gpt_control = readl(gpt + 0x60);
	saved_gpt_clock = readl(gpt + 0x64);
	pr_info("Y2TIMER_PROBE: GPT6 control=%#x clock=%#x irq_enable=%#x\n",
		saved_gpt_control, saved_gpt_clock, readl(gpt));
	/* No takeover, no counter reset, no shared IRQ enable/ack writes. */
	if (saved_gpt_control || saved_gpt_clock || (readl(gpt) & BIT(5))) {
		result = -EBUSY;
		goto unmap;
	}
	sample = alloc_percpu(struct local_sample);
	if (!sample) {
		result = -ENOMEM;
		goto unmap;
	}
	cpus_read_lock();
	on_each_cpu(save_local, NULL, 1);
	for_each_online_cpu(cpu) {
		if (per_cpu_ptr(sample, cpu)->saved_control & 1) {
			result = -EBUSY;
			goto cleanup;
		}
	}
	writel(0x30, gpt + 0x60); /* Stock GPT_FREE_RUN, still stopped. */
	writel(0, gpt + 0x64);    /* Stock GPT_CLK_SRC_SYS / DIV_1. */
	writel(0x31, gpt + 0x60);
	started = true;
	if (readl(gpt + 0x60) != 0x31 || readl(gpt + 0x64)) {
		result = -EIO;
		goto cleanup;
	}
	for (trial = 0; trial < 3; trial++) {
		on_each_cpu(sample_begin, NULL, 1);
		msleep(50);
		on_each_cpu(sample_end, NULL, 1);
		for_each_online_cpu(cpu) {
			struct local_sample *s = per_cpu_ptr(sample, cpu);
			u64 hz = div64_u64(s->delta_count * 1000000000ULL, s->delta_ns);
			pr_info("Y2TIMER_PROBE: calibration trial=%u cpu=%u ticks=%llu ns=%llu hz=%llu\n",
				trial, cpu, s->delta_count, s->delta_ns, hz);
			if (hz < 12900000 || hz > 13100000) {
				result = -ERANGE;
				goto cleanup;
			}
		}
	}
	probe_irq = irq_create_fwspec_mapping(&spec);
	if (!probe_irq) {
		result = -EINVAL;
		goto cleanup;
	}
	mapped = true;
	result = request_percpu_irq(probe_irq, local_irq, "y2-timer-probe", sample);
	if (result)
		goto cleanup;
	requested = true;
	for (trial = 0; trial < 3; trial++) {
		on_each_cpu(arm_local, NULL, 1);
		msleep(100); /* GPT1/2 keep running even if this PPI never arrives. */
		on_each_cpu(restore_local, NULL, 1);
		for_each_online_cpu(cpu) {
			struct local_sample *s = per_cpu_ptr(sample, cpu);
			s64 delay = s->irq_ns ? s->irq_ns - s->start_ns : -1;
			pr_info("Y2TIMER_PROBE: ppi=29 trial=%u cpu=%u count=%u ctl=%#x delay_ns=%lld\n",
				trial, cpu, s->count, s->irq_control, delay);
			if (s->count != 1 || !(s->irq_control & 4) ||
			    delay < 5000000 || delay > 100000000)
				result = -ETIME;
		}
		if (result)
			break;
	}
cleanup:
	if (requested) {
		on_each_cpu(restore_local, NULL, 1);
		free_percpu_irq(probe_irq, sample);
	}
	if (mapped)
		irq_dispose_mapping(probe_irq);
	if (started) {
		writel(saved_gpt_control, gpt + 0x60);
		writel(saved_gpt_clock, gpt + 0x64);
		pr_info("Y2TIMER_PROBE: restored control=%#x clock=%#x result=%d\n",
			readl(gpt + 0x60), readl(gpt + 0x64), result);
	}
	cpus_read_unlock();
	free_percpu(sample);
unmap:
	iounmap(gpt);
put_node:
	of_node_put(gic);
	pr_info("Y2TIMER_PROBE: complete result=%d; deliberate EAGAIN, no resident module\n", result);
	return -EAGAIN;
}
module_init(y2_local_timer_probe_init);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Bounded stock-backed Y2 GPT6/physical PPI29 diagnostic");
