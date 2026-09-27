// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 latch/flush protocol (MediaTek 2011-2014), adapted to Linux CPU PM.
 * This is a low-power latch owner, not a second IRQ domain. The live GIC and
 * sysirq drivers continue to own interrupt configuration. Only CPU0 with the
 * other cores physically off may temporarily mask SPIs. */
#include <linux/io.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/of.h>
#include <linux/cpu.h>
#include <asm/barrier.h>
#include "cirq.h"
#include "cirq-policy.h"
static void __iomem *cirq, *dist, *pol;
static unsigned masks[7], entries, flushes, last_pending;
static bool active;
static unsigned saved_control;
bool y2_cirq_ready(void) { return !!smp_load_acquire(&cirq); }
int y2_cirq_begin(void)
{
	unsigned b;
	if (!y2_cirq_ready()) return -ENODEV;
	if (WARN_ON_ONCE(!irqs_disabled()) || num_online_cpus() != 1 ||
	    smp_processor_id() || active) return -EBUSY;
	saved_control = readl(cirq + 0x300);
	if (saved_control & 1) return -EBUSY;
	/* Snapshot before masking; clone enabled state, polarity and sensitivity.
	 * Reject preexisting enabled pending interrupts rather than enter sleep. */
	for (b = 1; b < 7; b++) {
		masks[b] = readl(dist + 0x100 + b * 4);
		if (readl(dist + 0x200 + b * 4) & masks[b]) return -EAGAIN;
	}
	for (b = 0; b < Y2_CIRQ_BANKS; b++) {
		unsigned valid = y2_cirq_valid(b);
		unsigned enabled = masks[b + 2] & valid;
		unsigned pending = readl(dist + 0x200 + (b + 2) * 4);
		unsigned sens = y2_cirq_sensitivity(readl(dist + 0xc00 + (b * 2 + 4) * 4),
			readl(dist + 0xc00 + (b * 2 + 5) * 4)) & valid;
		writel(valid, cirq + 0xc0 + b * 4);
		writel(valid, cirq + 0x1c0 + b * 4);
		writel(sens, cirq + 0x180 + b * 4);
		writel(valid, cirq + 0x280 + b * 4);
		writel(~readl(pol + (b + 1) * 4) & valid, cirq + 0x240 + b * 4);
		writel(y2_cirq_ack_mask(pending, enabled, valid), cirq + 0x40 + b * 4);
		writel(enabled, cirq + 0x100 + b * 4);
	}
	writel(saved_control | 3, cirq + 0x300); /* MT6582 enable + edge-only; no newer FLUSH bit */
	dsb(sy);
	if ((readl(cirq + 0x300) & 3) != 3) {
		writel(saved_control & ~1U, cirq + 0x300);
		return -EIO;
	}
	for (b = 1; b < 7; b++) writel(~0U, dist + 0x180 + b * 4);
	writel(BIT(149 % 32), dist + 0x100 + (149 / 32) * 4); /* SPM IRQ */
	dsb(sy);
	active = true;
	entries++;
	return 0;
}
void y2_cirq_end(void)
{
	unsigned b;
	if (!active) return;
	/* Stock unmask -> latch read -> GIC pending set -> mask -> disable.
	 * GIC cluster restore must have completed before this operation. */
	last_pending = 0;
	for (b = 0; b < Y2_CIRQ_BANKS; b++) writel(y2_cirq_valid(b), cirq + 0x100 + b * 4);
	dsb(sy);
	for (b = 0; b < Y2_CIRQ_BANKS; b++) {
		unsigned pending = readl(cirq + b * 4) & y2_cirq_valid(b);
		writel(pending, dist + 0x200 + (b + 2) * 4);
		last_pending |= pending;
		writel(y2_cirq_valid(b), cirq + 0xc0 + b * 4);
	}
	dsb(sy);
	writel((saved_control | 2) & ~1U, cirq + 0x300);
	for (b = 1; b < 7; b++) {
		writel(~0U, dist + 0x180 + b * 4);
		writel(masks[b], dist + 0x100 + b * 4);
	}
	dsb(sy);
	active = false;
	flushes++;
}
unsigned y2_cirq_snapshot(void)
{
	return cirq ? readl(cirq + 0x300) : 0;
}
static ssize_t state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	return sysfs_emit(buf, "ready=%u active=%u entries=%u flushes=%u pending=%#x range=64-218\n",
		y2_cirq_ready(), READ_ONCE(active), READ_ONCE(entries), READ_ONCE(flushes), READ_ONCE(last_pending));
}
static DEVICE_ATTR_RO(state);
static int y2_cirq_probe(struct platform_device *pdev)
{
	void __iomem *base;
	int ret;
	base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(base)) return PTR_ERR(base);
	/* Read/mask windows deliberately shared with the IRQchip owners; no
	 * resource reservation over their range or second configuration owner. */
	dist = devm_ioremap(&pdev->dev, 0x10211000, 0x1000);
	pol = devm_ioremap(&pdev->dev, 0x10200100, 0x1c);
	if (!dist || !pol) return -ENOMEM;
	if (readl(base + 0x300) & 1) return -EBUSY;
	ret = device_create_file(&pdev->dev, &dev_attr_state);
	if (ret) return ret;
	smp_store_release(&cirq, base);
	return 0;
}
static const struct of_device_id matches[] = { { .compatible = "innioasis,y2-cirq" }, {} };
static struct platform_driver y2_cirq_driver = { .probe = y2_cirq_probe,
	.driver = { .name = "y2-cirq", .of_match_table = matches, .suppress_bind_attrs = true } };
module_platform_driver(y2_cirq_driver);
MODULE_LICENSE("GPL");
