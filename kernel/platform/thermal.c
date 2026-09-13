// SPDX-License-Identifier: GPL-2.0-only
/* Register sequence derived from Copyright (C) 2011-2014 MediaTek Inc. */
/* MT6582 two-sensor AUXADC thermal monitor. Register layout/calibration:
 * MediaTek GPL BSP mtk_ts_cpu.c, d53dd75c3ff7. Different from the 6.18
 * auxadc_thermal variants (offset correction, two fuses, analog mux address).
 * Only read our own factory efuses. Do not install synthetic calibration or
 * change firmware hardware protection thresholds/interrupts. */
#include <linux/clk.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/nvmem-consumer.h>
#include <linux/platform_device.h>
#include <linux/slab.h>
#include <linux/thermal.h>
#include <linux/unaligned.h>
#include "power-math.h"

struct y2_thermal {
	void __iomem *base, *adc, *analog;
	struct y2_soc_cal cal;
};
static int y2_soc_temp(struct thermal_zone_device *tz, int *temp)
{
	struct y2_thermal *t = thermal_zone_device_priv(tz);
	unsigned a = readl(t->base + 0x90) & 0xfff;
	unsigned b = readl(t->base + 0x94) & 0xfff;
	/* Zero and saturated ADC values are not meaningful Celsius readings. */
	if (!a || !b || a == 0xfff || b == 0xfff) return -ENODATA;
	*temp = max(y2_soc_mc(&t->cal, a, 0), y2_soc_mc(&t->cal, b, 1));
	return 0;
}
static const struct thermal_zone_device_ops y2_soc_thermal_ops = { .get_temp = y2_soc_temp };
static int y2_thermal_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct y2_thermal *t;
	struct nvmem_cell *cell;
	struct thermal_zone_device *tz;
	struct clk *clk;
	static const char *const clocks[] = { "therm", "auxadc", "efuse" };
	u8 *fuses;
	size_t len;
	unsigned i;
	int ret;
	t = devm_kzalloc(dev, sizeof(*t), GFP_KERNEL);
	if (!t) return -ENOMEM;
	for (i = 0; i < ARRAY_SIZE(clocks); i++) {
		clk = devm_clk_get_enabled(dev, clocks[i]);
		if (IS_ERR(clk)) return dev_err_probe(dev, PTR_ERR(clk), "thermal clock %s\n", clocks[i]);
	}
	cell = devm_nvmem_cell_get(dev, "calibration");
	if (IS_ERR(cell)) return dev_err_probe(dev, PTR_ERR(cell), "own thermal efuses\n");
	fuses = nvmem_cell_read(cell, &len);
	if (IS_ERR(fuses)) return PTR_ERR(fuses);
	ret = len == 8 ? y2_soc_calibrate(get_unaligned_le32(fuses), get_unaligned_le32(fuses + 4), &t->cal) : -1;
	kfree(fuses);
	if (ret) return dev_err_probe(dev, -ENODATA, "no valid SoC calibration; Celsius unavailable\n");
	t->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(t->base)) return PTR_ERR(t->base);
	t->adc = devm_platform_ioremap_resource(pdev, 1);
	if (IS_ERR(t->adc)) return PTR_ERR(t->adc);
	t->analog = devm_platform_ioremap_resource(pdev, 2);
	if (IS_ERR(t->analog)) return PTR_ERR(t->analog);
	if (readl(t->base + 0x0c)) {
		/* An existing autonomous protection owner must remain intact. Adopt
		 * only the exact two-sensor path this driver knows how to convert. */
		if ((readl(t->base) & 3) != 3 || readl(t->base + 0x64) != 0x10209604 ||
		    readl(t->base + 0x78) != 0x11001040 || readl(t->base + 0x7c) != 0x11001040 ||
		    readl(t->base + 0x48) != 0 || readl(t->base + 0x4c) != 1)
			return dev_err_probe(dev, -EBUSY, "unrecognized inherited thermal protection; retained\n");
	} else {
		writel(0, t->base); /* No inherited protection is active. */
		writel(readl(t->adc) & ~BIT(11), t->adc);
		writel(BIT(11), t->adc + 0x0c);
		writel(0x3ff, t->base + 4);
		writel(0x03ff0000, t->base + 8);
		writel(0x00ffffff, t->base + 0x40);
		writel(0, t->base + 0x38);
		writel(0xffffffff, t->base + 0x44);
		writel(0, t->base + 0x14); writel(0, t->base + 0x18);
		writel(BIT(11), t->adc + 8);
		writel(BIT(11), t->base + 0x54); writel(BIT(11), t->base + 0x60);
		writel(0x1100100c, t->base + 0x68);
		writel(0x11001008, t->base + 0x74);
		writel(0x11001040, t->base + 0x78); writel(0x11001040, t->base + 0x7c);
		writel(0, t->base + 0x80); writel(0x2c, t->base + 0x84);
		writel(0, t->base + 0x88); writel(2, t->base + 0x8c);
		writel(readl(t->analog) & ~0xc0, t->analog);
		writel(0, t->base + 0x48); writel(1, t->base + 0x4c);
		writel(0x10209604, t->base + 0x64);
		writel(3, t->base + 0x8c); writel(3, t->base);
	}
	/* Keep monitor/ADC/clocks powered across s2idle: autonomous protection
	 * and conversion continue. No deep suspend is advertised by this port. */
	tz = devm_thermal_of_zone_register(dev, 0, t, &y2_soc_thermal_ops);
	if (IS_ERR(tz)) return dev_err_probe(dev, PTR_ERR(tz), "CPU thermal zone\n");
	dev_info(dev, "two calibrated CPU sensors, maximum reported; own read-only efuses; protection retained\n");
	return 0;
}
static const struct of_device_id y2_thermal_match[] = { { .compatible = "mediatek,mt6582-thermal" }, {} };
static struct platform_driver y2_thermal_driver = {
	.probe = y2_thermal_probe,
	.driver = { .name = "mt6582-thermal", .of_match_table = y2_thermal_match, .suppress_bind_attrs = true },
};
module_platform_driver(y2_thermal_driver);
MODULE_LICENSE("GPL");
