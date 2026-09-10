// SPDX-License-Identifier: GPL-2.0-only
/* M2 shared observation endpoint and proven guarded USB startup. */
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/io.h>
#include <linux/ioport.h>
#include <linux/delay.h>
#include <linux/mutex.h>
#include <linux/sched.h>
#include <linux/uaccess.h>
#include "/project/kernel/diagnostic/text.h"
#include "/project/kernel/diagnostic/usb_wake.h"
#include "/project/kernel/usb/live.h"
static void y2_usb_begin(void);
static ssize_t y2_usb_status(char __user *buf);

static struct cdev y2_cdev;
static DEFINE_MUTEX(y2_frame_lock);
/* Legacy PID1 ABI accepted without touching LK framebuffer or DRM registers. */
static ssize_t y2_text_write(struct file *file, const char __user *buf,
                            size_t count, loff_t *pos)
{
    if (task_pid_nr(current) != 1) return -EPERM;
    return count == sizeof(struct y2_text_frame) ? count : -EINVAL;
}
static struct y2_platform_snapshot y2_power;
static bool y2_power_done;
static void y2_power_delay(void *context) { udelay(10); }
extern int y2_ccf_usb_read(unsigned address, unsigned *value);
extern int y2_pmic_snapshot(struct y2_pwrap_snapshot *snapshot);
static int y2_clock_read(void *context, unsigned address, unsigned *value)
{ return y2_ccf_usb_read(address, value); }
struct y2_usb_mapping { void __iomem *mac, *phy; };
static int y2_usb_read(void *context, unsigned address, unsigned width, unsigned *value)
{
    struct y2_usb_mapping *mapping = context;
    void __iomem *reg;
    unsigned i;
    for (i = 0; i < Y2_USB_STATE_COUNT; ++i)
        if (address == y2_usb_registers[i].address &&
            width == y2_usb_registers[i].width) break;
    if (i == Y2_USB_STATE_COUNT) {
        for (i = 0; i < 7; ++i)
            if (width == 1 && address == Y2_USB_PHY_BASE+y2_usb_mode_offsets[i]) break;
        if (i == 7) return -EINVAL;
    }
    reg = address >= Y2_USB_PHY_BASE ? mapping->phy + (address - Y2_USB_PHY_BASE) :
                                     mapping->mac + (address - Y2_USB_MAC_BASE);
    *value = width == 1 ? readb(reg) : readw(reg);
    return 0;
}
static int y2_usb_release_suspend(void *context, unsigned *written)
{
    struct y2_usb_mapping *mapping = context;
    /* The only new write: the source-proven force_suspendm bit, once. */
    if (readb(mapping->phy + 0x6a) != 4) return -EBUSY;
    writeb(0, mapping->phy + 0x6a);
    *written = 1;
    return 0;
}
static void y2_usb_settle(void *context) { udelay(800); }
static void y2_collect_usb(void)
{
    struct y2_usb_mapping mapping = { 0 };
    struct y2_usb_state_io io = { .context = &mapping, .read = y2_usb_read };
    struct y2_usb_wake_io wake = { .context = &mapping, .read = y2_usb_read,
        .release_suspend = y2_usb_release_suspend, .settle = y2_usb_settle };
    y2_power.usb.result = -ENODEV;
    if (!y2_usb_state_ready(&y2_power)) return;
    y2_power.usb.result = -EBUSY;
    if (!request_mem_region(Y2_USB_MAC_BASE, Y2_USB_MAC_BYTES, "y2-usb-state")) return;
    if (!request_mem_region(Y2_USB_PHY_BASE, Y2_USB_PHY_BYTES, "y2-usb-state")) goto release_mac;
    mapping.mac = ioremap(Y2_USB_MAC_BASE, Y2_USB_MAC_BYTES);
    mapping.phy = ioremap(Y2_USB_PHY_BASE, Y2_USB_PHY_BYTES);
    y2_power.usb.result = -ENOMEM;
    if (mapping.mac && mapping.phy) {
        y2_usb_state_probe(&io, &y2_power);
        y2_usb_wake_probe(&wake, &y2_power);
    }
    if (mapping.phy) iounmap(mapping.phy);
    if (mapping.mac) iounmap(mapping.mac);
    release_mem_region(Y2_USB_PHY_BASE, Y2_USB_PHY_BYTES);
release_mac:
    release_mem_region(Y2_USB_MAC_BASE, Y2_USB_MAC_BYTES);
}
static void y2_collect_power(void)
{
    struct y2_usb_clock_io clocks = { .read = y2_clock_read };
    y2_pmic_snapshot(&y2_power.power);
    y2_usb_clock_probe(&clocks, &y2_power.power, &y2_power.clock);
    y2_collect_usb();
}
static ssize_t y2_power_snapshot(struct file *file, char __user *buf,
                                 size_t count, loff_t *pos)
{
    int rc = 0;
    if (task_pid_nr(current) != 1) return -EPERM;
    if (count == sizeof(struct y2_usb_live)) return y2_usb_status(buf);
    if (count != sizeof(y2_power)) return -EINVAL;
    if (mutex_lock_interruptible(&y2_frame_lock)) return -ERESTARTSYS;
    if (!y2_power_done) {
        y2_power_done = true; /* cache even failures and failed user copies */
        y2_collect_power();
        y2_usb_begin();
    }
    if (copy_to_user(buf, &y2_power, sizeof(y2_power))) rc = -EFAULT;
    mutex_unlock(&y2_frame_lock);
    return rc ? rc : sizeof(y2_power);
}
static const struct file_operations y2_fops = {
    .write = y2_text_write, .read = y2_power_snapshot,
};
static int __init y2_diagnostic_init(void)
{
    int ret;
    dev_t dev=MKDEV(Y2_TEXT_MAJOR,0);
    if(!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
    ret=register_chrdev_region(dev,1,"y2diag");
    if(ret) return ret;
    cdev_init(&y2_cdev,&y2_fops);
    ret=cdev_add(&y2_cdev,dev,1);
    if(ret) { unregister_chrdev_region(dev,1); return ret; }
    /* Run the unplugged guard before display/storage device_initcall probes.
     * A slow/failing consumer must not delay USB's inherited-state snapshot. */
    y2_power_done = true;
    y2_collect_power();
    y2_usb_begin();
    pr_info("Y2BASELINE M2-BASELINE-02: USB independent of display; 300s owner window\n");
    return ret;
}
device_initcall(y2_diagnostic_init);
#include "/project/kernel/usb/y2_musb.c"
