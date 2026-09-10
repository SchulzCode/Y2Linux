// SPDX-License-Identifier: GPL-2.0
/* MT6582 minimal M2 pinctrl/GPIO/EINT.
 * GPIO/EINT access layout adapted from Chris Hendrickson's gpio-mt6582.c,
 * artificery-dev/linux f0d149a2a325c0790eb816be71314ebdcaa8ee99.
 * Mux/bias layout from MediaTek GPL mt_gpio_base.c (2011-2014).
 * Only reviewed M2 functions exposed; no guessed pad drive strength.
 */
#include <linux/gpio/driver.h>
#include "../pinctrl/mediatek/mtk-eint.h"
#include <linux/io.h>
#include <linux/module.h>
#include <linux/pinctrl/pinconf-generic.h>
#include <linux/pinctrl/pinconf.h>
#include <linux/pinctrl/pinctrl.h>
#include <linux/pinctrl/pinmux.h>
#include <linux/platform_device.h>
struct y2_pins {
	struct gpio_chip gpio;
	struct pinctrl_desc desc;
	struct pinctrl_dev *pctl;
	void __iomem *base, *eintbase;
	struct mtk_eint eint;
	struct mtk_eint_pin epins[169];
	struct pinctrl_pin_desc pins[169];
	spinlock_t lock;
};
static const unsigned nav_pins[] = {6, 7, 9, 10, 54}, wheel_pins[] = {55}, pmic_pins[] = {25},
		      lcm_pins[] = {112};
static const unsigned i2c0_pins[] = {84, 85}, i2c1_pins[] = {86, 87},
		      sd_pins[] = {124, 125, 126, 127, 128, 129}, audio_pins[] = {43, 44, 46};
static const char *const group_names[] = {"navigation", "wheel-irq", "pmic-irq", "lcm-reset",
					  "i2c0",	"i2c1",	     "msdc1", "i2s-lk"};
static const unsigned *const group_pins[] = {nav_pins,	wheel_pins, pmic_pins, lcm_pins,
					     i2c0_pins, i2c1_pins,  sd_pins, audio_pins};
static const unsigned group_counts[] = {5, 1, 1, 1, 2, 2, 6, 3};
static const char *const function_names[] = {"gpio", "eint", "lcm-reset", "i2c0", "i2c1", "msdc1", "i2s-lk"};
static int group_count(struct pinctrl_dev *p) { return ARRAY_SIZE(group_names); }
static const char *group_name(struct pinctrl_dev *p, unsigned n) { return group_names[n]; }
static int group_get(struct pinctrl_dev *p, unsigned n, const unsigned **pins, unsigned *num)
{
	*pins = group_pins[n];
	*num = group_counts[n];
	return 0;
}
static int func_count(struct pinctrl_dev *p) { return ARRAY_SIZE(function_names); }
static const char *func_name(struct pinctrl_dev *p, unsigned n) { return function_names[n]; }
static int func_groups(struct pinctrl_dev *p, unsigned n, const char *const **groups, unsigned *num)
{
	*groups = group_names + (n == 0 ? 0 : n == 1 ? 1 : n + 1);
	*num = n == 1 ? 2 : 1;
	return 0;
}
static void pin_bit(struct y2_pins *p, unsigned block, unsigned pin, bool value)
{
	writel(BIT(pin % 16), p->base + block + (pin / 16) * 16 + (value ? 4 : 8));
}
static void pin_mode(struct y2_pins *p, unsigned pin, unsigned mode)
{
	unsigned long flags;
	unsigned off = 0x600 + (pin / 5) * 16, shift = (pin % 5) * 3;
	spin_lock_irqsave(&p->lock, flags);
	writel(7 << shift, p->base + off + 8);
	writel(mode << shift, p->base + off + 4);
	spin_unlock_irqrestore(&p->lock, flags);
}
static int pin_mux(struct pinctrl_dev *pc, unsigned f, unsigned g)
{
	struct y2_pins *p = pinctrl_dev_get_drvdata(pc);
	unsigned i;
	if ((f == 0 && g != 0) || (f == 1 && (g < 1 || g > 2)) || (f >= 2 && g != f + 1))
		return -EINVAL;
	for (i = 0; i < group_counts[g]; i++) {
		unsigned pin = group_pins[g][i];
        if (f == 3 || f == 4 || f == 2)
            dev_info(p->gpio.parent, "before pin%u mux=%u dir=%u pull=%u/%u input=%u IES=%08x\n", pin,
                (readl(p->base + 0x600 + (pin / 5) * 16) >> ((pin % 5) * 3)) & 7,
                !!(readl(p->base + (pin / 16) * 16) & BIT(pin % 16)),
                !!(readl(p->base + 0x100 + (pin / 16) * 16) & BIT(pin % 16)),
                !!(readl(p->base + 0x200 + (pin / 16) * 16) & BIT(pin % 16)),
                !!(readl(p->base + 0x500 + (pin / 16) * 16) & BIT(pin % 16)),
                readl(p->base + (pin <= 85 ? 0x900 : 0x910)));
		if (f < 2)
			pin_bit(p, 0, pin, false);
		/* EINT25/55 are already selected by LK on tested donor hardware.
		 * Do not assume a universal GPIO->EINT mux function number. */
		/* The retained second-I2S hardware proof uses LK's pad mux.
		 * Claim the pins and retain that mux; report it for physical review. */
		if (f == 6 && !((readl(p->base + 0x600 + (pin / 5) * 16) >>
				 ((pin % 5) * 3)) & 7))
			return dev_err_probe(p->gpio.parent, -EINVAL,
					     "I2S pin%u lacks inherited peripheral mux\n", pin);
		if (f != 1 && f != 6)
			pin_mode(p, pin, f >= 2 ? 1 : 0);
		dev_info(p->gpio.parent, "pin%u mode=%u function=%s\n", pin,
			 (readl(p->base + 0x600 + (pin / 5) * 16) >> ((pin % 5) * 3)) & 7,
			 function_names[f]);
	}
	return 0;
}
static int pin_config_set(struct pinctrl_dev *pc, unsigned pin, unsigned long *configs, unsigned n)
{
	struct y2_pins *p = pinctrl_dev_get_drvdata(pc);
	unsigned i;
	/* Regular bias cells only. Dedicated MSDC/keypad cells require a separate
	 * reviewed map and cannot fall through to this register layout. */
	if (pin >= 124 && pin <= 129) {
		unsigned off = pin == 124 ? 0xc50 : pin == 125 ? 0xc40 : 0xc60;
		unsigned shift = pin <= 125 ? 0 : 2 * (pin - 126);
		for (i = 0; i < n; i++) {
			enum pin_config_param param = pinconf_to_config_param(configs[i]);
			if (param != PIN_CONFIG_BIAS_DISABLE && param != PIN_CONFIG_BIAS_PULL_UP &&
			    param != PIN_CONFIG_BIAS_PULL_DOWN)
				return -ENOTSUPP;
		}
		for (i = 0; i < n; i++) {
			enum pin_config_param param = pinconf_to_config_param(configs[i]);
			unsigned v = param == PIN_CONFIG_BIAS_DISABLE	? 0
				     : param == PIN_CONFIG_BIAS_PULL_UP ? 2
									: 1;
			writel(3 << shift, p->base + off + 8);
			writel(v << shift, p->base + off + 4);
		}
		return 0;
	}
	if (pin != 6 && pin != 7 && pin != 9 && pin != 10 && pin != 25 && pin != 54 && pin != 55 &&
	    pin != 112 && pin != 84 && pin != 85 && pin != 86 && pin != 87)
		return -ENOTSUPP;
	for (i = 0; i < n; i++) {
		enum pin_config_param param = pinconf_to_config_param(configs[i]);
		if (param != PIN_CONFIG_BIAS_DISABLE && param != PIN_CONFIG_BIAS_PULL_UP &&
		    param != PIN_CONFIG_BIAS_PULL_DOWN && param != PIN_CONFIG_INPUT_ENABLE)
			return -ENOTSUPP;
	}
	for (i = 0; i < n; i++) {
		enum pin_config_param param = pinconf_to_config_param(configs[i]);
		if (param == PIN_CONFIG_INPUT_ENABLE) {
			pin_bit(p, 0, pin, false);
			continue;
		}
		pin_bit(p, 0x100, pin, false);
		if (param != PIN_CONFIG_BIAS_DISABLE) {
			pin_bit(p, 0x200, pin, param == PIN_CONFIG_BIAS_PULL_UP);
			pin_bit(p, 0x100, pin, true);
		}
	}
	return 0;
}
static int pin_config_group(struct pinctrl_dev *pc, unsigned g, unsigned long *c, unsigned n)
{
	unsigned i;
	int ret;
	for (i = 0; i < group_counts[g]; i++) {
		ret = pin_config_set(pc, group_pins[g][i], c, n);
		if (ret)
			return ret;
	}
	return 0;
}
static int pin_config_get(struct pinctrl_dev *pc, unsigned pin, unsigned long *c)
{
	return -ENOTSUPP;
}
static const struct pinctrl_ops pin_ops = {.get_groups_count = group_count,
					   .get_group_name = group_name,
					   .get_group_pins = group_get,
					   .dt_node_to_map = pinconf_generic_dt_node_to_map_all,
					   .dt_free_map = pinconf_generic_dt_free_map};
static bool audio_output(unsigned n) { return n == 8 || n == 15 || n == 18 || n == 20; }
static int mux_gpio_request(struct pinctrl_dev *pc, struct pinctrl_gpio_range *range, unsigned n)
{
	struct y2_pins *p = pinctrl_dev_get_drvdata(pc);
	if (!audio_output(n) && n != 6 && n != 7 && n != 9 && n != 10 && n != 54)
		return -ENOTSUPP;
	/* Clear output latch before selecting GPIO: never pulse amp/power high. */
	if (audio_output(n))
		pin_bit(p, 0x400, n, false);
	pin_bit(p, 0, n, false);
	pin_mode(p, n, 0);
	return 0;
}
static const struct pinmux_ops mux_ops = {.get_functions_count = func_count,
					  .get_function_name = func_name,
					  .get_function_groups = func_groups,
					  .set_mux = pin_mux,
					  .gpio_request_enable = mux_gpio_request,
					  .strict = true};
static const struct pinconf_ops conf_ops = {.is_generic = true,
					    .pin_config_get = pin_config_get,
					    .pin_config_set = pin_config_set,
					    .pin_config_group_set = pin_config_group};
static int gpio_direction(struct gpio_chip *gc, unsigned n)
{
	struct y2_pins *p = gpiochip_get_data(gc);
	return !!(readl(p->base + (n / 16) * 16) & BIT(n % 16)) ? GPIO_LINE_DIRECTION_OUT
								: GPIO_LINE_DIRECTION_IN;
}
static int gpio_input(struct gpio_chip *gc, unsigned n)
{
	struct y2_pins *p = gpiochip_get_data(gc);
	pin_bit(p, 0, n, false);
	return 0;
}
static int gpio_output(struct gpio_chip *gc, unsigned n, int value)
{
	struct y2_pins *p = gpiochip_get_data(gc);
	if (!audio_output(n))
		return -ENOTSUPP;
	pin_bit(p, 0x400, n, value);
	pin_bit(p, 0, n, true);
	return 0;
}
static int gpio_set(struct gpio_chip *gc, unsigned n, int value)
{
	if (!audio_output(n))
		return -ENOTSUPP;
	pin_bit(gpiochip_get_data(gc), 0x400, n, value);
	return 0;
}
static int gpio_get(struct gpio_chip *gc, unsigned n)
{
	struct y2_pins *p = gpiochip_get_data(gc);
	return !!(readl(p->base + 0x500 + (n / 16) * 16) & BIT(n % 16));
}
static int gpio_irq(struct gpio_chip *gc, unsigned n)
{
	struct y2_pins *p = gpiochip_get_data(gc);
	return mtk_eint_find_irq(&p->eint, n);
}
static int eint_gpio(void *data, unsigned long n, unsigned *gpio, struct gpio_chip **gc)
{
	struct y2_pins *p = data;
	if (n != 25 && n != 55)
		return -EINVAL;
	*gpio = n;
	*gc = &p->gpio;
	return 0;
}
static int eint_state(void *data, unsigned long n)
{
	struct y2_pins *p = data;
	return gpio_get(&p->gpio, n);
}
static int eint_set(void *data, unsigned long n)
{
	struct y2_pins *p = data;
	if (n != 25 && n != 55)
		return -EINVAL;
	return gpio_input(&p->gpio, n);
}
static const struct mtk_eint_xt eint_xlate = {
    .get_gpio_n = eint_gpio, .get_gpio_state = eint_state, .set_gpio_as_eint = eint_set};
static const struct mtk_eint_hw eint_hw = {
    .port_mask = 7, .ports = 6, .ap_num = 169, .db_cnt = 16, .db_time = debounce_time_mt2701};
static int pins_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct y2_pins *p;
	int ret, i;
	p = devm_kzalloc(dev, sizeof(*p), GFP_KERNEL);
	if (!p)
		return -ENOMEM;
	spin_lock_init(&p->lock);
	p->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(p->base))
		return PTR_ERR(p->base);
	p->eintbase = devm_platform_ioremap_resource(pdev, 1);
	if (IS_ERR(p->eintbase))
		return PTR_ERR(p->eintbase);
	p->eint.irq = platform_get_irq(pdev, 0);
	if (p->eint.irq < 0)
		return p->eint.irq;
	for (i = 0; i < 169; i++) {
		p->pins[i].number = i;
		p->pins[i].name = devm_kasprintf(dev, GFP_KERNEL, "GPIO%d", i);
		if (!p->pins[i].name)
			return -ENOMEM;
		p->epins[i] = (struct mtk_eint_pin){.number = i, .index = i, .debounce = i < 16};
	}
	p->desc = (struct pinctrl_desc){.name = "mt6582-m2",
					.owner = THIS_MODULE,
					.pins = p->pins,
					.npins = 169,
					.pctlops = &pin_ops,
					.pmxops = &mux_ops,
					.confops = &conf_ops};
	p->pctl = devm_pinctrl_register(dev, &p->desc, p);
	if (IS_ERR(p->pctl))
		return PTR_ERR(p->pctl);
	p->gpio = (struct gpio_chip){.label = dev_name(dev),
				     .parent = dev,
				     .owner = THIS_MODULE,
				     .base = -1,
				     .ngpio = 169,
				     .request = gpiochip_generic_request,
				     .free = gpiochip_generic_free,
				     .get = gpio_get,
				     .get_direction = gpio_direction,
				     .direction_input = gpio_input,
				     .direction_output = gpio_output,
				     .set = gpio_set,
				     .to_irq = gpio_irq};
	ret = devm_gpiochip_add_data(dev, &p->gpio, p);
	if (ret)
		return ret;
	p->eint.dev = dev;
	p->eint.base = &p->eintbase;
	p->eint.nbase = 1;
	p->eint.hw = &eint_hw;
	p->eint.pctl = p;
	p->eint.gpio_xlate = &eint_xlate;
	ret = mtk_eint_do_init(&p->eint, p->epins);
	if (ret)
		return dev_err_probe(dev, ret, "EINT init\n");
	dev_info(dev, "minimal pinctrl/GPIO; EINT parent high, PMIC25 high/wheel55 falling\n");
	return 0;
}
static const struct of_device_id pins_match[] = {{.compatible = "innioasis,y2-pinctrl"}, {}};
static struct platform_driver pins_driver = {
    .probe = pins_probe,
    .driver = {.name = "y2-pinctrl", .of_match_table = pins_match, .suppress_bind_attrs = true}};
static int __init pins_init(void) { return platform_driver_register(&pins_driver); }
subsys_initcall(pins_init);
MODULE_LICENSE("GPL");
