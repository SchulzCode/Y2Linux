// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 G3D gate, stock CG_MFG: CON +0 / SET +4 / CLR +8, bit 0.
 * This is the only owner of MFGCFG. SPM owns its power domain separately. */
#include <linux/clk-provider.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include "spm.h"

struct y2_mfg_clock { struct clk_hw hw; void __iomem *base; };
static int y2_g3d_enable(struct clk_hw *hw)
{
	struct y2_mfg_clock *c = container_of(hw, struct y2_mfg_clock, hw);
	if (y2_spm_mfg_status() != 1) return -EIO;
	writel(BIT(0), c->base + 8);
	return readl(c->base) & BIT(0) ? -EIO : 0;
}
static void y2_g3d_disable(struct clk_hw *hw)
{
	struct y2_mfg_clock *c = container_of(hw, struct y2_mfg_clock, hw);
	if (WARN_ON_ONCE(y2_spm_mfg_status() != 1)) return;
	writel(BIT(0), c->base + 4);
	readl(c->base);
}
static int y2_g3d_enabled(struct clk_hw *hw)
{
	struct y2_mfg_clock *c = container_of(hw, struct y2_mfg_clock, hw);
	/* Never read the gated domain at registration, debugfs or late init. */
	if (y2_spm_mfg_status() != 1) return 0;
	return !(readl(c->base) & BIT(0));
}
static const struct clk_ops y2_g3d_ops = {
	.enable = y2_g3d_enable, .disable = y2_g3d_disable,
	.is_enabled = y2_g3d_enabled,
};
static int y2_mfg_clock_probe(struct platform_device *pdev)
{
	const struct clk_parent_data parent = { .index = 0 };
	const struct clk_init_data init = { .name = "y2-mfg-g3d",
		.ops = &y2_g3d_ops, .parent_data = &parent, .num_parents = 1,
		/* The gate is operated only while Lima holds genpd powered. */
		.flags = CLK_IGNORE_UNUSED };
	struct y2_mfg_clock *c;
	int ret = y2_spm_mfg_status();
	if (ret < 0) return ret;
	c = devm_kzalloc(&pdev->dev, sizeof(*c), GFP_KERNEL);
	if (!c) return -ENOMEM;
	c->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(c->base)) return PTR_ERR(c->base);
	c->hw.init = &init;
	ret = devm_clk_hw_register(&pdev->dev, &c->hw);
	if (ret) return ret;
	return devm_of_clk_add_hw_provider(&pdev->dev, of_clk_hw_simple_get, &c->hw);
}
static const struct of_device_id y2_mfg_clock_match[] = {
	{ .compatible = "mediatek,mt6582-mfgcfg" }, {}
};
static struct platform_driver y2_mfg_clock_driver = {
	.probe = y2_mfg_clock_probe,
	.driver = { .name = "y2-mfg-clock", .of_match_table = y2_mfg_clock_match,
		.suppress_bind_attrs = true },
};
module_platform_driver(y2_mfg_clock_driver);
MODULE_LICENSE("GPL");
