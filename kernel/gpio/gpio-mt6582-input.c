// SPDX-License-Identifier: GPL-2.0
/*
 * MT6582 inherited GPIO inputs for the Y2 navigation slice.
 *
 * Derived from Chris Hendrickson's gpio-mt6582.c, commit
 * f0d149a2a325c0790eb816be71314ebdcaa8ee99 of artificery-dev/linux.
 * Original: "GPIO + EINT (external interrupt) controller driver for the
 * MediaTek MT6582." See docs/knowledge/donor-audit.md for provenance.
 *
 * The vendor and donor agree on 16 GPIOs per 0x10-byte register bank,
 * DIR at 0x000 and DIN at 0x500. This input-only provider preserves LK's
 * mux, pulls, direction, clocks and EINT. An output-configured line is an
 * error, never silently reconfigured. gpio-reserved-ranges restricts the
 * candidate to the five reviewed navigation lines. General pinctrl/EINT
 * ownership will replace this explicitly named handoff provider.
 */
#include <linux/gpio/driver.h>
#include <linux/io.h>
#include <linux/mod_devicetable.h>
#include <linux/platform_device.h>

#define MT6582_GPIO_DIR 0x000
#define MT6582_GPIO_DIN 0x500
#define MT6582_GPIO_MODE 0x600
#define MT6582_GPIO_COUNT 169

struct mt6582_input_gpio {
	struct gpio_chip chip;
	void __iomem *base;
};

static void __iomem *mt6582_input_reg(struct mt6582_input_gpio *g,
				   unsigned int block, unsigned int offset)
{
	return g->base + block + (offset / 16) * 0x10;
}

static int mt6582_input_get_direction(struct gpio_chip *chip, unsigned int offset)
{
	struct mt6582_input_gpio *g = gpiochip_get_data(chip);

	if (offset >= MT6582_GPIO_COUNT)
		return -EINVAL;
	return readl(mt6582_input_reg(g, MT6582_GPIO_DIR, offset)) & BIT(offset % 16) ?
		GPIO_LINE_DIRECTION_OUT : GPIO_LINE_DIRECTION_IN;
}

static int mt6582_input_direction_input(struct gpio_chip *chip, unsigned int offset)
{
	int direction = mt6582_input_get_direction(chip, offset);

	if (direction < 0)
		return direction;
	return direction == GPIO_LINE_DIRECTION_IN ? 0 : -EBUSY;
}

static int mt6582_input_get(struct gpio_chip *chip, unsigned int offset)
{
	struct mt6582_input_gpio *g = gpiochip_get_data(chip);
	int ret = mt6582_input_direction_input(chip, offset);

	if (ret)
		return ret;
	return !!(readl(mt6582_input_reg(g, MT6582_GPIO_DIN, offset)) & BIT(offset % 16));
}

static int mt6582_input_request(struct gpio_chip *chip, unsigned int offset)
{
	struct mt6582_input_gpio *g = gpiochip_get_data(chip);
	int ret = mt6582_input_direction_input(chip, offset);

	if (ret)
		return dev_err_probe(chip->parent, ret, "GPIO%u is not an inherited input\n",
				     offset);
	dev_info(chip->parent, "GPIO%u input mux=%u level=%d (unchanged)\n", offset,
		 (readl(g->base + MT6582_GPIO_MODE + (offset / 5) * 0x10) >>
		  ((offset % 5) * 3)) & 7, mt6582_input_get(chip, offset));
	return 0;
}

static int mt6582_input_probe(struct platform_device *pdev)
{
	struct mt6582_input_gpio *g;
	struct resource *res;

	res = platform_get_resource(pdev, IORESOURCE_MEM, 0);
	if (!res || resource_size(res) < 0x840)
		return -EINVAL;
	g = devm_kzalloc(&pdev->dev, sizeof(*g), GFP_KERNEL);
	if (!g)
		return -ENOMEM;
	g->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(g->base))
		return PTR_ERR(g->base);
	g->chip.label = dev_name(&pdev->dev);
	g->chip.parent = &pdev->dev;
	g->chip.base = -1;
	g->chip.ngpio = MT6582_GPIO_COUNT;
	g->chip.request = mt6582_input_request;
	g->chip.get = mt6582_input_get;
	g->chip.get_direction = mt6582_input_get_direction;
	g->chip.direction_input = mt6582_input_direction_input;
	return devm_gpiochip_add_data(&pdev->dev, &g->chip, g);
}

static const struct of_device_id mt6582_input_match[] = {
	{ .compatible = "innioasis,y2-gpio-input" },
	{ }
};

static struct platform_driver mt6582_input_driver = {
	.probe = mt6582_input_probe,
	.driver = {
		.name = "mt6582-gpio-input",
		.of_match_table = mt6582_input_match,
	},
};
builtin_platform_driver(mt6582_input_driver);
