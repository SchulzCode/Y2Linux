// SPDX-License-Identifier: GPL-2.0-only
/* Temporary built-in diagnostic, included only by the reviewed Y2 overlay.
 * D12/D13: no panel/clock/pinmux programming. All display registers READ ONLY.
 */
#include <linux/io.h>
#include <linux/proc_fs.h>
#include <linux/sched.h>
#include <linux/uaccess.h>
#include "/project/kernel/diagnostic/policy.h"

static void __iomem *y2_ovl, *y2_dsi, *y2_wdt, *y2_pixels;
static unsigned y2_phase, y2_beats;

static int y2_display_valid(void)
{
    struct y2_snapshot s = {
        .wdt_mode = readl(y2_wdt),
        .ovl_en = readl(y2_ovl + 0x0c),
        .src_con = readl(y2_ovl + 0x2c),
        .roi = readl(y2_ovl + 0x20),
        .con = readl(y2_ovl + 0x70),
        .size = readl(y2_ovl + 0x78),
        .offset = readl(y2_ovl + 0x7c),
        .addr = readl(y2_ovl + 0x80),
        .pitch = readl(y2_ovl + 0x84),
        .dsi_start = readl(y2_dsi),
        .dsi_ctrl = readl(y2_dsi + 0x10),
        .dsi_mode = readl(y2_dsi + 0x14),
    };
    if (y2_valid(&s)) return 1;
    pr_err("Y2DIAG REFUSED wdt=%x ovl=%x src=%x roi=%x con=%x size=%x offset=%x addr=%x pitch=%x dsi=%x/%x/%x\n",
        s.wdt_mode, s.ovl_en, s.src_con, s.roi, s.con, s.size, s.offset,
        s.addr, s.pitch, s.dsi_start, s.dsi_ctrl, s.dsi_mode);
    return 0;
}

static int y2_paint(unsigned stage)
{
    unsigned x, y;
    if (!y2_display_valid()) return -ENODEV;
    for (y = 0; y < Y2_HEIGHT; ++y)
        for (x = 0; x < Y2_WIDTH; ++x)
            writew(y2_pixel(stage, x, y), y2_pixels + y * Y2_STRIDE + x * 2);
    /* Complete WC pixel stores before reporting the stage. */
    wmb();
    pr_info("Y2DIAG VISUAL stage=%u beats=%u\n", stage, y2_beats);
    return 0;
}

static ssize_t y2_command(struct file *file, const char __user *buf,
                          size_t count, loff_t *pos)
{
    char command;
    int stage;
    int ret;
    if (task_pid_nr(current) != 1) return -EPERM;
    if (count != 1) return -EINVAL;
    if (copy_from_user(&command, buf, 1)) return -EFAULT;
    stage = y2_next_stage(y2_phase, y2_beats, command);
    if (stage < 0) return -EINVAL;
    ret = y2_paint(stage);
    if (ret) return ret;
    if (command == 'I') y2_phase = 1;
    if (command == 'H') ++y2_beats;
    if (command == 'F') y2_phase = 2;
    return 1;
}
static const struct proc_ops y2_proc_ops = { .proc_write = y2_command };

static int __init y2_diagnostic_init(void)
{
    if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
    y2_wdt = ioremap(0x10007000, 4);
    y2_ovl = ioremap(0x14007000, 0x88);
    y2_dsi = ioremap(0x1400c000, 0x18);
    if (!y2_wdt || !y2_ovl || !y2_dsi) goto fail;
    if (!y2_display_valid()) goto fail;
    y2_pixels = ioremap_wc(Y2_FB_BASE, Y2_FB_BYTES);
    if (!y2_pixels) goto fail;
    if (y2_paint(0)) goto fail;
    if (!proc_create("y2_diag", 0200, NULL, &y2_proc_ops)) goto fail;
    pr_info("Y2DIAG KERNEL WDT stopped; guarded LK RGB565 ready\n");
    return 0;
fail:
    if (y2_pixels) iounmap(y2_pixels);
    if (y2_dsi) iounmap(y2_dsi);
    if (y2_ovl) iounmap(y2_ovl);
    if (y2_wdt) iounmap(y2_wdt);
    pr_err("Y2DIAG observation unavailable; no display reinitialization\n");
    return -ENODEV;
}
device_initcall(y2_diagnostic_init);
