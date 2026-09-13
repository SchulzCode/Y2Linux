// SPDX-License-Identifier: GPL-2.0-only
/* Read-only power_supply telemetry plus a fail-closed charging inhibit.
 * No known Y2 pack thermistor conversion: keep CHR_EN/CSDAC_EN clear.
 * Never change current/CV/OVP, emergency reset, trim or charger watchdog.
 * Limits are configuration, not measured current. */
#include <linux/iio/consumer.h>
#include <linux/interrupt.h>
#include <linux/mfd/mt6397/core.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/power_supply.h>
#include <linux/regmap.h>
#include <linux/workqueue.h>
#include "power-math.h"

struct y2_charger {
	struct device *dev;
	struct regmap *map;
	struct iio_channel *battery;
	struct power_supply *bat, *usb;
	struct delayed_work work;
};
static int y2_charger_inhibit(struct y2_charger *c)
{
	unsigned val;
	int ret = regmap_update_bits(c->map, 0, BIT(3) | BIT(4), 0);
	if (!ret) ret = regmap_read(c->map, 0, &val);
	if (!ret && (val & (BIT(3) | BIT(4)))) ret = -EIO;
	return ret;
}
static enum power_supply_property y2_bat_props[] = {
	POWER_SUPPLY_PROP_PRESENT, POWER_SUPPLY_PROP_STATUS,
	POWER_SUPPLY_PROP_HEALTH, POWER_SUPPLY_PROP_VOLTAGE_NOW,
	POWER_SUPPLY_PROP_CONSTANT_CHARGE_CURRENT, POWER_SUPPLY_PROP_CONSTANT_CHARGE_VOLTAGE,
};
static enum power_supply_property y2_usb_props[] = { POWER_SUPPLY_PROP_ONLINE };
static int y2_bat_get(struct power_supply *psy, enum power_supply_property p,
		      union power_supply_propval *v)
{
	struct y2_charger *c = power_supply_get_drvdata(psy);
	unsigned value;
	int ret, raw;
	switch (p) {
	case POWER_SUPPLY_PROP_PRESENT:
		ret = regmap_read(c->map, 0x00e, &value);
		if (ret) return ret;
		if ((value & 5) != 5) return -ENODATA; /* BATON_EN, TDET_EN */
		v->intval = !(value & BIT(12));
		return 0;
	case POWER_SUPPLY_PROP_STATUS:
		ret = regmap_read(c->map, 0, &value);
		if (ret) return ret;
		/* Enabled charge engine does not prove battery current or completion. */
		v->intval = (value & (BIT(3) | BIT(4))) ? POWER_SUPPLY_STATUS_UNKNOWN :
			(value & BIT(5)) ? POWER_SUPPLY_STATUS_NOT_CHARGING : POWER_SUPPLY_STATUS_DISCHARGING;
		return 0;
	case POWER_SUPPLY_PROP_HEALTH:
		ret = regmap_read(c->map, 0x00c, &value);
		if (ret) return ret;
		v->intval = (value & BIT(6)) ? POWER_SUPPLY_HEALTH_OVERVOLTAGE : POWER_SUPPLY_HEALTH_UNKNOWN;
		return 0;
	case POWER_SUPPLY_PROP_VOLTAGE_NOW:
		ret = iio_read_channel_raw(c->battery, &raw);
		if (ret < 0) return ret;
		v->intval = y2_battery_uv(raw);
		return 0;
	case POWER_SUPPLY_PROP_CONSTANT_CHARGE_CURRENT:
		ret = regmap_read(c->map, 0x008, &value);
		if (!ret) v->intval = y2_charge_ua(value);
		return ret;
	case POWER_SUPPLY_PROP_CONSTANT_CHARGE_VOLTAGE:
		ret = regmap_read(c->map, 0x006, &value);
		if (!ret) v->intval = y2_charge_uv(value);
		return ret;
	default: return -EINVAL;
	}
}
static int y2_usb_get(struct power_supply *psy, enum power_supply_property p,
		      union power_supply_propval *v)
{
	struct y2_charger *c = power_supply_get_drvdata(psy);
	unsigned value;
	int ret;
	if (p != POWER_SUPPLY_PROP_ONLINE) return -EINVAL;
	ret = regmap_read(c->map, 0, &value);
	if (!ret) v->intval = !!(value & BIT(5));
	return ret;
}
static const struct power_supply_desc y2_bat_desc = {
	.name = "BAT0", .type = POWER_SUPPLY_TYPE_BATTERY,
	.properties = y2_bat_props, .num_properties = ARRAY_SIZE(y2_bat_props), .get_property = y2_bat_get,
};
static const struct power_supply_desc y2_usb_desc = {
	/* Existing initramfs ABI: retained while moving ownership under the MFD. */
	.name = "y2-usb-presence", .type = POWER_SUPPLY_TYPE_USB,
	.properties = y2_usb_props, .num_properties = ARRAY_SIZE(y2_usb_props), .get_property = y2_usb_get,
};
static void y2_charger_poll(struct work_struct *work)
{
	struct y2_charger *c = container_of(to_delayed_work(work), struct y2_charger, work);
	int ret = y2_charger_inhibit(c);
	if (ret) dev_err_ratelimited(c->dev, "cannot verify charging inhibited: %d\n", ret);
	power_supply_changed(c->bat); power_supply_changed(c->usb);
	queue_delayed_work(system_freezable_wq, &c->work, msecs_to_jiffies(10000));
}
static irqreturn_t y2_charger_irq(int irq, void *arg)
{
	struct y2_charger *c = arg;
	mod_delayed_work(system_freezable_wq, &c->work, 0);
	return IRQ_HANDLED;
}
static int y2_charger_suspend(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	int ret;
	cancel_delayed_work_sync(&c->work);
	ret = y2_charger_inhibit(c);
	if (ret) queue_delayed_work(system_freezable_wq, &c->work, msecs_to_jiffies(10000));
	return ret;
}
static int y2_charger_resume(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	int ret = y2_charger_inhibit(c);
	queue_delayed_work(system_freezable_wq, &c->work, 0);
	return ret;
}
static void y2_charger_shutdown(struct platform_device *pdev)
{
	struct y2_charger *c = platform_get_drvdata(pdev);
	int ret;
	cancel_delayed_work_sync(&c->work);
	ret = y2_charger_inhibit(c);
	if (ret) dev_err(&pdev->dev, "shutdown charger inhibit failed: %d\n", ret);
}
static void y2_charger_cancel(void *data)
{
	struct y2_charger *c = data;
	cancel_delayed_work_sync(&c->work);
}
static int y2_charger_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct mt6397_chip *chip = dev_get_drvdata(dev->parent);
	struct power_supply_config cfg = {};
	struct y2_charger *c;
	unsigned wdt, current, voltage;
	int ret, irq;
	if (!chip || !chip->regmap) return -EPROBE_DEFER;
	c = devm_kzalloc(dev, sizeof(*c), GFP_KERNEL);
	if (!c) return -ENOMEM;
	c->dev = dev; c->map = chip->regmap;
	/* Inhibit before any dependency can defer. Watchdog expiry cannot enable
	 * this engine. A future charging policy must own its 4-second servicing. */
	ret = y2_charger_inhibit(c);
	if (ret) return dev_err_probe(dev, ret, "charging inhibit\n");
	c->battery = devm_iio_channel_get(dev, "battery-voltage");
	if (IS_ERR(c->battery)) return dev_err_probe(dev, PTR_ERR(c->battery), "battery ADC\n");
	ret = regmap_read(c->map, 0x01a, &wdt);
	if (!ret) ret = regmap_read(c->map, 0x008, &current);
	if (!ret) ret = regmap_read(c->map, 0x006, &voltage);
	if (ret) return ret;
	cfg.drv_data = c; cfg.fwnode = dev_fwnode(dev);
	c->bat = devm_power_supply_register(dev, &y2_bat_desc, &cfg);
	if (IS_ERR(c->bat)) return PTR_ERR(c->bat);
	c->usb = devm_power_supply_register(dev, &y2_usb_desc, &cfg);
	if (IS_ERR(c->usb)) return PTR_ERR(c->usb);
	platform_set_drvdata(pdev, c);
	INIT_DELAYED_WORK(&c->work, y2_charger_poll);
	irq = platform_get_irq(pdev, 0);
	if (irq < 0) return irq;
	ret = devm_add_action_or_reset(dev, y2_charger_cancel, c);
	if (ret) return ret;
	ret = devm_request_threaded_irq(dev, irq, NULL, y2_charger_irq, IRQF_ONESHOT, "mt6323-chrdet", c);
	if (ret) return ret;
	queue_delayed_work(system_freezable_wq, &c->work, msecs_to_jiffies(10000));
	dev_info(dev, "charging INHIBITED: pack thermometry unvalidated; inherited limit=%duA/%duV WDT_CON=%04x unchanged\n",
		 y2_charge_ua(current), y2_charge_uv(voltage), wdt);
	return 0;
}
static DEFINE_SIMPLE_DEV_PM_OPS(y2_charger_pm, y2_charger_suspend, y2_charger_resume);
static const struct of_device_id y2_charger_match[] = { { .compatible = "innioasis,y2-mt6323-charger" }, {} };
static struct platform_driver y2_charger_driver = {
	.probe = y2_charger_probe, .shutdown = y2_charger_shutdown,
	.driver = { .name = "y2-mt6323-charger", .of_match_table = y2_charger_match,
		.pm = pm_sleep_ptr(&y2_charger_pm), .suppress_bind_attrs = true },
};
module_platform_driver(y2_charger_driver);
MODULE_LICENSE("GPL");
