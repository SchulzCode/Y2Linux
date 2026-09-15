// SPDX-License-Identifier: GPL-2.0-only
/* MT6323 production charger. All hardware IO uses the parent's sole regmap.
 * Stock Y2 contract and bounded-profile limitations: docs/knowledge/m4-charging.md.
 * Pack temperature, measured current and SOC deliberately remain unsupported.
 */
#include <linux/iio/consumer.h>
#include <linux/delay.h>
#include <linux/interrupt.h>
#include <linux/ktime.h>
#include <linux/mfd/mt6397/core.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/power_supply.h>
#include <linux/property.h>
#include <linux/regmap.h>
#include <linux/workqueue.h>
#include "power-math.h"
#include "charger-policy.h"
#include "source-policy.h"
#include "source.h"

#define Y2_POLL_MS 1000
#define Y2_PET_LATE_MS 2500

enum y2_charge_fault {
	Y2_FAULT_IO = BIT(0), Y2_FAULT_ADC = BIT(1), Y2_FAULT_BATTERY = BIT(2),
	Y2_FAULT_VOLTAGE = BIT(3), Y2_FAULT_OVP = BIT(4), Y2_FAULT_THERMAL = BIT(5),
	Y2_FAULT_WATCHDOG = BIT(6), Y2_FAULT_PROTECTION = BIT(7),
	Y2_FAULT_TIMEOUT = BIT(8), Y2_FAULT_SOURCE = BIT(9),
};
struct y2_charger {
	struct device *dev;
	struct regmap *map;
	struct y2_charge_io io;
	struct iio_channel *battery, *baton, *isense, *die;
	struct power_supply *bat, *usb, *input;
	struct delayed_work work;
	struct workqueue_struct *wq;
	struct notifier_block input_nb;
	struct mutex lock;
	struct y2_charge_cycle cycle;
	u64 last_account, last_pet_ms, pets, report_at;
	unsigned fault, con0, con4, ov, baton_con, wdt, thr;
	u64 first_fault_ms;
	unsigned first_fault, first_con0, first_cv, first_ov, first_wdt, first_thr;
	int first_uv, first_sample_error;
	unsigned charge_ua, selector;
	enum y2_source_type source;
	int uv, baton_raw, isense_raw, die_mc, input_ua, sample_error, stop_error, last_error;
	int status, behaviour;
	bool active, paused, stopping, online, present, awake, irq_wake;
	bool source_valid, source_changed;
	int irq;
};

static int y2_charge_read(void *context, unsigned reg, unsigned *value)
{
	return regmap_read(context, reg, value);
}
static int y2_charge_update(void *context, unsigned reg, unsigned mask,
			    unsigned value, int force)
{
	return force ? regmap_write_bits(context, reg, mask, value) :
		regmap_update_bits(context, reg, mask, value);
}
static void y2_charge_awake(struct y2_charger *c, bool awake)
{
	if (c->awake == awake) return;
	c->awake = awake;
	if (awake) pm_stay_awake(c->dev);
	else pm_relax(c->dev);
}
static int y2_charger_inhibit(struct y2_charger *c)
{
	int ret;
	/* Stop/readback overwrites status registers. Retain the original detector
	 * and sample validity first, including input OVP versus battery OVP. */
	if (c->fault && !c->first_fault) {
		c->first_fault = c->fault;
		c->first_fault_ms = ktime_to_ms(ktime_get_boottime());
		c->first_con0 = c->con0; c->first_cv = c->con4; c->first_ov = c->ov;
		c->first_wdt = c->wdt; c->first_thr = c->thr;
		c->first_uv = c->uv; c->first_sample_error = c->sample_error;
		dev_err_ratelimited(c->dev, "first fault=%#x battery=%duV sample_error=%d chr=%#x cv=%#x ovp=%#x wdt=%#x thermal=%#x\n",
			c->fault, c->uv, c->sample_error, c->con0, c->con4, c->ov, c->wdt, c->thr);
	}
	ret = y2_charge_stop(&c->io);
	if (!ret) ret = regmap_read(c->map, 0x000, &c->con0);
	if (!ret) ret = regmap_read(c->map, 0x01e, &c->wdt);
	c->stop_error = ret;
	if (!ret) {
		c->active = false;
		y2_charge_awake(c, false);
	} else {
		/* Keep trying, keep the AP awake, and do NOT pet a faulty charger.
		 * The armed hardware watchdog remains the fallback if PWRAP fails. */
		c->fault |= Y2_FAULT_IO;
		c->last_error = ret;
		y2_charge_awake(c, true);
		dev_err_ratelimited(c->dev, "charger inhibit not verified: %d\n", ret);
	}
	return ret;
}
static void y2_charge_account_now(struct y2_charger *c)
{
	u64 now = ktime_get_boottime_seconds();
	if (c->active && now > c->last_account)
		y2_charge_account(&c->cycle, now - c->last_account, c->uv);
	c->last_account = now;
}
static int y2_charge_sample(struct y2_charger *c)
{
	int ret, raw;

	ret = regmap_read(c->map, 0x000, &c->con0);
	if (ret) return ret;
	c->online = !!(c->con0 & BIT(5));
	ret = regmap_read(c->map, 0x00e, &c->baton_con);
	if (ret) return ret;
	c->present = !(c->baton_con & BIT(12)) && (c->baton_con & 5) == 5;
	ret = regmap_read(c->map, 0x00c, &c->ov);
	if (!ret) ret = regmap_read(c->map, 0x004, &c->con4);
	if (!ret) ret = regmap_read(c->map, 0x01e, &c->wdt);
	if (!ret) ret = regmap_read(c->map, 0x044, &c->thr);
	if (ret) return ret;
	c->input_ua = y2_usb_charge_allocation();
	ret = iio_read_channel_raw(c->battery, &raw);
	if (ret < 0) return ret;
	c->uv = y2_battery_uv(raw);
	if (raw <= 0 || raw >= 32767) return -ENODATA;
	ret = iio_read_channel_raw(c->baton, &c->baton_raw);
	if (ret < 0) return ret;
	ret = iio_read_channel_raw(c->isense, &c->isense_raw);
	if (ret < 0) return ret;
	ret = iio_read_channel_processed(c->die, &c->die_mc);
	if (ret < 0) return ret;
	/* These are acquisition validity checks, not a pack-temperature model. */
	if (c->baton_raw <= 0 || c->baton_raw >= 32767 ||
	    c->isense_raw <= 0 || c->isense_raw >= 32767) return -ENODATA;
	return 0;
}

static int y2_wait_source(void *context, unsigned ms)
{
	unsigned value;
	int ret;
	msleep(ms);
	ret = regmap_read(context, 0, &value);
	return ret ? ret : value & BIT(5) ? 0 : -ENODEV;
}

static int y2_charger_detect(struct y2_charger *c)
{
	struct y2_bc11_io io = { .pmic = c->io, .wait_source = y2_wait_source };
	int ret, release;
	ret = y2_charger_inhibit(c);
	if (ret) return ret;
	y2_usb_source_invalidate();
	ret = y2_usb_bc11_begin();
	if (ret) return ret;
	WRITE_ONCE(c->source_changed, false);
	ret = y2_bc11_detect(&io, &c->source);
	if (!ret && READ_ONCE(c->source_changed)) ret = -EAGAIN;
	release = y2_usb_bc11_end(!ret &&
		(c->source == Y2_SOURCE_SDP || c->source == Y2_SOURCE_CDP));
	if (!ret) ret = release;
	if (!ret) {
		c->source_valid = true;
		dev_info(c->dev, "BC1.1 source=%u; charging independent of gadget configuration\n", c->source);
	}
	return ret;
}

static unsigned y2_charger_target(struct y2_charger *c)
{
	unsigned ua;
	if (!c->source_valid || READ_ONCE(c->source_changed) || !c->online) return 0;
	ua = y2_source_charge_ua(c->source, c->input_ua);
	/* LK's low-voltage setup uses 450 mA. Below its normal boot threshold
	 * take the already supported 70-mA level; never exceed the source limit.
	 * Full userspace stays out until an explicit, adequately powered boot. */
	if (c->uv < Y2_BOOT_MIN_UV) ua = min(ua, 70000U);
	else if (c->uv <= Y2_PRECHARGE_UV) ua = min(ua, 450000U);
	return ua;
}

static unsigned y2_charger_sample_fault(struct y2_charger *c)
{
	unsigned fault = 0;
	if (!c->present) fault |= Y2_FAULT_BATTERY;
	if (c->con0 & BIT(7) || c->ov & BIT(6)) fault |= Y2_FAULT_OVP;
	if (c->thr & 0x700 || c->die_mc >= 150000) fault |= Y2_FAULT_THERMAL;
	if (c->uv <= 0 || c->uv >= Y2_CHARGE_SAFETY_UV) fault |= Y2_FAULT_VOLTAGE;
	if (c->active && (c->wdt & BIT(2))) fault |= Y2_FAULT_WATCHDOG;
	return fault;
}

static void y2_charger_run(struct y2_charger *c)
{
	u64 now_ms = ktime_to_ms(ktime_get_boottime());
	unsigned target;
	int ret;

	y2_charge_account_now(c);
	/* Do not rearm a watchdog after a scheduling stall or expiry. */
	if (c->active && now_ms - c->last_pet_ms >= Y2_PET_LATE_MS)
		c->fault |= Y2_FAULT_WATCHDOG;
	ret = y2_charge_sample(c);
	c->sample_error = ret;
	if (ret) {
		c->last_error = ret;
		c->fault |= Y2_FAULT_ADC | Y2_FAULT_IO;
		goto inhibit;
	}
	if (!c->online) {
		c->source_valid = false;
		c->source = Y2_SOURCE_UNKNOWN;
		y2_usb_source_invalidate();
		if (!y2_charger_inhibit(c) && !y2_charger_sample_fault(c) &&
		    c->uv < Y2_CHARGE_UV) {
			/* Only verified removal, engine-off and recovered safety inputs
			 * start a new session. Enumeration/manual auto cannot clear it. */
			c->fault = 0;
			c->first_fault = 0;
			c->last_error = 0;
			c->cycle = (struct y2_charge_cycle){};
		}
		c->status = POWER_SUPPLY_STATUS_DISCHARGING;
		return;
	}
	c->fault |= y2_charger_sample_fault(c);
	if (c->cycle.timed_out) c->fault |= Y2_FAULT_TIMEOUT;
	if (c->fault || c->behaviour == POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE) goto inhibit;
	if (READ_ONCE(c->source_changed)) c->source_valid = false;
	if (!c->source_valid) {
		ret = y2_charger_detect(c);
		if (ret) {
			if (ret != -EAGAIN && ret != -ENODEV) {
				c->fault |= Y2_FAULT_SOURCE;
				c->last_error = ret;
			}
			goto inhibit;
		}
		ret = y2_charge_sample(c);
		c->sample_error = ret;
		if (ret) { c->fault |= Y2_FAULT_ADC | Y2_FAULT_IO; c->last_error = ret; goto inhibit; }
		c->fault |= y2_charger_sample_fault(c);
		if (c->fault || !c->online) goto inhibit;
	}
	/* Evaluate completion before changing the current or losing active state. */
	y2_charge_termination(&c->cycle, ktime_get_boottime_seconds(), c->uv, c->active);
	if (c->cycle.hold) goto inhibit;
	target = y2_charger_target(c);
	if (!target) goto inhibit;
	/* Reprogram current only after a verified engine stop. The session's
	 * watchdog/fault/elapsed-time budgets survive every source budget change. */
	if (c->active && target != c->charge_ua && y2_charger_inhibit(c)) goto inhibit;
	if (!c->active) {
		c->charge_ua = target;
		c->selector = y2_charge_selector(target);
	}

	if (!c->active) {
		y2_charge_awake(c, true);
		/* Timestamp before the service sequence: time spent blocked in IO
		 * must count against the deadline, including the very first start. */
		c->last_pet_ms = ktime_to_ms(ktime_get_boottime());
		ret = y2_charge_prepare(&c->io, c->selector);
		/* Recheck external allowance and fresh critical ADCs after prepare,
		 * immediately before the only charge-enable sequence. */
		if (!ret) {
			ret = y2_charge_sample(c);
			c->sample_error = ret;
			if (!ret) c->fault |= y2_charger_sample_fault(c);
		}
		if (c->fault) goto inhibit;
		if (!ret && y2_charger_target(c) != c->charge_ua) goto inhibit;
		if (!ret && c->uv >= Y2_CHARGE_UV) {
			y2_charge_termination(&c->cycle, ktime_get_boottime_seconds(), c->uv, 0);
			goto inhibit;
		}
		if (!ret && ktime_to_ms(ktime_get_boottime()) - c->last_pet_ms >= Y2_PET_LATE_MS)
			ret = -ETIME;
		if (!ret) ret = y2_charge_start(&c->io, c->selector);
		if (ret) { c->last_error = ret; c->fault |= Y2_FAULT_PROTECTION; goto inhibit; }
		c->active = true;
		c->last_account = ktime_get_boottime_seconds();
		c->cycle.confirm_at = c->last_account;
	} else {
		ret = y2_charge_protections(&c->io);
		if (!ret) ret = y2_charge_regulation(&c->io, c->selector);
		if (!ret) ret = y2_charge_expect(&c->io, 0x000, 0xbd, 0x39);
		if (!ret) ret = y2_charge_expect(&c->io, 0x01a, 0x1f, 0x10);
		if (!ret) ret = y2_charge_expect(&c->io, 0x01e, 5, 1);
		if (ret) { c->last_error = ret; c->fault |= Y2_FAULT_PROTECTION; goto inhibit; }
	}
	/* Re-read allowance before the service; the USB notifier also schedules
	 * immediate inhibition on disconnect/reset/bus suspend. */
	c->input_ua = y2_usb_charge_allocation();
	if (y2_charger_target(c) != c->charge_ua) goto inhibit;
	if (ktime_to_ms(ktime_get_boottime()) - c->last_pet_ms >= Y2_PET_LATE_MS) {
		c->fault |= Y2_FAULT_WATCHDOG;
		goto inhibit;
	}
	ret = y2_charge_expect(&c->io, 0x01e, 5, 1);
	if (!ret) ret = y2_charge_expect(&c->io, 0x000, 0xbd, 0x39);
	now_ms = ktime_to_ms(ktime_get_boottime());
	if (!ret && now_ms - c->last_pet_ms >= Y2_PET_LATE_MS) ret = -ETIME;
	if (!ret) ret = y2_charge_pet(&c->io);
	if (ret) { c->last_error = ret; c->fault |= Y2_FAULT_WATCHDOG; goto inhibit; }
	c->last_pet_ms = now_ms;
	c->pets++;
	ret = regmap_read(c->map, 0x000, &c->con0);
	if (!ret) ret = regmap_read(c->map, 0x01e, &c->wdt);
	if (ret) { c->last_error = ret; c->fault |= Y2_FAULT_IO; goto inhibit; }
	c->status = POWER_SUPPLY_STATUS_CHARGING;
	return;
inhibit:
	if (c->fault || !y2_charger_target(c) ||
	    c->behaviour == POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE) {
		c->cycle.confirmations = 0;
		c->cycle.confirm_at = ktime_get_boottime_seconds();
	}
	y2_charger_inhibit(c);
	c->status = c->stop_error ? POWER_SUPPLY_STATUS_UNKNOWN :
		!c->online ? POWER_SUPPLY_STATUS_DISCHARGING :
		c->cycle.hold && c->cycle.full && !c->fault &&
		c->behaviour == POWER_SUPPLY_CHARGE_BEHAVIOUR_AUTO ?
		POWER_SUPPLY_STATUS_FULL : POWER_SUPPLY_STATUS_NOT_CHARGING;
}
static void y2_charger_poll(struct work_struct *work)
{
	struct y2_charger *c = container_of(to_delayed_work(work), struct y2_charger, work);
	unsigned old_fault;
	int old_status;

	mutex_lock(&c->lock);
	if (c->paused || c->stopping) goto out;
	old_fault = c->fault; old_status = c->status;
	y2_charger_run(c);
	if (old_fault != c->fault || old_status != c->status ||
	    ktime_get_boottime_seconds() >= c->report_at) {
		/* Bounded production history survives offline-to-normal handover.
		 * No console connection is needed to qualify the earlier charge trend. */
		dev_info(c->dev, "status=%d fault=%#x battery=%duV input=%duA watchdog_pets=%llu source=%u limit=%uuA pmic=%dmC baton_raw=%d isense_raw=%d cv=%#x hold=%d full=%d\n",
			 c->status, c->fault, c->uv, c->input_ua, c->pets, c->source,
			 c->charge_ua, c->die_mc, c->baton_raw, c->isense_raw, c->con4,
			 c->cycle.hold, c->cycle.full);
		c->report_at = ktime_get_boottime_seconds() + 60;
		power_supply_changed(c->bat);
	}
	power_supply_changed(c->usb);
	queue_delayed_work(c->wq, &c->work, msecs_to_jiffies(Y2_POLL_MS));
out:
	mutex_unlock(&c->lock);
}
static irqreturn_t y2_charger_irq(int irq, void *arg)
{
	struct y2_charger *c = arg;
	WRITE_ONCE(c->source_changed, true);
	y2_usb_source_invalidate();
	if (!READ_ONCE(c->stopping)) mod_delayed_work(c->wq, &c->work, 0);
	return IRQ_HANDLED;
}
static int y2_charger_input_changed(struct notifier_block *nb, unsigned long event, void *data)
{
	struct y2_charger *c = container_of(nb, struct y2_charger, input_nb);
	if (event == PSY_EVENT_PROP_CHANGED && data == c->input && !READ_ONCE(c->stopping))
		mod_delayed_work(c->wq, &c->work, 0);
	return NOTIFY_OK;
}

static enum power_supply_property y2_bat_props[] = {
	POWER_SUPPLY_PROP_PRESENT, POWER_SUPPLY_PROP_STATUS, POWER_SUPPLY_PROP_HEALTH,
	POWER_SUPPLY_PROP_VOLTAGE_NOW, POWER_SUPPLY_PROP_CONSTANT_CHARGE_CURRENT,
	POWER_SUPPLY_PROP_CONSTANT_CHARGE_VOLTAGE, POWER_SUPPLY_PROP_CHARGE_BEHAVIOUR,
};
static enum power_supply_property y2_usb_props[] = {
	POWER_SUPPLY_PROP_ONLINE, POWER_SUPPLY_PROP_USB_TYPE,
};
static int y2_bat_get(struct power_supply *psy, enum power_supply_property p,
		      union power_supply_propval *v)
{
	struct y2_charger *c = power_supply_get_drvdata(psy);
	unsigned value;
	int ret = 0, raw;

	mutex_lock(&c->lock);
	switch (p) {
	case POWER_SUPPLY_PROP_PRESENT:
		ret = regmap_read(c->map, 0x00e, &value);
		if (!ret && (value & 5) != 5) ret = -ENODATA;
		if (!ret) v->intval = !(value & BIT(12));
		break;
	case POWER_SUPPLY_PROP_STATUS:
		v->intval = c->sample_error || c->stop_error ? POWER_SUPPLY_STATUS_UNKNOWN : c->status;
		break;
	case POWER_SUPPLY_PROP_HEALTH:
		v->intval = c->fault & (Y2_FAULT_OVP | Y2_FAULT_VOLTAGE) ? POWER_SUPPLY_HEALTH_OVERVOLTAGE :
			c->fault & Y2_FAULT_WATCHDOG ? POWER_SUPPLY_HEALTH_WATCHDOG_TIMER_EXPIRE :
			c->fault & Y2_FAULT_TIMEOUT ? POWER_SUPPLY_HEALTH_SAFETY_TIMER_EXPIRE :
			c->fault ? POWER_SUPPLY_HEALTH_UNSPEC_FAILURE : POWER_SUPPLY_HEALTH_UNKNOWN;
		/* Unknown includes unmeasured pack temperature; never claim Good. */
		break;
	case POWER_SUPPLY_PROP_VOLTAGE_NOW:
		ret = iio_read_channel_raw(c->battery, &raw);
		if (ret >= 0) { v->intval = y2_battery_uv(raw); ret = 0; }
		break;
	case POWER_SUPPLY_PROP_CONSTANT_CHARGE_CURRENT:
		ret = regmap_read(c->map, 0x008, &value);
		if (!ret) v->intval = y2_charge_ua(value);
		break;
	case POWER_SUPPLY_PROP_CONSTANT_CHARGE_VOLTAGE:
		ret = regmap_read(c->map, 0x006, &value);
		if (!ret) v->intval = y2_charge_uv(value);
		break;
	case POWER_SUPPLY_PROP_CHARGE_BEHAVIOUR: v->intval = c->behaviour; break;
	default: ret = -EINVAL;
	}
	mutex_unlock(&c->lock);
	return ret;
}
static int y2_bat_set(struct power_supply *psy, enum power_supply_property p,
		      const union power_supply_propval *v)
{
	struct y2_charger *c = power_supply_get_drvdata(psy);
	int ret = 0;
	if (p != POWER_SUPPLY_PROP_CHARGE_BEHAVIOUR ||
	    (v->intval != POWER_SUPPLY_CHARGE_BEHAVIOUR_AUTO &&
	     v->intval != POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE)) return -EINVAL;
	mutex_lock(&c->lock);
	c->behaviour = v->intval;
	y2_charge_account_now(c);
	if (c->behaviour == POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE) {
		ret = y2_charger_inhibit(c);
		c->status = ret ? POWER_SUPPLY_STATUS_UNKNOWN : POWER_SUPPLY_STATUS_NOT_CHARGING;
	}
	if (!c->stopping && !c->paused) mod_delayed_work(c->wq, &c->work, 0);
	mutex_unlock(&c->lock);
	power_supply_changed(c->bat);
	return ret;
}
static int y2_bat_writeable(struct power_supply *psy, enum power_supply_property p)
{
	return p == POWER_SUPPLY_PROP_CHARGE_BEHAVIOUR;
}
static int y2_usb_get(struct power_supply *psy, enum power_supply_property p,
		      union power_supply_propval *v)
{
	struct y2_charger *c = power_supply_get_drvdata(psy);
	unsigned value;
	int ret;
	if (p == POWER_SUPPLY_PROP_USB_TYPE) {
		mutex_lock(&c->lock);
		v->intval = !c->source_valid || READ_ONCE(c->source_changed) ? POWER_SUPPLY_USB_TYPE_UNKNOWN :
			c->source == Y2_SOURCE_SDP ? POWER_SUPPLY_USB_TYPE_SDP :
			c->source == Y2_SOURCE_CDP ? POWER_SUPPLY_USB_TYPE_CDP :
			c->source == Y2_SOURCE_DCP ? POWER_SUPPLY_USB_TYPE_DCP :
			c->source == Y2_SOURCE_APPLE ? POWER_SUPPLY_USB_TYPE_APPLE_BRICK_ID :
			POWER_SUPPLY_USB_TYPE_UNKNOWN;
		mutex_unlock(&c->lock);
		return 0;
	}
	if (p != POWER_SUPPLY_PROP_ONLINE) return -EINVAL;
	ret = regmap_read(c->map, 0, &value);
	if (!ret) v->intval = !!(value & BIT(5));
	return ret;
}
static const struct power_supply_desc y2_bat_desc = {
	.name = "BAT0", .type = POWER_SUPPLY_TYPE_BATTERY,
	.properties = y2_bat_props, .num_properties = ARRAY_SIZE(y2_bat_props),
	.get_property = y2_bat_get, .set_property = y2_bat_set,
	.property_is_writeable = y2_bat_writeable,
	.charge_behaviours = BIT(POWER_SUPPLY_CHARGE_BEHAVIOUR_AUTO) |
		BIT(POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE),
};
static const struct power_supply_desc y2_usb_desc = {
	/* Retain the existing production/rescue external-power ABI. */
	.name = "y2-usb-presence", .type = POWER_SUPPLY_TYPE_USB,
	.properties = y2_usb_props, .num_properties = ARRAY_SIZE(y2_usb_props), .get_property = y2_usb_get,
	.usb_types = BIT(POWER_SUPPLY_USB_TYPE_UNKNOWN) | BIT(POWER_SUPPLY_USB_TYPE_SDP) |
		BIT(POWER_SUPPLY_USB_TYPE_CDP) | BIT(POWER_SUPPLY_USB_TYPE_DCP) |
		BIT(POWER_SUPPLY_USB_TYPE_APPLE_BRICK_ID),
};

static const char *y2_charger_phase(struct y2_charger *c)
{
	if (c->fault) return "FAULT";
	if (!c->online) return "NO_SOURCE";
	if (!c->source_valid || READ_ONCE(c->source_changed)) return "SOURCE_DETECT";
	if (c->cycle.hold) return c->cycle.full ? "FULL" : "HOLD";
	if (!c->active) return "INHIBITED";
	if (c->uv <= Y2_PRECHARGE_UV) return "PRECHARGE";
	if (c->con4 & BIT(6)) return "CONSTANT_VOLTAGE";
	return "CONSTANT_CURRENT";
}
static ssize_t charging_state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_charger *c = power_supply_get_drvdata(dev_get_drvdata(dev));
	ssize_t n;
	mutex_lock(&c->lock);
	n = sysfs_emit(buf, "phase=%s source=%u source_valid=%u charge_limit_ua=%u\n"
		"active=%u online=%u present=%u fault=0x%x sample_error=%d stop_error=%d paused=%u last_error=%d\n"
		"battery_uv=%d baton_raw=%d isense_raw=%d pmic_die_mc=%d input_budget_ua=%d\n"
		"chr_con0=0x%04x cv_status=0x%04x battery_ovp=0x%04x baton=0x%04x watchdog=0x%04x thermal=0x%04x\n"
		"watchdog_pets=%llu last_pet_ms=%llu total_seconds=%llu cv_seconds=%llu precharge_seconds=%llu hold=%d full=%d timeout=%d\n"
		"first_fault=0x%x at_ms=%llu battery_uv=%d sample_error=%d chr=0x%x cv=0x%x ovp=0x%x watchdog=0x%x thermal=0x%x\n",
		y2_charger_phase(c), c->source, c->source_valid, c->charge_ua,
		c->active, c->online, c->present, c->fault, c->sample_error, c->stop_error, c->paused, c->last_error,
		c->uv, c->baton_raw, c->isense_raw, c->die_mc, c->input_ua,
		c->con0, c->con4, c->ov, c->baton_con, c->wdt, c->thr,
		c->pets, c->last_pet_ms, c->cycle.total, c->cycle.cv, c->cycle.precharge, c->cycle.hold, c->cycle.full, c->cycle.timed_out,
		c->first_fault, c->first_fault_ms, c->first_uv, c->first_sample_error,
		c->first_con0, c->first_cv, c->first_ov, c->first_wdt, c->first_thr);
	mutex_unlock(&c->lock);
	return n;
}
static DEVICE_ATTR_RO(charging_state);
static struct attribute *y2_charge_attrs[] = { &dev_attr_charging_state.attr, NULL };
ATTRIBUTE_GROUPS(y2_charge);

static int y2_charger_prepare_pm(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	int ret;
	mutex_lock(&c->lock);
	/* Stock Y2 held a wake lock on USB. Reject system sleep while active,
	 * including direct `echo mem`, before any provider is suspended. The
	 * workqueue is not freezable, so watchdog servicing survives PM entry. */
	if (c->active || c->stop_error) {
		mutex_unlock(&c->lock);
		return -EBUSY;
	}
	c->paused = true;
	ret = y2_charger_inhibit(c);
	if (ret) c->paused = false;
	mutex_unlock(&c->lock);
	if (!ret) cancel_delayed_work_sync(&c->work);
	return ret;
}
static void y2_charger_complete_pm(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	mutex_lock(&c->lock);
	c->paused = false;
	if (!c->stopping) mod_delayed_work(c->wq, &c->work, 0);
	mutex_unlock(&c->lock);
}
static int y2_charger_suspend(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	int ret;
	/* Arm before the MFD late-suspend callback applies its selected wake mask. */
	if (!device_may_wakeup(dev)) return 0;
	ret = enable_irq_wake(c->irq);
	if (!ret) c->irq_wake = true;
	return ret;
}
static int y2_charger_resume(struct device *dev)
{
	struct y2_charger *c = dev_get_drvdata(dev);
	int ret = 0;
	if (c->irq_wake) {
		ret = disable_irq_wake(c->irq);
		if (!ret) c->irq_wake = false;
	}
	return ret;
}
static void y2_charger_shutdown(struct platform_device *pdev)
{
	struct y2_charger *c = platform_get_drvdata(pdev);
	mutex_lock(&c->lock);
	c->stopping = true;
	mutex_unlock(&c->lock);
	cancel_delayed_work_sync(&c->work);
	mutex_lock(&c->lock);
	y2_charger_inhibit(c);
	mutex_unlock(&c->lock);
}
static void y2_charger_release(void *data)
{
	struct y2_charger *c = data;
	WRITE_ONCE(c->stopping, true);
	cancel_delayed_work_sync(&c->work);
	y2_charger_inhibit(c);
	destroy_workqueue(c->wq);
	if (c->irq_wake) disable_irq_wake(c->irq);
	device_init_wakeup(c->dev, false);
}
static void y2_charger_notifier_release(void *data)
{
	struct y2_charger *c = data;
	power_supply_unreg_notifier(&c->input_nb);
}
static void y2_charger_quiesce(void *data)
{
	struct y2_charger *c = data;
	mutex_lock(&c->lock);
	c->stopping = true;
	mutex_unlock(&c->lock);
	cancel_delayed_work_sync(&c->work);
	mutex_lock(&c->lock);
	y2_charger_inhibit(c);
	mutex_unlock(&c->lock);
}
static int y2_charger_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct mt6397_chip *chip = dev_get_drvdata(dev->parent);
	struct power_supply_config cfg = {};
	struct y2_charger *c;
	int ret, irq;

	if (!chip || !chip->regmap) return -EPROBE_DEFER;
	c = devm_kzalloc(dev, sizeof(*c), GFP_KERNEL);
	if (!c) return -ENOMEM;
	c->dev = dev; c->map = chip->regmap;
	c->io = (struct y2_charge_io){ c->map, y2_charge_read, y2_charge_update };
	mutex_init(&c->lock);
	c->status = POWER_SUPPLY_STATUS_UNKNOWN;
	c->stopping = true; /* notifier cannot queue work until all devres are ready */
	c->sample_error = -EAGAIN;
	/* No dependency can defer leaving an inherited charger enabled. */
	ret = y2_charge_stop(&c->io);
	if (ret) return dev_err_probe(dev, ret, "charging inhibit\n");
	c->battery = devm_iio_channel_get(dev, "battery-voltage");
	if (IS_ERR(c->battery)) return dev_err_probe(dev, PTR_ERR(c->battery), "battery ADC\n");
	c->baton = devm_iio_channel_get(dev, "baton");
	if (IS_ERR(c->baton)) return dev_err_probe(dev, PTR_ERR(c->baton), "BATON ADC\n");
	c->isense = devm_iio_channel_get(dev, "isense");
	if (IS_ERR(c->isense)) return dev_err_probe(dev, PTR_ERR(c->isense), "ISENSE ADC\n");
	c->die = devm_iio_channel_get(dev, "pmic-temperature");
	if (IS_ERR(c->die)) return dev_err_probe(dev, PTR_ERR(c->die), "PMIC die ADC\n");
	c->input = devm_power_supply_get_by_reference(dev, "power-supplies");
	if (IS_ERR(c->input)) return dev_err_probe(dev, PTR_ERR(c->input), "USB input supply\n");
	if (!c->input) return -EPROBE_DEFER;
	if (!device_link_add(dev, c->input->dev.parent, DL_FLAG_AUTOREMOVE_CONSUMER)) return -ENOMEM;
	c->wq = alloc_ordered_workqueue("y2-charger", WQ_HIGHPRI | WQ_MEM_RECLAIM);
	if (!c->wq) return -ENOMEM;
	INIT_DELAYED_WORK(&c->work, y2_charger_poll);
	ret = device_init_wakeup(dev, true);
	if (ret) { destroy_workqueue(c->wq); return ret; }
	ret = devm_add_action_or_reset(dev, y2_charger_release, c);
	if (ret) return ret;
	cfg.drv_data = c; cfg.fwnode = dev_fwnode(dev); cfg.attr_grp = y2_charge_groups;
	c->bat = devm_power_supply_register(dev, &y2_bat_desc, &cfg);
	if (IS_ERR(c->bat)) return PTR_ERR(c->bat);
	cfg.attr_grp = NULL;
	c->usb = devm_power_supply_register(dev, &y2_usb_desc, &cfg);
	if (IS_ERR(c->usb)) return PTR_ERR(c->usb);
	platform_set_drvdata(pdev, c);
	irq = platform_get_irq(pdev, 0);
	if (irq < 0) return irq;
	ret = devm_request_threaded_irq(dev, irq, NULL, y2_charger_irq, IRQF_ONESHOT, "mt6323-chrdet", c);
	if (ret) return ret;
	c->irq = irq;
	c->input_nb.notifier_call = y2_charger_input_changed;
	ret = power_supply_reg_notifier(&c->input_nb);
	if (ret) return ret;
	ret = devm_add_action_or_reset(dev, y2_charger_notifier_release, c);
	if (ret) return ret;
	/* This action precedes power_supply/IRQ teardown, unlike allocation cleanup. */
	ret = devm_add_action_or_reset(dev, y2_charger_quiesce, c);
	if (ret) return ret;
	WRITE_ONCE(c->stopping, false);
	queue_delayed_work(c->wq, &c->work, msecs_to_jiffies(Y2_POLL_MS));
	dev_info(dev, "stock-derived BC1.1 charging: 70/450/650mA, 4.175V; pack temp/current/SOC unavailable\n");
	return 0;
}
static const struct dev_pm_ops y2_charger_pm = {
	.prepare = y2_charger_prepare_pm, .complete = y2_charger_complete_pm,
	.suspend = y2_charger_suspend, .resume = y2_charger_resume,
};
static const struct of_device_id y2_charger_match[] = { { .compatible = "innioasis,y2-mt6323-charger" }, {} };
static struct platform_driver y2_charger_driver = {
	.probe = y2_charger_probe, .shutdown = y2_charger_shutdown,
	.driver = { .name = "y2-mt6323-charger", .of_match_table = y2_charger_match,
		.pm = pm_sleep_ptr(&y2_charger_pm), .suppress_bind_attrs = true },
};
module_platform_driver(y2_charger_driver);
MODULE_LICENSE("GPL");
