// SPDX-License-Identifier: GPL-2.0-only
/* D12/D13/D14: temporary guarded text output, no display control writes. */
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/io.h>
#include <linux/mutex.h>
#include <linux/sched.h>
#include <linux/uaccess.h>
#include "/project/kernel/diagnostic/text.h"

static void __iomem *y2_ovl, *y2_dsi, *y2_wdt, *y2_pixels;
static struct cdev y2_cdev;
static DEFINE_MUTEX(y2_frame_lock);
static struct y2_text_frame y2_frame;

static int y2_display_valid(void)
{
    struct y2_snapshot s = {
        .wdt_mode = readl(y2_wdt), .ovl_en = readl(y2_ovl + 0x0c),
        .src_con = readl(y2_ovl + 0x2c), .roi = readl(y2_ovl + 0x20),
        .con = readl(y2_ovl + 0x70), .size = readl(y2_ovl + 0x78),
        .offset = readl(y2_ovl + 0x7c), .addr = readl(y2_ovl + 0x80),
        .pitch = readl(y2_ovl + 0x84), .dsi_start = readl(y2_dsi),
        .dsi_ctrl = readl(y2_dsi + 0x10), .dsi_mode = readl(y2_dsi + 0x14),
    };
    return y2_valid(&s);
}
static int y2_paint_text(void)
{
    unsigned x,y;
    if (!y2_display_valid()) return -ENODEV;
    for(y=0;y<Y2_HEIGHT;++y) for(x=0;x<Y2_WIDTH;++x)
        writew(y2_text_pixel(&y2_frame,x,y),y2_pixels+y*Y2_STRIDE+x*2);
    wmb();
    /* No printk: successful visible progress must not wait for UART. */
    return 0;
}
static ssize_t y2_text_write(struct file *file, const char __user *buf,
                             size_t count, loff_t *pos)
{
    int ret;
    if(task_pid_nr(current)!=1) return -EPERM;
    if(count!=sizeof(y2_frame)) return -EINVAL;
    if(mutex_lock_interruptible(&y2_frame_lock)) return -ERESTARTSYS;
    if(copy_from_user(&y2_frame,buf,count)) ret=-EFAULT;
    else if(!y2_text_valid(&y2_frame)) ret=-EINVAL;
    else ret=y2_paint_text();
    mutex_unlock(&y2_frame_lock);
    return ret ? ret : count;
}
static const struct file_operations y2_fops = { .write = y2_text_write };
static int __init y2_diagnostic_init(void)
{
    int ret;
    dev_t dev=MKDEV(Y2_TEXT_MAJOR,0);
    if(!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
    y2_wdt=ioremap(0x10007000,4);
    y2_ovl=ioremap(0x14007000,0x88);
    y2_dsi=ioremap(0x1400c000,0x18);
    if(!y2_wdt || !y2_ovl || !y2_dsi || !y2_display_valid()) goto fail;
    y2_pixels=ioremap_wc(Y2_FB_BASE,Y2_FB_BYTES);
    if(!y2_pixels) goto fail;
    ret=register_chrdev_region(dev,1,"y2diag");
    if(ret) goto fail;
    cdev_init(&y2_cdev,&y2_fops);
    ret=cdev_add(&y2_cdev,dev,1);
    if(ret) { unregister_chrdev_region(dev,1); goto fail; }
    memset(y2_frame.rows,' ',sizeof(y2_frame.rows));
    y2_frame.magic=Y2_TEXT_MAGIC;
    memcpy(y2_frame.rows[0],"KERNEL READY; WAITING FOR PID1",28);
    memcpy(y2_frame.rows[1],"IF STATIC: RECORD THIS LAST STAGE",32);
    y2_paint_text();
    return 0;
fail:
    if(y2_pixels) iounmap(y2_pixels);
    if(y2_dsi) iounmap(y2_dsi);
    if(y2_ovl) iounmap(y2_ovl);
    if(y2_wdt) iounmap(y2_wdt);
    pr_err("Y2DIAG text unavailable; no display reinitialization\n");
    return -ENODEV;
}
device_initcall(y2_diagnostic_init);
