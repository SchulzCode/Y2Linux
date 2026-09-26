// SPDX-License-Identifier: GPL-2.0-only
/* Bounded observation on the exact Y2: no CP15, MMIO or loader-RAM writes.
 * Always fails init deliberately, so no module remains installed. Loading this
 * external module sets the O taint; a fresh normal boot is required afterwards.
 * Do not use its measurements as clockevent/PPI/wake qualification.
 */
#include <linux/cpu.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/ktime.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/smp.h>

struct timer_sample {
	u64 count, ns;
	u32 frequency, kernel_control, physical_control, virtual_control, pfr1;
};
static struct timer_sample samples[2][4];

static void snapshot_timer(void *arg)
{
	unsigned int cpu = smp_processor_id(), phase = *(unsigned int *)arg;
	struct timer_sample *s;
	u32 lo, hi;

	if (cpu >= 4)
		return;
	s = &samples[phase][cpu];
	asm volatile("mrc p15, 0, %0, c0, c1, 1" : "=r" (s->pfr1));
	if (((s->pfr1 >> 16) & 0xf) != 1)
		return;
	asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (s->frequency));
	asm volatile("mrc p15, 0, %0, c14, c1, 0" : "=r" (s->kernel_control));
	asm volatile("mrc p15, 0, %0, c14, c2, 1" : "=r" (s->physical_control));
	asm volatile("mrc p15, 0, %0, c14, c3, 1" : "=r" (s->virtual_control));
	isb();
	asm volatile("mrrc p15, 0, %0, %1, c14" : "=r" (lo), "=r" (hi));
	s->count = ((u64)hi << 32) | lo;
	s->ns = ktime_get_ns();
}

static int snapshot_bin(void)
{
	/* Same retained and reserved LK ATAG page as the earlier GPU-bin receipt.
	 * Print only the five CPU-bin bits, not identifiers or calibration words.
	 */
	const u32 *p = phys_to_virt(0x80000100);
	unsigned int off = 0, limit = 0x3f00 / sizeof(*p);

	while (off + 2 <= limit) {
		u32 words = p[off], tag = p[off + 1];

		if (!words)
			break;
		if (words < 2 || words > limit - off)
			return -EINVAL;
		if (tag == 0x41000804) {
			if (words != 25 || p[off + 24] != 22)
				return -EPROTO;
			pr_info("Y2CEILING_READ_ONLY: devinfo3_low2=%u devinfo15_bits30_28=%u words=%u count=%u\n",
				p[off + 5] & 3, (p[off + 17] >> 28) & 7,
				words, p[off + 24]);
			return 0;
		}
		off += words;
	}
	return -ENOENT;
}

static int __init y2_ceiling_snapshot_init(void)
{
	unsigned int trial, phase, cpu;

	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	pr_info("Y2CEILING_READ_ONLY: bin_result=%d\n", snapshot_bin());
	cpus_read_lock();
	for (trial = 0; trial < 3; trial++) {
		phase = 0;
		on_each_cpu(snapshot_timer, &phase, 1);
		msleep(50);
		phase = 1;
		on_each_cpu(snapshot_timer, &phase, 1);
		for_each_online_cpu(cpu) {
			struct timer_sample *a, *b;

			if (cpu >= 4)
				continue;
			a = &samples[0][cpu];
			b = &samples[1][cpu];
			pr_info("Y2CEILING_READ_ONLY: trial=%u cpu=%u pfr1=%#x cntfrq=%u cntkctl=%#x cntp_ctl=%#x cntv_ctl=%#x delta_count=%llu delta_ns=%llu\n",
				trial, cpu, b->pfr1, b->frequency,
				b->kernel_control, b->physical_control,
				b->virtual_control, b->count - a->count,
				b->ns - a->ns);
		}
	}
	cpus_read_unlock();
	return -EAGAIN;
}
module_init(y2_ceiling_snapshot_init);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Temporary read-only Y2 CPU-bin and physical-counter observation");
