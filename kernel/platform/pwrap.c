// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 WACS2 transport: canonical owner shared by USB and MT6323 MFD.
 * Offsets/INIT/SYNC contract: retained MT6582 BSP pwrap_hal and our USBACM-03.
 * No reset, bus reinitialization, arbitration change or wrapper exception IRQ.
 */
#include "/project/kernel/diagnostic/pwrap.h"
#include "policy.h"
#include "shared.h"
#include <linux/clk.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/mfd/mt6323/registers.h>
#include <linux/module.h>
#include <linux/of_platform.h>
#include <linux/platform_device.h>
#include <linux/power_supply.h>
#include <linux/property.h>
#include <linux/regmap.h>

struct y2_wrap {
	void __iomem *base;
	struct regmap *map;
	struct device *dev;
};
static DEFINE_MUTEX(y2_wrap_lock);
static struct y2_wrap *y2_wrap;
static unsigned wrap_read(void *p, unsigned off) { return readl(p + off); }
static void wrap_write(void *p, unsigned off, unsigned val) { writel(val, p + off); }
static void wrap_delay(void *p) { udelay(10); }
static struct y2_pwrap_io wrap_io(struct y2_wrap *w)
{
	return (struct y2_pwrap_io){w->base, wrap_read, wrap_write, wrap_delay};
}

/* Regmap serializes all commands, including RMW. A fault poisons the channel:
 * subsequent requests refuse unowned pending state rather than clear it. */
static int wrap_reg_read(void *context, unsigned reg, unsigned *val)
{
	struct y2_wrap *w = context;
	struct y2_pwrap_io io = wrap_io(w);
	unsigned state = readl(w->base + 0xa0);
	int ret;
	if (reg & 1)
		return -EINVAL;
	ret = y2_pwrap_wait_sync(&io, &state);
	if (ret)
		return ret;
	writel((reg >> 1) << 16, w->base + 0x9c);
	ret = y2_pwrap_wait(&io, &state, 1);
	if (ret)
		return ret;
	*val = state & 0xffff;
	writel(1, w->base + 0xa4);
	return y2_pwrap_wait(&io, &state, 0);
}
static int wrap_reg_write(void *context, unsigned reg, unsigned val)
{
	struct y2_wrap *w = context;
	struct y2_pwrap_io io = wrap_io(w);
	unsigned state, old, mask = 0;
	int ret;
	/* Development write firewall. Only VGP2 enable/selector is added for M3; charger/reset remain blocked.
	 * INT_CON/STATUS are the upstream MT6323 MFD's mask/W1C registers.
	 * Backlight may only lower PWM duty or restore the saved duty, never current. */
	mask = y2_pmic_write_mask(reg);
	if (!mask)
		return -EPERM;
	if (mask != 0xffff) {
		ret = wrap_reg_read(w, reg, &old);
		if (ret)
			return ret;
		if ((old ^ val) & ~mask)
			return -EPERM;
	}
	state = readl(w->base + 0xa0);
	ret = y2_pwrap_wait_sync(&io, &state);
	if (ret)
		return ret;
	writel(BIT(31) | ((reg >> 1) << 16) | (val & 0xffff), w->base + 0x9c);
	return y2_pwrap_wait(&io, &state, 0);
}
static const struct regmap_config wrap_config = {
    .reg_bits = 16,
    .val_bits = 16,
    .reg_stride = 2,
    .max_register = 0xffe,
    .reg_read = wrap_reg_read,
    .reg_write = wrap_reg_write,
    .cache_type = REGCACHE_NONE,
};
int y2_pmic_snapshot(struct y2_pwrap_snapshot *s)
{
	int ret = -EPROBE_DEFER;
	*s = (struct y2_pwrap_snapshot){.magic = Y2_PWRAP_MAGIC, .result = ret};
	mutex_lock(&y2_wrap_lock);
	if (y2_wrap) {
		struct y2_wrap *w = y2_wrap;
		s->mux = readl(w->base);
		s->wrap = readl(w->base + 4);
		s->arb = readl(w->base + 0x50);
		s->channel = readl(w->base + 0x94);
		s->init = readl(w->base + 0x98);
		s->before = readl(w->base + 0xa0);
		ret = regmap_read(w->map, 0x100, &s->cid);
		if (!ret) {
			s->valid = 1;
			ret = regmap_read(w->map, 0x502, &s->vusb);
		}
		if (!ret) {
			s->valid |= 2;
			ret = regmap_read(w->map, 0, &s->chrdet);
		}
		if (!ret)
			s->valid |= 4;
		s->after = readl(w->base + 0xa0);
		s->result = ret;
	}
	mutex_unlock(&y2_wrap_lock);
	return ret;
}
static enum power_supply_property y2_usb_props[] = {POWER_SUPPLY_PROP_ONLINE};
static int y2_supply_get(struct power_supply *s, enum power_supply_property p,
			 union power_supply_propval *v)
{
	struct y2_wrap *w = power_supply_get_drvdata(s);
	unsigned value;
	int ret;
	if (p != POWER_SUPPLY_PROP_ONLINE)
		return -EINVAL;
	ret = regmap_read(w->map, MT6323_CHR_CON0, &value);
	if (!ret)
		v->intval = !!(value & BIT(5));
	return ret;
}
static const struct power_supply_desc y2_supply_desc = {.name = "y2-usb-presence",
							.type = POWER_SUPPLY_TYPE_USB,
							.properties = y2_usb_props,
							.num_properties = ARRAY_SIZE(y2_usb_props),
							.get_property = y2_supply_get};
static int wrap_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct y2_wrap *w;
	struct y2_pwrap_snapshot s;
	struct y2_pwrap_io io;
	struct power_supply_config cfg = {};
	struct power_supply *psy;
	struct clk *clk;
	int ret;
	w = devm_kzalloc(dev, sizeof(*w), GFP_KERNEL);
	if (!w)
		return -ENOMEM;
	clk = devm_clk_get_enabled(dev, NULL);
	if (IS_ERR(clk))
		return dev_err_probe(dev, PTR_ERR(clk), "wrap clock\n");
	w->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(w->base))
		return PTR_ERR(w->base);
	w->dev = dev;
	io = wrap_io(w);
	y2_pwrap_probe(&io, &s);
	if (s.result)
		return dev_err_probe(dev, s.result, "inherited WACS2 not ready\n");
	w->map = devm_regmap_init(dev, NULL, w, &wrap_config);
	if (IS_ERR(w->map))
		return PTR_ERR(w->map);
	mutex_lock(&y2_wrap_lock);
	if (y2_wrap) {
		mutex_unlock(&y2_wrap_lock);
		return -EBUSY;
	}
	y2_wrap = w;
	mutex_unlock(&y2_wrap_lock);
	/* Built-in non-unbindable owner: USB uses this map throughout the window. */
	platform_set_drvdata(pdev, w);
	cfg.drv_data = w;
	cfg.fwnode = dev_fwnode(dev);
	psy = devm_power_supply_register(dev, &y2_supply_desc, &cfg);
	if (IS_ERR(psy))
		dev_warn(dev, "USB presence class: %ld\n", PTR_ERR(psy));
	ret = devm_of_platform_populate(dev);
	if (ret)
		dev_warn(dev, "PMIC children: %d; USB transport remains live\n", ret);
	dev_info(dev,
		 "MT6323 CID=%04x VUSB=%04x CHRDET=%u; serialized regmap; rail/charger/reset "
		 "writes blocked\n",
		 s.cid, s.vusb, !!(s.chrdet & 32));
	return 0;
}
static const struct of_device_id wrap_match[] = {{.compatible = "innioasis,y2-pwrap"}, {}};
static struct platform_driver wrap_driver = {
    .probe = wrap_probe,
    .driver = {.name = "y2-pwrap", .of_match_table = wrap_match, .suppress_bind_attrs = true}};
static int __init wrap_init(void) { return platform_driver_register(&wrap_driver); }
subsys_initcall(wrap_init);
MODULE_LICENSE("GPL");
