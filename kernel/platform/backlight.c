// SPDX-License-Identifier: GPL-2.0
/* MT6323 four-sink LCD backlight, informed by Chris Hendrickson's GPL
 * mt6323-backlight.c (53fb57bb99c9) and upstream leds-mt6323.
 * Fixed inherited current/clock/mode, linear PWM only. Zero disables sinks.
 * Maximum never exceeds the lowest inherited duty; no guessed current ceiling.
 */
#include <linux/backlight.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/regmap.h>
struct y2_bl {
	struct regmap *map;
	struct mutex lock;
	unsigned ceiling;
};
static int y2_bl_update(struct backlight_device *bl)
{
	struct y2_bl *b = bl_get_data(bl);
	unsigned old[4], en, i, level = backlight_get_brightness(bl);
	int ret;
	if (level > b->ceiling)
		return -EINVAL;
	mutex_lock(&b->lock);
	ret = regmap_read(b->map, 0x356, &en);
	if (ret)
		goto out;
	for (i = 0; i < 4; i++) {
		ret = regmap_read(b->map, 0x330 + 8 * i, &old[i]);
		if (ret)
			goto out;
	}
	/* Dark during a multi-register transaction; no partially updated bright frame. */
	ret = regmap_update_bits(b->map, 0x356, 15, 0);
	if (ret)
		goto out;
	if (level) {
		for (i = 0; i < 4; i++) {
			ret = regmap_update_bits(b->map, 0x330 + 8 * i, 0x1f00, (level - 1) << 8);
			if (ret)
				goto rollback;
		}
		ret = regmap_update_bits(b->map, 0x356, 15, 15);
		if (ret)
			goto rollback;
	}
	goto out;
rollback:
	/* Keep dark on a failed restore. Never report a failed PMIC write as success. */
	for (i = 0; i < 4; i++)
		if (regmap_write(b->map, 0x330 + 8 * i, old[i]))
			goto out;
	if (regmap_update_bits(b->map, 0x356, 15, en & 15))
		dev_err(&bl->dev, "backlight restore failed; remain dark\n");
out:
	mutex_unlock(&b->lock);
	return ret;
}
static int y2_bl_read(struct backlight_device *bl)
{
    struct y2_bl *b = bl_get_data(bl);
    unsigned en, pwm, current_step;
    int ret;
    mutex_lock(&b->lock);
    ret = regmap_read(b->map, 0x356, &en);
    if (!ret) ret = regmap_read(b->map, 0x330, &pwm);
    if (!ret) ret = regmap_read(b->map, 0x334, &current_step);
    if (!ret) {
        dev_info_ratelimited(&bl->dev, "Y2SCAN backlight enable=%04x pwm=%04x current=%04x\n", en,pwm,current_step);
        ret = (en & 15) ? ((pwm >> 8) & 31) + 1 : 0;
    }
    mutex_unlock(&b->lock);
    return ret;
}
static const struct backlight_ops y2_bl_ops = {.update_status = y2_bl_update, .get_brightness = y2_bl_read};
static int y2_bl_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct y2_bl *b;
	struct backlight_device *bl;
	struct backlight_properties props = {.type = BACKLIGHT_RAW};
	unsigned i, v, en, step = 0;
	int ret;
	b = devm_kzalloc(dev, sizeof(*b), GFP_KERNEL);
	if (!b)
		return -ENOMEM;
	b->map = dev_get_regmap(dev->parent, NULL);
	if (!b->map)
		return -EPROBE_DEFER;
	mutex_init(&b->lock);
	b->ceiling = 32;
	ret = regmap_read(b->map, 0x356, &en);
	if (ret)
		return ret;
	if ((en & 15) != 15)
		return dev_err_probe(dev, -ENODEV, "not four inherited active ISINKs\n");
	for (i = 0; i < 4; i++) {
		ret = regmap_read(b->map, 0x330 + 8 * i, &v);
		if (ret)
			return ret;
		if (v & 0x0c)
			return dev_err_probe(dev, -ENODEV, "ISINK%u not PWM\n", i);
		b->ceiling = min(b->ceiling, ((v >> 8) & 31) + 1);
		ret = regmap_read(b->map, 0x334 + 8 * i, &v);
		if (ret)
			return ret;
		if (!i)
			step = v & 0x7000;
		if ((v & 0x7000) != step)
			return dev_err_probe(dev, -ENODEV, "unequal inherited currents\n");
	}
	props.max_brightness = b->ceiling;
	props.brightness = b->ceiling;
	bl = devm_backlight_device_register(dev, "y2-backlight", dev, b, &y2_bl_ops, &props);
	if (IS_ERR(bl))
		return PTR_ERR(bl);
	dev_info(dev, "four ISINK PWM; max duty=%u/32 inherited current step=%u unchanged\n",
		 b->ceiling, step >> 12);
	return 0; /* No startup write or implicit maximum-current request. */
}
static const struct of_device_id y2_bl_match[] = {{.compatible = "innioasis,y2-backlight"}, {}};
static struct platform_driver y2_bl_driver = {
    .probe = y2_bl_probe, .driver = {.name = "y2-backlight", .of_match_table = y2_bl_match}};
module_platform_driver(y2_bl_driver);
MODULE_LICENSE("GPL");
