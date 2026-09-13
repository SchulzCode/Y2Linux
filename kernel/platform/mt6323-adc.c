// SPDX-License-Identifier: GPL-2.0-only
/* MT6323 AUXADC missing from upstream 6.18. Vendor request/ready and efuse
 * semantics retained; all transfers share the MFD regmap, including failures. */
#include <linux/delay.h>
#include <linux/iio/iio.h>
#include <linux/mfd/mt6397/core.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/property.h>
#include <linux/regmap.h>
#include <linux/thermal.h>
#include "power-math.h"

struct y2_adc {
	struct regmap *map;
	struct mutex lock;
	struct y2_pmic_cal cal;
	bool calibrated;
};
static int y2_adc_sample(struct y2_adc *a, unsigned channel, int *raw)
{
	unsigned saved, val, reg;
	int ret, restore;
	switch (channel) {
	case 7: reg = 0x714; break; /* BATSNS, internal 4:1 divider */
	case 3: reg = 0x71c; break; /* THR_SENSE1, PMIC die */
	default: return -EINVAL;
	}
	mutex_lock(&a->lock);
	ret = regmap_read(a->map, 0x758, &saved); /* AUXADC_CON11 VBUF_EN */
	if (ret) goto out;
	ret = regmap_update_bits(a->map, 0x758, BIT(4), BIT(4));
	if (ret) goto out;
	ret = regmap_update_bits(a->map, 0x76e, BIT(channel), 0);
	if (ret) goto restore;
	ret = regmap_update_bits(a->map, 0x76e, BIT(channel), BIT(channel));
	if (ret) goto restore;
	/* BSP requires 1 ms settling for these channels. A timeout is an error,
	 * never a request to return the last conversion as a fresh measurement. */
	usleep_range(1000, 1500);
	ret = regmap_read_poll_timeout(a->map, reg, val, val & BIT(15), 1000, 20000);
	if (!ret) *raw = val & 0x7fff;
restore:
	restore = regmap_update_bits(a->map, 0x758, BIT(4), saved & BIT(4));
	if (!ret) ret = restore;
out:
	mutex_unlock(&a->lock);
	return ret;
}
static int y2_adc_read(struct iio_dev *indio, const struct iio_chan_spec *chan,
		       int *val, int *val2, long mask)
{
	struct y2_adc *a = iio_priv(indio);
	int ret, raw;
	if (mask == IIO_CHAN_INFO_SCALE && chan->type == IIO_VOLTAGE) {
		*val = 7200; *val2 = 15; /* mV, factory hardware trimming retained */
		return IIO_VAL_FRACTIONAL_LOG2;
	}
	if (mask != IIO_CHAN_INFO_RAW && mask != IIO_CHAN_INFO_PROCESSED) return -EINVAL;
	if (mask == IIO_CHAN_INFO_PROCESSED && !a->calibrated) return -ENODATA;
	ret = y2_adc_sample(a, chan->channel, &raw);
	if (ret) return ret;
	if (mask == IIO_CHAN_INFO_PROCESSED && (!raw || raw == 0x7fff)) return -ENODATA;
	*val = mask == IIO_CHAN_INFO_RAW ? raw : y2_pmic_mc(&a->cal, raw);
	return IIO_VAL_INT;
}
static int y2_adc_xlate(struct iio_dev *indio, const struct fwnode_reference_args *args)
{
	unsigned i;
	if (args->nargs != 1) return -EINVAL;
	for (i = 0; i < indio->num_channels; i++)
		if (indio->channels[i].channel == args->args[0]) return i;
	return -EINVAL;
}
static const struct iio_info y2_adc_info = { .read_raw = y2_adc_read, .fwnode_xlate = y2_adc_xlate };
static const struct iio_chan_spec y2_adc_channels[] = {
	{ .type = IIO_VOLTAGE, .indexed = 1, .channel = 7,
	  .datasheet_name = "BATSNS", .info_mask_separate = BIT(IIO_CHAN_INFO_RAW) | BIT(IIO_CHAN_INFO_SCALE) },
	{ .type = IIO_TEMP, .indexed = 1, .channel = 3,
	  .datasheet_name = "THR_SENSE1", .info_mask_separate = BIT(IIO_CHAN_INFO_RAW) | BIT(IIO_CHAN_INFO_PROCESSED) },
};
static int y2_pmic_temp(struct thermal_zone_device *tz, int *temp)
{
	struct y2_adc *a = thermal_zone_device_priv(tz);
	int raw, ret = y2_adc_sample(a, 3, &raw);
	if (!ret && (!raw || raw == 0x7fff)) return -ENODATA;
	if (!ret) *temp = y2_pmic_mc(&a->cal, raw);
	return ret;
}
static const struct thermal_zone_device_ops y2_pmic_thermal_ops = { .get_temp = y2_pmic_temp };
static int y2_adc_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct mt6397_chip *chip = dev_get_drvdata(dev->parent);
	struct iio_dev *indio;
	struct y2_adc *a;
	struct thermal_zone_device *tz;
	unsigned cid, fuse0, fuse1;
	int ret;
	if (!chip || !chip->regmap) return -EPROBE_DEFER;
	indio = devm_iio_device_alloc(dev, sizeof(*a));
	if (!indio) return -ENOMEM;
	a = iio_priv(indio); a->map = chip->regmap;
	mutex_init(&a->lock);
	ret = regmap_read(a->map, 0x100, &cid);
	if (!ret) ret = regmap_read(a->map, 0x63a, &fuse0);
	if (!ret) ret = regmap_read(a->map, 0x63c, &fuse1);
	if (ret) return dev_err_probe(dev, ret, "ADC efuse read\n");
	a->calibrated = !y2_pmic_calibrate(cid, fuse0, fuse1, &a->cal);
	indio->name = "mt6323-auxadc"; indio->info = &y2_adc_info;
	indio->modes = INDIO_DIRECT_MODE;
	indio->channels = y2_adc_channels; indio->num_channels = ARRAY_SIZE(y2_adc_channels);
	ret = devm_iio_device_register(dev, indio);
	if (ret) return ret;
	if (a->calibrated) {
		tz = devm_thermal_of_zone_register(dev, 0, a, &y2_pmic_thermal_ops);
		if (IS_ERR(tz)) return dev_err_probe(dev, PTR_ERR(tz), "PMIC thermal zone\n");
	} else dev_warn(dev, "no valid die calibration; temperature unavailable\n");
	dev_info(dev, "BATSNS 15-bit 1.8 V x4; die calibration=%u; no pack temperature/current/SOC\n", a->calibrated);
	return 0;
}
static const struct of_device_id y2_adc_match[] = { { .compatible = "mediatek,mt6323-auxadc" }, {} };
static struct platform_driver y2_adc_driver = {
	.probe = y2_adc_probe,
	.driver = { .name = "mt6323-auxadc", .of_match_table = y2_adc_match },
};
module_platform_driver(y2_adc_driver);
MODULE_LICENSE("GPL");
