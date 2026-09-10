// SPDX-License-Identifier: GPL-2.0
/* Adapted from Chris Hendrickson, artificery-dev/linux 53fb57bb99c9.
 * GC9503V LK command table and 480x360 sync-pulse mode. The historical
 * 368-line claim was retracted in the September 2026 recovery notes.
 * Panel power remains inherited; reset is owned by MMSYS, routed on GPIO112.
 */
#include <linux/delay.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/regulator/consumer.h>
#include <linux/reset.h>

#include <drm/drm_mipi_dsi.h>
#include <drm/drm_modes.h>
#include <drm/drm_panel.h>

struct gc9503v {
	struct drm_panel panel;
	struct mipi_dsi_device *dsi;
	struct reset_control *reset;
};

/* One MIPI DCS long/short write: command byte + parameter bytes. */
struct gc9503v_cmd {
	u8 cmd;
	u8 len;
	u8 data[52]; /* largest entry is the 52-byte gamma table (D1..D6) */
};

/*
 * GC9503V init table, lifted verbatim from the vendor LK push_table (40 rows).
 * D1..D6 are six identical 52-byte gamma tables (R/G/B positive+negative).
 */
#define GAMMA                                                                                      \
	0x00, 0x00, 0x00, 0x0c, 0x00, 0x22, 0x00, 0x32, 0x00, 0x46, 0x00, 0x66, 0x00, 0x84, 0x00,  \
	    0xad, 0x00, 0xd5, 0x01, 0x12, 0x01, 0x4a, 0x01, 0xa4, 0x01, 0xee, 0x01, 0xf0, 0x02,    \
	    0x36, 0x02, 0x88, 0x02, 0xbe, 0x03, 0x01, 0x03, 0x38, 0x03, 0x62, 0x03, 0x81, 0x03,    \
	    0xa1, 0x03, 0xcd, 0x03, 0xd8, 0x03, 0xe0, 0x03, 0xff

static const struct gc9503v_cmd gc9503v_init_cmds[] = {
    {0xf0, 5, {0x55, 0xaa, 0x52, 0x08, 0x00}}, /* unlock inside regs */
    {0xf6, 2, {0x5a, 0x87}},
    {0xc1, 1, {0x3f}},
    {0xcd, 1, {0x25}},
    {0xc9, 1, {0x12}},
    {0xa9, 1, {0xad}},
    {0xf8, 1, {0x8a}},
    {0xac, 1, {0x45}},
    {0xa7, 1, {0x47}},
    {0xa0, 1, {0xbb}},
    {0x86, 4, {0x99, 0xa3, 0xa3, 0x31}},
    {0xfa, 4, {0x08, 0x08, 0x00, 0x04}},
    {0xa3, 1, {0x6e}},
    {0xfd, 3, {0x28, 0x3c, 0x00}},
    {0x9a, 1, {0x99}},
    {0x9b, 1, {0x70}},
    {0x82, 2, {0x5c, 0x5c}},
    {0xb1, 1, {0x10}},
    {0x7a, 2, {0x0f, 0x13}},
    {0x7b, 2, {0x0f, 0x13}},
    {0x69, 7, {0x14, 0x22, 0x14, 0x22, 0x44, 0x22, 0x08}},
    {0x6b, 1, {0x07}},
    {0x6d, 32, {0x1d, 0x07, 0x10, 0x03, 0x0e, 0x1f, 0x01, 0x1e, 0x09, 0x0a, 0x0b,
		0x0c, 0x1e, 0x1e, 0x1e, 0x1e, 0x1e, 0x1e, 0x1e, 0x1e, 0x14, 0x13,
		0x12, 0x11, 0x1e, 0x02, 0x1f, 0x0e, 0x03, 0x10, 0x08, 0x1d}},
    {0x60, 8, {0x38, 0x0d, 0x62, 0x62, 0x38, 0x0c, 0x62, 0x62}},
    {0x61, 8, {0x38, 0x0e, 0x62, 0x62, 0x38, 0x0e, 0x62, 0x62}},
    {0x63, 8, {0x38, 0x0b, 0x62, 0x62, 0x38, 0x0a, 0x62, 0x62}},
    {0x64,
     16,
     {0x38, 0x09, 0x01, 0x67, 0x03, 0x03, 0x38, 0x07, 0x01, 0x69, 0x03, 0x03, 0x62, 0x62, 0x62,
      0x62}},
    {0x65,
     16,
     {0x38, 0x05, 0x01, 0x6b, 0x00, 0x03, 0x38, 0x03, 0x01, 0x6d, 0x03, 0x03, 0x62, 0x62, 0x62,
      0x62}},
    {0x66,
     16,
     {0xc1, 0x7c, 0x08, 0x13, 0x00, 0x03, 0xc1, 0x7c, 0x08, 0x13, 0x03, 0x03, 0x72, 0x72, 0x72,
      0x72}},
    {0x67,
     16,
     {0xc8, 0x13, 0x01, 0x7c, 0x00, 0x03, 0xc8, 0x13, 0x01, 0x7c, 0x03, 0x03, 0x72, 0x72, 0x72,
      0x72}},
    {0xd1, 52, {GAMMA}},
    {0xd2, 52, {GAMMA}},
    {0xd3, 52, {GAMMA}},
    {0xd4, 52, {GAMMA}},
    {0xd5, 52, {GAMMA}},
    {0xd6, 52, {GAMMA}},
};

static inline struct gc9503v *to_gc9503v(struct drm_panel *panel)
{
	return container_of(panel, struct gc9503v, panel);
}

static int gc9503v_prepare(struct drm_panel *panel)
{
	struct gc9503v *ctx = to_gc9503v(panel);
	struct mipi_dsi_device *dsi = ctx->dsi;
	struct device *dev = &dsi->dev;
	unsigned int i;
	int ret;

	/* Reset high 10ms, low 10ms, high 120ms: retained LK/recovery sequence. */
	ret = reset_control_deassert(ctx->reset);
	if (ret)
		return ret;
	msleep(10);
	ret = reset_control_assert(ctx->reset);
	if (ret)
		return ret;
	msleep(10);
	ret = reset_control_deassert(ctx->reset);
	if (ret)
		return ret;
	msleep(120);

	for (i = 0; i < ARRAY_SIZE(gc9503v_init_cmds); i++) {
		const struct gc9503v_cmd *c = &gc9503v_init_cmds[i];

		ret = mipi_dsi_dcs_write(dsi, c->cmd, c->data, c->len);
		if (ret < 0) {
			dev_err(dev, "init cmd 0x%02x failed: %d\n", c->cmd, ret);
			goto disable_supply;
		}
	}

	ret = mipi_dsi_dcs_exit_sleep_mode(dsi);
	if (ret < 0)
		goto disable_supply;
	msleep(120);

	ret = mipi_dsi_dcs_set_display_on(dsi);
	if (ret < 0)
		goto disable_supply;
	msleep(20);

	return 0;

disable_supply:
	reset_control_assert(ctx->reset);
	return ret;
}

static int gc9503v_unprepare(struct drm_panel *panel)
{
	struct gc9503v *ctx = to_gc9503v(panel);

	mipi_dsi_dcs_set_display_off(ctx->dsi);
	mipi_dsi_dcs_enter_sleep_mode(ctx->dsi);
	msleep(120);

	reset_control_assert(ctx->reset);
	return 0;
}

/* LK: 890 x 488 totals at 27.362MHz (~63Hz), active 480x360. */
static const struct drm_display_mode gc9503v_mode = {
    .clock = 27362,
    .hdisplay = 480,
    .hsync_start = 480 + 200,
    .hsync_end = 480 + 200 + 10,
    .htotal = 480 + 200 + 10 + 200,
    .vdisplay = 360,
    .vsync_start = 360 + 60,
    .vsync_end = 360 + 60 + 8,
    .vtotal = 360 + 60 + 8 + 60,
    .width_mm = 46,
    .height_mm = 35,
};

static int gc9503v_get_modes(struct drm_panel *panel, struct drm_connector *connector)
{
	struct drm_display_mode *mode;

	mode = drm_mode_duplicate(connector->dev, &gc9503v_mode);
	if (!mode)
		return -ENOMEM;

	drm_mode_set_name(mode);
	mode->type = DRM_MODE_TYPE_DRIVER | DRM_MODE_TYPE_PREFERRED;
	connector->display_info.width_mm = mode->width_mm;
	connector->display_info.height_mm = mode->height_mm;
	drm_mode_probed_add(connector, mode);

	return 1;
}

static const struct drm_panel_funcs gc9503v_funcs = {
    .prepare = gc9503v_prepare,
    .unprepare = gc9503v_unprepare,
    .get_modes = gc9503v_get_modes,
};

static int gc9503v_probe(struct mipi_dsi_device *dsi)
{
	struct device *dev = &dsi->dev;
	struct gc9503v *ctx;
	int ret;

	ctx = devm_kzalloc(dev, sizeof(*ctx), GFP_KERNEL);
	if (!ctx)
		return -ENOMEM;

	ctx->dsi = dsi;
	mipi_dsi_set_drvdata(dsi, ctx);

	ctx->reset = devm_reset_control_get_exclusive(dev, NULL);
	if (IS_ERR(ctx->reset))
		return dev_err_probe(dev, PTR_ERR(ctx->reset), "panel reset\n");
	if (!of_property_read_bool(dev->of_node, "innioasis,lk-powered"))
		return -EINVAL;

	dsi->lanes = 2;
	/*
	 * DSI *wire* format (SoC->panel link), not the framebuffer format. LK's
	 * LCM_PARAMS word_count = 480*3 = 1440 -> 3 bytes/pixel = 24-bit on the
	 * link, so RGB888 here. The visible framebuffer is 16bpp RGB565; the OVL
	 * plane upconverts to the DSI link format. If colours are wrong on HW, the
	 * link may instead be loosely-packed RGB666 (also 3 bytes/pixel) -> try
	 * MIPI_DSI_FMT_RGB666.
	 */
	dsi->format = MIPI_DSI_FMT_RGB888;
	/* LK drives the panel in SYNC-PULSE video mode (DSI_MODE_CTRL=1). */
	dsi->mode_flags = MIPI_DSI_MODE_VIDEO | MIPI_DSI_MODE_VIDEO_SYNC_PULSE | MIPI_DSI_MODE_LPM;

	drm_panel_init(&ctx->panel, dev, &gc9503v_funcs, DRM_MODE_CONNECTOR_DSI);

	ret = drm_panel_of_backlight(&ctx->panel);
	if (ret)
		return ret;

	drm_panel_add(&ctx->panel);

	ret = mipi_dsi_attach(dsi);
	if (ret < 0) {
		drm_panel_remove(&ctx->panel);
		return dev_err_probe(dev, ret, "failed to attach to DSI host\n");
	}

	return 0;
}

static void gc9503v_remove(struct mipi_dsi_device *dsi)
{
	struct gc9503v *ctx = mipi_dsi_get_drvdata(dsi);

	mipi_dsi_detach(dsi);
	drm_panel_remove(&ctx->panel);
}

static const struct of_device_id gc9503v_of_match[] = {{.compatible = "innioasis,y2-gc9503v"}, {}};
MODULE_DEVICE_TABLE(of, gc9503v_of_match);

static struct mipi_dsi_driver gc9503v_driver = {
    .probe = gc9503v_probe,
    .remove = gc9503v_remove,
    .driver =
	{
	    .name = "panel-gc9503v",
	    .of_match_table = gc9503v_of_match,
	},
};
module_mipi_dsi_driver(gc9503v_driver);

MODULE_DESCRIPTION("GalaxyCore GC9503V MIPI-DSI panel driver (Innioasis Y2)");
MODULE_LICENSE("GPL");
