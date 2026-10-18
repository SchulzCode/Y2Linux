// SPDX-License-Identifier: GPL-2.0
/* Adapted from Chris Hendrickson, artificery-dev/linux 53fb57bb99c9.
 * GC9503V LK command table and 480x360 sync-pulse mode. The historical
 * 368-line claim was retracted in the September 2026 recovery notes.
 * Panel power remains inherited; reset is owned by MMSYS, routed on GPIO112.
 * Screen-off can use the stock power-down sequence (POWER-FINAL-27, stock_suspend).
 */
#include <linux/delay.h>
#include <linux/errno.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/regulator/consumer.h>
#include <linux/reset.h>
#include <linux/string.h>

#include <drm/drm_mipi_dsi.h>
#include <drm/drm_modes.h>
#include <drm/drm_panel.h>
#include <video/mipi_display.h>

#if IS_ENABLED(CONFIG_Y2_POWER)
#include "boot.h"
#include "boot-policy.h"
#endif

struct gc9503v {
	struct drm_panel panel;
	struct mipi_dsi_device *dsi;
	struct reset_control *reset;
	bool inherited;
	int kind; /* Y2LCM_*: which panel this player carries, once identified */
};

/* The Y2 ships with one of two panels; the stock kernel has a driver for each. */
enum { Y2LCM_UNKNOWN, Y2LCM_GC9503V, Y2LCM_ST7701S };

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

/*
 * POWER-FINAL-27: the Y2 FM kernel's GC9503V lcm_suspend (0xc04a24f4, table
 * 0xc09ff464, ledger F22): display off and sleep-in framed by the vendor's
 * analog power-down block, then reset left HIGH (SET_RESET_PIN(1)) for 50 ms.
 * stock_suspend=N (kernel default) keeps 26's display-off, sleep-in, reset-low.
 * prepare() pulses reset and replays the full init either way.
 */
struct gc9503v_suspend_cmd {
	u8 cmd;
	u8 len;
	u8 delay_ms; /* after this command */
	u8 data[5];
};
static const struct gc9503v_suspend_cmd gc9503v_stock_suspend_cmds[] = {
    {0xf0, 5, 0, {0x55, 0xaa, 0x52, 0x08, 0x00}},
    {0xc1, 1, 0, {0x3f}},
    {0x6c, 1, 20, {0x60}},
    {0xb1, 1, 0, {0x00}},
    {0xfa, 4, 20, {0x7f, 0x00, 0x00, 0x00}},
    {0x6c, 1, 10, {0x50}},
    {MIPI_DCS_SET_DISPLAY_OFF, 0, 50, {}},
    {MIPI_DCS_ENTER_SLEEP_MODE, 0, 20, {}},
    {0xf0, 5, 0, {0x55, 0xaa, 0x52, 0x08, 0x00}},
    {0xc2, 1, 0, {0xce}},
    {0xc3, 1, 0, {0xcd}},
    {0xc6, 1, 0, {0xfc}},
    {0xc5, 1, 0, {0x03}},
    {0xcd, 1, 0, {0x64}},
    {0xc4, 1, 0, {0xff}},
    {0xc9, 1, 0, {0xcd}},
    {0xf6, 2, 0, {0x5a, 0x87}},
    {0xfd, 3, 0, {0xaa, 0xaa, 0x0a}},
    {0xfe, 2, 0, {0x6a, 0x0a}},
    {0x78, 2, 0, {0x2a, 0xaa}},
    {0x92, 2, 0, {0x17, 0x08}},
    {0x77, 2, 0, {0xaa, 0x2a}},
    {0x76, 2, 0, {0xaa, 0xaa}},
    {0x84, 1, 0, {0x00}},
    {0x78, 2, 0, {0x2b, 0xba}},
    {0x89, 1, 0, {0x73}},
    {0x88, 1, 0, {0x3a}},
    {0x85, 1, 0, {0xb0}},
    {0x76, 2, 0, {0xeb, 0xaa}},
    {0x94, 1, 0, {0x80}},
    {0x87, 3, 0, {0x04, 0x07, 0x30}},
    {0x93, 1, 0, {0x27}},
    {0xaf, 1, 0, {0x02}},
};
/*
 * Sitronix ST7701S (stock st7701_hvga_dsi_vdo_boe), the panel of the owner's
 * player. Stock FM kernel: lcm_init 0xc04a2754 (= lcm_resume) pulses reset
 * 1/20 ms/0/50 ms/1/150 ms and pushes the 47 rows at 0xc09fffb8 (0xffe = delay,
 * 0xffd = end); lcm_suspend 0xc04a27d8 pushes the 7 rows at 0xc0a00cf0, waits
 * 50 ms and leaves reset high. Delay rows are folded into delay_ms below.
 */
struct st7701s_cmd {
	u8 cmd;
	u8 len;
	u8 delay_ms; /* after this command */
	u8 data[16];
};
static const struct st7701s_cmd st7701s_init_cmds[] = {
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x13}},
    {0xef, 1, 0, {0x08}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x10}},
    {0xc0, 2, 0, {0x2c, 0x00}},
    {0xc1, 2, 0, {0x10, 0x0c}},
    {0xc2, 2, 0, {0x21, 0x0a}},
    {0xcc, 1, 0, {0x10}},
    {0xb0, 16, 0, {0x00, 0x0b, 0x12, 0x0d, 0x10, 0x06, 0x02, 0x08, 0x07, 0x1f, 0x04, 0x11, 0x0f, 0x29, 0x31, 0x1e}},
    {0xb1, 16, 0, {0x00, 0x0b, 0x13, 0x0d, 0x11, 0x06, 0x03, 0x08, 0x07, 0x20, 0x04, 0x12, 0x11, 0x29, 0x31, 0x1e}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x11}},
    {0xb0, 1, 0, {0x5d}},
    {0xb1, 1, 0, {0x72}},
    {0xb2, 1, 0, {0x84}},
    {0xb3, 1, 0, {0x80}},
    {0xb5, 1, 0, {0x4d}},
    {0xb7, 1, 0, {0x85}},
    {0xb8, 1, 0, {0x20}},
    {0xb9, 1, 0, {0x10}},
    {0xc1, 1, 0, {0x78}},
    {0xc2, 1, 0, {0x78}},
    {0xd0, 1, 100, {0x88}},
    {0xe0, 3, 0, {0x80, 0x00, 0x02}},
    {0xe1, 11, 0, {0x05, 0x00, 0x07, 0x00, 0x06, 0x00, 0x08, 0x00, 0x00, 0x33, 0x33}},
    {0xe2, 12, 0, {0x00, 0x00, 0x30, 0x30, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00}},
    {0xe3, 4, 0, {0x00, 0x00, 0x11, 0x11}},
    {0xe4, 2, 0, {0x44, 0x44}},
    {0xe5, 16, 0, {0x0c, 0x78, 0x00, 0xe0, 0x0e, 0x7a, 0x00, 0xe0, 0x08, 0x74, 0x00, 0xe0, 0x0a, 0x76, 0x00, 0xe0}},
    {0xe6, 4, 0, {0x00, 0x00, 0x11, 0x11}},
    {0xe7, 2, 0, {0x44, 0x44}},
    {0xe8, 16, 0, {0x0d, 0x79, 0x00, 0xe0, 0x0f, 0x7b, 0x00, 0xe0, 0x09, 0x75, 0x00, 0xe0, 0x0b, 0x77, 0x00, 0xe0}},
    {0xe9, 2, 0, {0x36, 0x00}},
    {0xeb, 7, 0, {0x00, 0x01, 0xe4, 0xe4, 0x44, 0x88, 0x40}},
    {0xed, 16, 0, {0xa1, 0xc2, 0xfb, 0x0f, 0x67, 0x45, 0xff, 0xff, 0xff, 0xff, 0x54, 0x76, 0xf0, 0xbf, 0x2c, 0x1a}},
    {0xef, 6, 0, {0x10, 0x0d, 0x04, 0x08, 0x3f, 0x1f}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x13}},
    {0xe8, 2, 0, {0x00, 0x0e}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x00}},
    {MIPI_DCS_EXIT_SLEEP_MODE, 0, 120, {}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x13}},
    {0xe8, 2, 10, {0x00, 0x0c}},
    {0xe8, 2, 0, {0x00, 0x00}},
    {0xff, 5, 0, {0x77, 0x01, 0x00, 0x00, 0x00}},
    {MIPI_DCS_SET_DISPLAY_ON, 0, 0, {}},
};
static const struct st7701s_cmd st7701s_suspend_cmds[] = {
    {MIPI_DCS_SET_DISPLAY_OFF, 0, 10, {}},
    {MIPI_DCS_ENTER_SLEEP_MODE, 0, 120, {}},
    {0xff, 6, 0, {0x77, 0x01, 0x00, 0x00, 0x00, 0x80}},
    {0xff, 5, 50, {0x77, 0x01, 0x00, 0x00, 0x80}},
};

static bool stock_suspend;
module_param(stock_suspend, bool, 0644);
MODULE_PARM_DESC(stock_suspend, "screen-off uses the stock GC9503V power-down sequence (reset high)");
/* Y2Linux #45: until the reset controller could report its line, prepare()
 * failed for LK's lit panel, DRM skipped every unprepare and the panel slept
 * powered and awake. Off (default) keeps that behaviour; on, screen-off powers
 * the panel down and screen-on replays the full stock init. Switching it off
 * again leaves the panel lit at the next screen-off. */
static bool power_down;
module_param(power_down, bool, 0644);
/* The platform's record of this player's panel (1 GC9503V, 2 ST7701S), used
 * only when LK did not identify it this boot: lcm=0-st7701 can be LK's
 * fallback to its last driver (fm-lk 0x81e0af60, see boot-policy.h). */
static int panel_hint;
module_param(panel_hint, int, 0644);
/* What the driver identified (0 not yet, 1 GC9503V, 2 ST7701S), for the platform. */
static int panel_kind;
module_param(panel_kind, int, 0444);
MODULE_PARM_DESC(panel_kind, "identified panel: 1 GC9503V, 2 ST7701S (0 not identified)");
MODULE_PARM_DESC(panel_hint, "panel recorded on an identified boot: 1 GC9503V, 2 ST7701S (0 none)");
MODULE_PARM_DESC(power_down, "screen-off powers the panel down; off keeps the lit LK panel (as before #45)");

static inline struct gc9503v *to_gc9503v(struct drm_panel *panel)
{
	return container_of(panel, struct gc9503v, panel);
}

/* The packet types of the stock DSI_set_cmdq_V2 (FM kernel 0xc05227f8):
 * commands above 0xAF go out as generic writes (0x13/0x23/0x29), the others
 * as DCS (0x05/0x15/0x39). Sent as DCS, the GC9503V ignored its unlock,
 * power, GIP and gamma rows: white stripes after the full init (6451). */
static int gc9503v_write(struct mipi_dsi_device *dsi, u8 cmd, const u8 *data, u8 len)
{
	u8 buf[1 + sizeof_field(struct gc9503v_cmd, data)];
	ssize_t ret;

	if (cmd <= 0xaf) {
		ret = mipi_dsi_dcs_write(dsi, cmd, data, len);
	} else {
		if (len >= sizeof(buf))
			return -EINVAL;
		buf[0] = cmd;
		memcpy(buf + 1, data, len);
		ret = mipi_dsi_generic_write(dsi, buf, len + 1);
	}
	return ret < 0 ? ret : 0;
}

/* Which of the two stock panels this player carries. LK ran the stock
 * lcm_compare_id and names the one it found (boot-policy.h): the owner's player
 * reports st7701, #45's reporter's gc9503v (also with lcm=0-). Without LK's word,
 * the GC9503V's own check (0xc04a2240: DCS 0x04 = 0x9503/0x9504) is the
 * fallback. Nothing is ever sent to an unidentified panel. */
static int gc9503v_identify(struct gc9503v *ctx)
{
	u8 id[3] = {};
	int ret;

	if (ctx->kind)
		return 0;
#if IS_ENABLED(CONFIG_Y2_POWER)
	switch (y2_boot_panel()) {
	case Y2_PANEL_GC9503V:
		ctx->kind = Y2LCM_GC9503V;
		WRITE_ONCE(panel_kind, ctx->kind);
		dev_info(&ctx->dsi->dev, "LK identified the panel as GC9503V\n");
		return 0;
	case Y2_PANEL_ST7701S:
		ctx->kind = Y2LCM_ST7701S;
		WRITE_ONCE(panel_kind, ctx->kind);
		dev_info(&ctx->dsi->dev, "LK identified the panel as ST7701S\n");
		return 0;
	case Y2_PANEL_OTHER:
		dev_warn_once(&ctx->dsi->dev, "LK found an unknown panel: it stays lit\n");
		return -ENODEV;
	}
#endif
	/* The stock compare_id reads: GC9503V 0x04 = xx 95 03/04 (0xc04a2240);
	 * ST7701S 0xA1 first byte 0x88 or 0x99 (0xc04a2534, max return 1). */
	ret = mipi_dsi_set_maximum_return_packet_size(ctx->dsi, sizeof(id));
	if (!ret)
		ret = mipi_dsi_dcs_read(ctx->dsi, MIPI_DCS_GET_DISPLAY_ID, id, sizeof(id));
	if (ret == sizeof(id) && ((id[1] << 8 | id[2]) == 0x9503 || (id[1] << 8 | id[2]) == 0x9504)) {
		ctx->kind = Y2LCM_GC9503V;
		WRITE_ONCE(panel_kind, ctx->kind);
		dev_info(&ctx->dsi->dev, "display ID %02x %02x %02x: GC9503V\n", id[0], id[1], id[2]);
		return 0;
	}
	{
		u8 st[1] = {};
		int st_ret = mipi_dsi_set_maximum_return_packet_size(ctx->dsi, sizeof(st));

		if (!st_ret)
			st_ret = mipi_dsi_dcs_read(ctx->dsi, 0xa1, st, sizeof(st));
		if (st_ret == sizeof(st) && (st[0] == 0x88 || st[0] == 0x99)) {
			ctx->kind = Y2LCM_ST7701S;
			WRITE_ONCE(panel_kind, ctx->kind);
			dev_info(&ctx->dsi->dev, "register 0xa1 = %02x: ST7701S\n", st[0]);
			return 0;
		}
		dev_warn_once(&ctx->dsi->dev, "ST7701S ID read 0xa1: %d, %02x\n", st_ret, st[0]);
	}
	switch (READ_ONCE(panel_hint)) {
	case Y2LCM_GC9503V:
	case Y2LCM_ST7701S:
		ctx->kind = READ_ONCE(panel_hint);
		WRITE_ONCE(panel_kind, ctx->kind);
		dev_info(&ctx->dsi->dev, "panel %s from the platform's record (LK did not identify it this boot)\n",
			 ctx->kind == Y2LCM_ST7701S ? "ST7701S" : "GC9503V");
		return 0;
	}
	dev_warn_once(&ctx->dsi->dev, "panel not identified (display ID %02x %02x %02x, read %d): it stays lit\n",
		 id[0], id[1], id[2], ret);
	return -ENODEV;
}

static int st7701s_send(struct gc9503v *ctx, const struct st7701s_cmd *table, unsigned int rows,
			const char *what)
{
	unsigned int i;
	int ret;

	for (i = 0; i < rows; i++) {
		ret = gc9503v_write(ctx->dsi, table[i].cmd, table[i].data, table[i].len);
		if (ret < 0) {
			dev_err(&ctx->dsi->dev, "ST7701S %s cmd 0x%02x (row %u) failed: %d\n", what,
				table[i].cmd, i, ret);
			return ret;
		}
		if (table[i].delay_ms)
			msleep(table[i].delay_ms);
	}
	return 0;
}

/* Stock lcm_init / lcm_resume of the ST7701S. */
static int st7701s_init(struct gc9503v *ctx)
{
	int ret;

	dev_info(&ctx->dsi->dev, "ST7701S full initialization (stock lcm_init)\n");
	ret = reset_control_deassert(ctx->reset);
	if (!ret) {
		msleep(20);
		ret = reset_control_assert(ctx->reset);
	}
	if (!ret) {
		msleep(50);
		ret = reset_control_deassert(ctx->reset);
	}
	if (ret)
		return ret;
	msleep(150);
	ret = st7701s_send(ctx, st7701s_init_cmds, ARRAY_SIZE(st7701s_init_cmds), "init");
	if (ret) {
		reset_control_assert(ctx->reset);
		return ret;
	}
	dev_info(&ctx->dsi->dev, "ST7701S sleep-out and display-on complete\n");
	return 0;
}

/* Stock lcm_suspend of the ST7701S; reset stays high. A failed sequence falls
 * back to reset low, which also leaves the panel dark and quiescent. */
static int st7701s_unprepare(struct gc9503v *ctx)
{
	int ret = st7701s_send(ctx, st7701s_suspend_cmds, ARRAY_SIZE(st7701s_suspend_cmds), "power-down");

	if (!ret)
		return 0;
	ret = reset_control_assert(ctx->reset);
	dev_err(&ctx->dsi->dev, "ST7701S power-down fell back to reset low: %d\n", ret);
	return ret;
}

static int gc9503v_prepare(struct drm_panel *panel)
{
	struct gc9503v *ctx = to_gc9503v(panel);
	struct mipi_dsi_device *dsi = ctx->dsi;
	struct device *dev = &dsi->dev;
	unsigned int i;
	int ret;

	if (ctx->inherited) {
        ret = reset_control_status(ctx->reset);
        /* Probe required innioasis,lk-powered: a reset controller that cannot
         * report its line means LK's lit panel. */
        if (ret == -ENOTSUPP)
            ret = 0;
        if (ret < 0) {
            dev_err(dev, "panel reset status unreadable: %d\n", ret);
            return ret;
        }
        /* Identify the LK-lit panel at the first screen-on (read-only DCS reads
         * while the DSI is up), so the platform knows it before arming. */
        if (!ret && !ctx->kind)
            gc9503v_identify(ctx);
        /* Not adopted: DRM keeps it unprepared and never powers it down. */
        if (!ret && !READ_ONCE(power_down))
            return -EOPNOTSUPP;
        /* Never send one panel's commands to the other. */
        if (gc9503v_identify(ctx))
            return -EOPNOTSUPP;
        ctx->inherited = false;
        if (!ret) {
            dev_info(dev, "retaining live LK panel, reset deasserted; no DCS replay\n");
            return 0;
        }
    }
    if (ctx->kind == Y2LCM_ST7701S)
        return st7701s_init(ctx);
    dev_info(dev, "panel reset asserted/previously unprepared: full DCS initialization\n");
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

		ret = gc9503v_write(dsi, c->cmd, c->data, c->len);
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

	dev_info(dev, "panel DCS sleep-out and display-on complete\n");
	return 0;

disable_supply:
	reset_control_assert(ctx->reset);
	return ret;
}

static int gc9503v_stock_unprepare(struct gc9503v *ctx)
{
	unsigned int i;
	int ret;

	for (i = 0; i < ARRAY_SIZE(gc9503v_stock_suspend_cmds); i++) {
		const struct gc9503v_suspend_cmd *c = &gc9503v_stock_suspend_cmds[i];

		ret = gc9503v_write(ctx->dsi, c->cmd, c->data, c->len);
		if (ret < 0) {
			dev_err(&ctx->dsi->dev, "stock power-down cmd 0x%02x failed: %d\n", c->cmd, ret);
			return ret;
		}
		if (c->delay_ms)
			msleep(c->delay_ms);
	}
	ret = reset_control_deassert(ctx->reset);
	if (ret)
		return ret;
	msleep(50);
	return 0;
}

static int gc9503v_unprepare(struct drm_panel *panel)
{
	struct gc9503v *ctx = to_gc9503v(panel);
	int ret, sleep_ret, reset_ret;

	/* Switched off after adoption: leave the panel lit and retain it again. */
	if (!READ_ONCE(power_down)) {
		ctx->inherited = true;
		return 0;
	}
	if (ctx->kind == Y2LCM_ST7701S)
		return st7701s_unprepare(ctx);

	/* A failed stock sequence falls back to 26's reset-low quiescent state. */
	if (READ_ONCE(stock_suspend) && !gc9503v_stock_unprepare(ctx))
		return 0;

	ret = mipi_dsi_dcs_set_display_off(ctx->dsi);
	sleep_ret = mipi_dsi_dcs_enter_sleep_mode(ctx->dsi);
	msleep(120);

	reset_ret = reset_control_assert(ctx->reset);
	if (ret < 0 || sleep_ret < 0 || reset_ret)
		dev_err(&ctx->dsi->dev, "panel power-down: display=%d sleep=%d reset=%d\n", ret, sleep_ret, reset_ret);
	/* A successful reset assert is the fallback quiescent state even when
	 * DCS failed. Tell DRM the panel is unprepared so resume replays its full
	 * initialization instead of skipping it with the panel held in reset. */
	return reset_ret;
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

	ctx->inherited = true;
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
	/* prepare() sends DCS, so the DSI host must be pre-enabled first. Linux
	 * 6.18.y reverted the DSI host's own pre_enable_prev_first request, so
	 * the panel asks for that order itself (upstream panel-driver idiom). */
	ctx->panel.prepare_prev_first = true;

	/* POWER-FINAL-20: no drm_panel_of_backlight(). Reborn and the platform own
	 * the backlight explicitly (bl_power). Linked, every pipeline enable called
	 * backlight_enable(): at shutdown, when Reborn exits and DRM restores the
	 * stale fbdev buffer, the panel lit up white or with colours after the
	 * shutdown animation. */
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
