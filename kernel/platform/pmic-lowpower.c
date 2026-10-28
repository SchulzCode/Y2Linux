// SPDX-License-Identifier: GPL-2.0-only
/* Static MT6323 low-power select for the LDOs, as the stock kernel's PMIC init
 * does. Writes go through the PWRAP transport, whose policy admits only bit 0 of
 * the registers in pmic-lowpower-policy.h. Applied once after the
 * PMIC is up, verified by readback, and revertible at run time:
 *   /sys/firmware/y2_pmic/lp_sel  (0600)  write 1 = set, 0 = clear, read = state
 * No voltage, enable, clock or sleep-voltage setting is changed. The same pass
 * sets the stock VIBR thermal shutdown once (#28); the switch leaves it set.
 * /sys/firmware/y2_pmic/power_status (0444) reads the PMIC's power-down record;
 * /sys/firmware/y2_pmic/long_press_reset (0200) takes only "0" (F57). */
#include <linux/capability.h>
#include <linux/delay.h>
#include <linux/init.h>
#include <linux/kobject.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/of_platform.h>
#include <linux/platform_device.h>
#include <linux/regmap.h>
#include <linux/sysfs.h>
#include <linux/workqueue.h>
#include "pmic-lowpower-policy.h"

static struct regmap *lp_map;
static DEFINE_MUTEX(lp_lock);
static struct delayed_work lp_work;
static unsigned lp_attempts;
static bool lp_ready;
static int lp_error;

static int lp_apply(bool set)
{
	unsigned i, value = 0;
	int ret = 0;
	for (i = 0; i < Y2_PMIC_LP_RAILS; i++) {
		ret = regmap_update_bits(lp_map, y2_pmic_lp_rails[i].reg, Y2_PMIC_LP_BIT,
					 set ? Y2_PMIC_LP_BIT : 0);
		if (!ret)
			ret = regmap_read(lp_map, y2_pmic_lp_rails[i].reg, &value);
		if (!ret && !!(value & Y2_PMIC_LP_BIT) != set)
			ret = -EIO;
		if (ret)
			break;
	}
	return ret;
}

static struct regmap *lp_find_map(void)
{
	struct device_node *node = of_find_compatible_node(NULL, NULL, "innioasis,y2-pwrap");
	struct platform_device *pdev;
	struct regmap *map;
	if (!node)
		return NULL;
	pdev = of_find_device_by_node(node);
	of_node_put(node);
	if (!pdev)
		return NULL;
	map = dev_get_regmap(&pdev->dev, NULL);
	put_device(&pdev->dev);
	return map;
}

static void lp_start(struct work_struct *work)
{
	unsigned cid = 0;
	int ret;
	mutex_lock(&lp_lock);
	if (!lp_map)
		lp_map = lp_find_map();
	if (!lp_map) {
		mutex_unlock(&lp_lock);
		if (++lp_attempts < 10)
			schedule_delayed_work(&lp_work, 5 * HZ);
		return;
	}
	ret = regmap_read(lp_map, Y2_PMIC_CID_REG, &cid);
	if (ret || !y2_pmic_lp_chip_ok(cid)) {
		lp_error = ret ? ret : -ENODEV;
		pr_warn("y2-pmic-lp: not applied (CID=%#x err=%d); vendor E2 map required\n", cid, lp_error);
		mutex_unlock(&lp_lock);
		return;
	}
	lp_error = lp_apply(true);
	lp_ready = !lp_error;
	pr_info("y2-pmic-lp: LP_SEL %s on VA/VIO28/VUSB/VM/VIO18/VMC/VMCH/VEMC3V3 (CID=%#x err=%d)\n",
		lp_ready ? "set" : "FAILED", cid, lp_error);
	ret = regmap_update_bits(lp_map, Y2_PMIC_VIBR_REG, Y2_PMIC_VIBR_THR, Y2_PMIC_VIBR_THR);
	if (ret)
		pr_warn("y2-pmic-lp: VIBR thermal shutdown not set (err=%d)\n", ret);
	mutex_unlock(&lp_lock);
}

static ssize_t lp_sel_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	unsigned i, value;
	ssize_t n;
	mutex_lock(&lp_lock);
	n = sysfs_emit(buf, "applied=%u error=%d", lp_ready, lp_error);
	for (i = 0; lp_map && i < Y2_PMIC_LP_RAILS; i++)
		n += sysfs_emit_at(buf, n, " %s=%u", y2_pmic_lp_rails[i].name,
				   regmap_read(lp_map, y2_pmic_lp_rails[i].reg, &value) ? 9U :
				   !!(value & Y2_PMIC_LP_BIT));
	if (lp_map)
		n += sysfs_emit_at(buf, n, " VIBR_THR=%u",
				   regmap_read(lp_map, Y2_PMIC_VIBR_REG, &value) ? 9U :
				   !!(value & Y2_PMIC_VIBR_THR));
	n += sysfs_emit_at(buf, n, "\n");
	mutex_unlock(&lp_lock);
	return n;
}

static ssize_t lp_sel_store(struct kobject *k, struct kobj_attribute *a, const char *buf, size_t size)
{
	int set, ret;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	set = y2_pmic_lp_parse(buf, size);
	if (set < 0)
		return -EINVAL;
	mutex_lock(&lp_lock);
	if (!lp_map) {
		mutex_unlock(&lp_lock);
		return -ENODEV;
	}
	ret = lp_apply(set);
	lp_ready = !ret && set;
	lp_error = ret;
	mutex_unlock(&lp_lock);
	return ret ? ret : size;
}
static struct kobj_attribute lp_sel_attr = __ATTR(lp_sel, 0600, lp_sel_show, lp_sel_store);

/* F56/F57: the PMIC's power-down record (pmic-lowpower-policy.h), raw and
 * read only; y2-platform decodes it into the previous-boot evidence. */
static ssize_t power_status_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	unsigned i, value;
	ssize_t n;
	mutex_lock(&lp_lock);
	if (!lp_map)
		lp_map = lp_find_map();
	n = sysfs_emit(buf, "valid=%u", lp_map ? 1U : 0U);
	for (i = 0; lp_map && i < Y2_PMIC_POWER_STATUS_REGS; i++) {
		if (regmap_read(lp_map, y2_pmic_power_status[i].reg, &value))
			n += sysfs_emit_at(buf, n, " %s=error", y2_pmic_power_status[i].name);
		else
			n += sysfs_emit_at(buf, n, " %s=0x%04x", y2_pmic_power_status[i].name, value);
	}
	n += sysfs_emit_at(buf, n, "\n");
	mutex_unlock(&lp_lock);
	return n;
}
static struct kobj_attribute power_status_attr = __ATTR(power_status, 0444, power_status_show, NULL);

/* F57: y2-platform turns the Power long-press reset off when it executes a
 * shutdown (pmic-lowpower-policy.h). Off only, until the next start. */
static ssize_t long_press_reset_store(struct kobject *k, struct kobj_attribute *a,
				      const char *buf, size_t size)
{
	unsigned value = 0;
	int ret;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	if (y2_pmic_lp_parse(buf, size) != 0)
		return -EINVAL;
	mutex_lock(&lp_lock);
	if (!lp_map)
		lp_map = lp_find_map();
	ret = lp_map ? regmap_update_bits(lp_map, Y2_PMIC_TOP_RST_MISC, Y2_PMIC_PWRKEY_RST_EN, 0) : -ENODEV;
	if (!ret)
		ret = regmap_read(lp_map, Y2_PMIC_TOP_RST_MISC, &value);
	if (!ret && (value & Y2_PMIC_PWRKEY_RST_EN))
		ret = -EIO;
	mutex_unlock(&lp_lock);
	pr_info("y2-pmic-lp: long-press reset %s for the shutdown (top_rst_misc=0x%04x err=%d)\n",
		ret ? "still on" : "off", value, ret);
	return ret ? ret : size;
}
static struct kobj_attribute long_press_reset_attr = __ATTR(long_press_reset, 0200, NULL, long_press_reset_store);

static int __init y2_pmic_lp_init(void)
{
	struct kobject *k;
	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	k = kobject_create_and_add("y2_pmic", firmware_kobj);
	if (k && sysfs_create_file(k, &lp_sel_attr.attr)) {
		kobject_put(k);
	} else if (k) {
		if (sysfs_create_file(k, &power_status_attr.attr))
			pr_warn("y2-pmic-lp: power_status not created\n");
		if (sysfs_create_file(k, &long_press_reset_attr.attr))
			pr_warn("y2-pmic-lp: long_press_reset not created\n");
	}
	INIT_DELAYED_WORK(&lp_work, lp_start);
	schedule_delayed_work(&lp_work, 10 * HZ);
	return 0;
}
late_initcall_sync(y2_pmic_lp_init);
