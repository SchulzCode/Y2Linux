// SPDX-License-Identifier: GPL-2.0-only
/* Y2 peripheral platform driver. Cached legacy observation ABI is read-only. */
#include <linux/cdev.h>
#include <linux/platform_device.h>
#include <linux/sysfs.h>
#include <linux/of.h>
#include "/project/kernel/usb/recover.h"
#include <linux/capability.h>
#include <linux/fs.h>
#include <linux/io.h>
#include <linux/ioport.h>
#include <linux/delay.h>
#include <linux/mutex.h>
#include <linux/power_supply.h>
#include <linux/pm_runtime.h>
#include <linux/sched.h>
#include <linux/uaccess.h>
#include "boot.h"
#include "source.h"
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
    if (!capable(CAP_SYS_ADMIN)) return -EPERM;
    return count == sizeof(struct y2_text_frame) ? count : -EINVAL;
}
static struct y2_platform_snapshot y2_power;
static struct device *y2_usb_parent;


static void y2_power_delay(void *context) { udelay(10); }
extern int y2_ccf_usb_read(unsigned address, unsigned *value);
extern int y2_pmic_snapshot(struct y2_pwrap_snapshot *snapshot);
static int y2_clock_read(void *context, unsigned address, unsigned *value)
{ return y2_ccf_usb_read(address, value); }
struct y2_usb_mapping { void __iomem *mac, *phy; };
static int y2_usb_recover_write(void *context, unsigned address, unsigned value)
{
    struct y2_usb_mapping *mapping=context;
    switch(address-Y2_USB_MAC_BASE) {
    case 0xa4: writel(value,mapping->mac+0xa4); return readl(mapping->mac+0xa4)==value ? 0 : -EIO;
    case 0x06: case 0x08: writew(value,mapping->mac+address-Y2_USB_MAC_BASE); return 0;
    case 0x0b: case 0x01: case 0x60: writeb(value,mapping->mac+address-Y2_USB_MAC_BASE); return 0;
    }
    switch(address-Y2_USB_PHY_BASE) {
    case 0x1a: case 0x1d: case 0x6d: case 0x68: case 0x69: case 0x6a: case 0x6b: case 0x6e:
        writeb(value,mapping->phy+(address-Y2_USB_PHY_BASE));return 0;
    default:return -EINVAL;
    }
}
static void y2_usb_recover_delay(void *context, unsigned us) { udelay(us); }
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
static void y2_collect_usb(void)
{
    struct y2_usb_mapping mapping = { 0 };
    struct y2_usb_state_io io = { .context = &mapping, .read = y2_usb_read };
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
        {
            struct y2_recover_io recover={io,y2_usb_recover_write,y2_usb_recover_delay};
            y2_usb_recover(&recover,&y2_power);
            pr_info("Y2USB recover rc=%d written=%u\n",y2_power.wake.result,y2_power.wake.written);
        }
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
    if (!capable(CAP_SYS_ADMIN)) return -EPERM;
    if (count == sizeof(struct y2_usb_live)) return y2_usb_status(buf);
    if (count != sizeof(y2_power)) return -EINVAL;
    if (mutex_lock_interruptible(&y2_frame_lock)) return -ERESTARTSYS;
    if (copy_to_user(buf, &y2_power, sizeof(y2_power))) rc = -EFAULT;
    mutex_unlock(&y2_frame_lock);
    return rc ? rc : sizeof(y2_power);
}
static const struct file_operations y2_fops = {
    .write = y2_text_write, .read = y2_power_snapshot,
};
#include <linux/platform_device.h>
#include <linux/of_irq.h>
#include <linux/usb/phy.h>
#include "/src/drivers/usb/musb/musb_core.h"
#include "/project/kernel/usb/session.h"


static void __iomem *y2_usb_phy;
static struct platform_device *y2_usb_child;
static struct musb *y2_musb;
static struct usb_phy y2_xceiv;
static struct usb_otg y2_otg;
static struct y2_session y2_session;
static struct y2_pwrap_snapshot y2_usb_supply;
static DEFINE_SPINLOCK(y2_usb_failure_lock);
static struct y2_usb_live y2_live = { .magic=Y2_USB_LIVE_MAGIC,.devctl=0x100 };
static bool y2_usb_started, y2_usb_finished;
static bool y2_usb_detached;
static bool y2_usb_pm_held;
static struct power_supply *y2_usb_input;
static unsigned y2_usb_budget_ma;
static DEFINE_MUTEX(y2_usb_lifecycle);
static bool y2_usb_data_source;

static bool y2_usb_data_permitted(void)
{
#ifdef CONFIG_Y2_POWER
    /* The offline gadget has ACM only, no userspace listener and no ECM.
     * It requests an SDP budget; PMIC charging already works without it. */
    return READ_ONCE(y2_usb_data_source);
#else
    return true;
#endif
}

int y2_usb_charge_allocation(void)
{
    if (READ_ONCE(y2_live.result) || READ_ONCE(y2_usb_finished)) return 0;
    if (!READ_ONCE(y2_usb_child) || READ_ONCE(y2_usb_detached)) return -1;
    return READ_ONCE(y2_usb_budget_ma) * 1000;
}

void y2_usb_source_invalidate(void)
{
    WRITE_ONCE(y2_usb_data_source, false);
}

int y2_usb_bc11_begin(void)
{
    unsigned value;
    int ret = 0;
    mutex_lock(&y2_usb_lifecycle);
    if (!y2_usb_phy || y2_usb_finished || READ_ONCE(y2_live.result)) ret = -ENODEV;
    else if (y2_musb && !y2_usb_detached) ret = -EAGAIN;
    if (ret) { mutex_unlock(&y2_usb_lifecycle); return ret; }
    /* Actual Y2 Charger_Detect_Init: USBPHYACR6 BC1.1 switch, bit 7.
     * The USB clock provider remains enabled throughout this transaction. */
    value = readb(y2_usb_phy + 0x1a);
    writeb(value | 0x80, y2_usb_phy + 0x1a);
    if (readb(y2_usb_phy + 0x1a) != (value | 0x80)) {
        writeb(value, y2_usb_phy + 0x1a);
        mutex_unlock(&y2_usb_lifecycle);
        return -EIO;
    }
    udelay(50);
    return 0;
}

int y2_usb_bc11_end(bool data_source)
{
    unsigned value = readb(y2_usb_phy + 0x1a) & ~0x80;
    int ret;
    writeb(value, y2_usb_phy + 0x1a);
    udelay(1);
    ret = readb(y2_usb_phy + 0x1a) == value ? 0 : -EIO;
    WRITE_ONCE(y2_usb_data_source, data_source && !ret);
    mutex_unlock(&y2_usb_lifecycle);
    return ret;
}
/* USB core calls set_power for configuration, reset, disconnect and bus
 * suspend/resume, sometimes under the MUSB spinlock. Publish only the budget;
 * power_supply's notifier schedules the sleeping charger/regmap owner. */
static int y2_usb_set_power(struct usb_phy *phy, unsigned ma)
{
    WRITE_ONCE(y2_usb_budget_ma, min(ma, 500U));
    if (y2_usb_input) power_supply_changed(y2_usb_input);
    return 0;
}
static enum power_supply_property y2_usb_input_props[] = {
    POWER_SUPPLY_PROP_ONLINE, POWER_SUPPLY_PROP_CURRENT_MAX,
};
static int y2_usb_input_get(struct power_supply *psy, enum power_supply_property p,
                            union power_supply_propval *v)
{
    bool available = !READ_ONCE(y2_live.result) && !READ_ONCE(y2_usb_finished) &&
        !READ_ONCE(y2_usb_detached);
    if (p == POWER_SUPPLY_PROP_CURRENT_MAX)
        v->intval = available ? READ_ONCE(y2_usb_budget_ma) * 1000 : 0;
    else if (p == POWER_SUPPLY_PROP_ONLINE)
        v->intval = available && !!(READ_ONCE(y2_live.chrdet) & 0x20);
    else return -EINVAL;
    return 0;
}
static const struct power_supply_desc y2_usb_input_desc = {
    .name = "y2-usb-input", .type = POWER_SUPPLY_TYPE_USB,
    .properties = y2_usb_input_props, .num_properties = ARRAY_SIZE(y2_usb_input_props),
    .get_property = y2_usb_input_get,
};
static unsigned long y2_irq_tick;
static unsigned y2_irq_burst;
static void y2_usb_worker(struct work_struct *work);
static DECLARE_DELAYED_WORK(y2_usb_work,y2_usb_worker);

static void y2_usb_phase(unsigned stage)
{
    WRITE_ONCE(y2_live.stage,stage);
    pr_info("Y2USB stage=%u result=%d\n",stage,READ_ONCE(y2_live.result));
}
static void y2_usb_fail(int rc)
{
    if(!READ_ONCE(y2_live.result)) WRITE_ONCE(y2_live.result,rc);
}
static unsigned y2_session_read(void *context,unsigned offset)
{
    struct musb *musb=context;
    return offset==0x100 ? readb(musb->mregs+MUSB_DEVCTL) : readb(y2_usb_phy+offset);
}
static void y2_session_write(void *context,unsigned offset,unsigned value)
{
    if(WARN_ON_ONCE(offset!=0x6c && offset!=0x6d)) return;
    writeb(value,y2_usb_phy+offset);
}
static struct y2_session_io y2_session_io(struct musb *musb)
{
    return (struct y2_session_io){musb,y2_session_read,y2_session_write,y2_power_delay};
}
static u8 y2_musb_readb(void __iomem *base,u32 offset)
{ return readb(base+offset); }
static u16 y2_musb_readw(void __iomem *base,u32 offset)
{ return readw(base+offset); }
static void y2_musb_writeb(void __iomem *base,u32 offset,u8 value)
{
    /* No SRP/host request, even if the generic state machine requests one. */
    if(offset==MUSB_DEVCTL) value &= ~(MUSB_DEVCTL_SESSION|MUSB_DEVCTL_HR);
    if(offset==MUSB_POWER && (READ_ONCE(y2_live.result) || READ_ONCE(y2_usb_detached)))
        value &= ~MUSB_POWER_SOFTCONN;
    writeb(value,base+offset);
}
static void y2_musb_writew(void __iomem *base,u32 offset,u16 value)
{ writew(value,base+offset); }
static u8 y2_musb_clearb(void __iomem *base,u32 offset)
{
    u8 v=readb(base+offset);
    writeb(v,base+offset); /* FM: sampled W1C, not read-to-clear. */
    return v;
}
static u16 y2_musb_clearw(void __iomem *base,u32 offset)
{
    u16 v=readw(base+offset);
    writew(v,base+offset);
    return v;
}
static void y2_musb_disable(struct musb *musb)
{ writel(0,musb->mregs+0xa4); }

static int y2_musb_fifos(struct musb *musb)
{
    void __iomem *b=musb->mregs;
    u8 index=readb(b+MUSB_INDEX);
    int ok;
    /* ECM/ACM: two bulk pairs and two 16-byte notifications, 2144 bytes. */
    writeb(1,b+MUSB_INDEX);
    ok=readb(b+MUSB_TXFIFOSZ)==6 && readb(b+MUSB_RXFIFOSZ)==6 &&
        readw(b+MUSB_TXFIFOADD)==8 && readw(b+MUSB_RXFIFOADD)==72;
    writeb(2,b+MUSB_INDEX);
    ok=ok && readb(b+MUSB_TXFIFOSZ)==1 && readw(b+MUSB_TXFIFOADD)==136;
    writeb(3,b+MUSB_INDEX);
    ok=ok && readb(b+MUSB_TXFIFOSZ)==6 && readb(b+MUSB_RXFIFOSZ)==6 &&
        readw(b+MUSB_TXFIFOADD)==138 && readw(b+MUSB_RXFIFOADD)==202;
    writeb(4,b+MUSB_INDEX);
    ok=ok && readb(b+MUSB_TXFIFOSZ)==1 && readw(b+MUSB_TXFIFOADD)==266;
    writeb(index,b+MUSB_INDEX);
    return ok;
}
static void y2_musb_enable(struct musb *musb)
{
    if(!y2_musb_fifos(musb)) y2_usb_fail(-EIO);
    /* Gate before SOFTCONN; DMA/IDDIG interrupt sources remain masked. */
    writel(READ_ONCE(y2_live.result) ? 0 : 7,musb->mregs+0xa4);
}
static irqreturn_t y2_musb_interrupt(int irq,void *context)
{
    struct musb *musb=context;
    unsigned long flags;
    u32 pending;
    irqreturn_t rc=IRQ_NONE;
    /* Bound a stuck interrupt even when no core status bit explains it. */
    if(y2_irq_tick!=jiffies) {y2_irq_tick=jiffies;y2_irq_burst=0;}
    ++y2_live.irqs;
    if(++y2_irq_burst>512) {
        writel(0,musb->mregs+0xa4);
        disable_irq_nosync(irq);
        y2_usb_fail(-EOVERFLOW);
        return IRQ_HANDLED;
    }
    spin_lock_irqsave(&musb->lock,flags);
    pending=readl(musb->mregs+0xa0)&readl(musb->mregs+0xa4);
    if(pending & 7) {
        musb->int_usb=y2_musb_clearb(musb->mregs,MUSB_INTRUSB) &
            readb(musb->mregs+MUSB_INTRUSBE);
        musb->int_tx=y2_musb_clearw(musb->mregs,MUSB_INTRTX) &
            readw(musb->mregs+MUSB_INTRTXE);
        musb->int_rx=y2_musb_clearw(musb->mregs,MUSB_INTRRX) &
            readw(musb->mregs+MUSB_INTRRXE);
        y2_live.events |= musb->int_usb;
        /* No host/SRP/OTG handling in this peripheral-only board. */
        musb->int_usb &= MUSB_INTR_SUSPEND|MUSB_INTR_RESUME|MUSB_INTR_RESET|MUSB_INTR_DISCONNECT;
        if(musb->int_usb & MUSB_INTR_RESET) {
            writeb(0,musb->mregs+MUSB_INDEX);
            writeb(0,musb->mregs+MUSB_FADDR);
        }
        if(musb->int_usb || musb->int_tx || musb->int_rx) rc=musb_interrupt(musb);
        else rc=IRQ_HANDLED; /* owned sampled status was acknowledged */
    }
    spin_unlock_irqrestore(&musb->lock,flags);
    return rc;
}
static int y2_set_peripheral(struct usb_otg *otg,struct usb_gadget *gadget)
{ otg->gadget=gadget;return 0; }
static int y2_phy_init(struct usb_phy *phy)
{ return READ_ONCE(y2_live.result); }
static int y2_musb_mode(struct musb *musb,u8 mode)
{ return mode==MUSB_PERIPHERAL ? 0 : -EOPNOTSUPP; }

static int y2_musb_init(struct musb *musb)
{
    struct y2_usb_mapping mapping={musb->mregs,y2_usb_phy};
    struct y2_usb_state_io io={&mapping,y2_usb_read};
    struct y2_platform_snapshot fresh=y2_power;
    struct y2_usb_clock_io clocks={.read=y2_clock_read};
    struct y2_session_io session=y2_session_io(musb);
    unsigned i,value;
    int rc=-ENODEV;
    y2_usb_phase(Y2_USB_PREFLIGHT);
    fresh.power=y2_usb_supply;
    y2_usb_clock_probe(&clocks,&fresh.power,&fresh.clock);
    y2_usb_state_probe(&io,&fresh);
    pr_info("Y2USB fresh power=%d/%x CHR=%04x clock=%d/%x USB=%d/%x\n",
        fresh.power.result,fresh.power.valid,fresh.power.chrdet,
        fresh.clock.result,fresh.clock.valid,fresh.usb.result,fresh.usb.valid);
    /* Re-normalize digital device state at registration as the cable may have
     * changed since probe. Same driver and recovery sequence for every boot. */
    { struct y2_recover_io recover={io,y2_usb_recover_write,y2_usb_recover_delay};
      y2_usb_recover(&recover,&fresh);
      if(fresh.wake.result) {rc=fresh.wake.result;goto fail;}
      y2_power.wake=fresh.wake; }
    for(i=0;i<7;++i) {
        value=readb(y2_usb_phy+y2_usb_mode_offsets[i]);
        if(value!=y2_power.wake.controls[i]) goto fail;
    }
    /* Claim the idle controller. No active inherited DMA is reset or reused. */
    writel(0,musb->mregs+0xa4);
    writeb(0,musb->mregs+MUSB_INTRUSBE);
    writew(0,musb->mregs+MUSB_INTRTXE);
    writew(0,musb->mregs+MUSB_INTRRXE);
    writeb(0,musb->mregs+MUSB_POWER);
    writeb(0,musb->mregs+MUSB_DEVCTL);
    for(i=0;i<8;++i) if(readw(musb->mregs+0x204+16*i)&1) goto fail;
    writeb(0,musb->mregs+MUSB_INDEX);
    if(!(readb(musb->mregs+0x1f)&MUSB_CONFIGDATA_DYNFIFO)) goto fail;
    y2_usb_phase(Y2_USB_SESSION);
    rc=y2_session_start(&session,&y2_session);
    WRITE_ONCE(y2_live.devctl,y2_session.devctl);
    if(rc) goto fail;
    for(i=0;i<7;++i) {
        value=readb(y2_usb_phy+y2_usb_mode_offsets[i]);
        pr_info("Y2USB PHY%02x saved=%02x session=%02x\n",
            y2_usb_mode_offsets[i],y2_power.wake.controls[i],value);
        if(value!=y2_power.wake.controls[i]) {rc=-EIO;goto fail;}
    }
    y2_xceiv=(struct usb_phy){.dev=musb->controller,.label="Y2 integrated USB2 PHY",
        .type=USB_PHY_TYPE_USB2,.otg=&y2_otg,.init=y2_phy_init,.last_event=USB_EVENT_VBUS,
        .set_power=y2_usb_set_power};
    y2_otg=(struct usb_otg){.usb_phy=&y2_xceiv,.set_peripheral=y2_set_peripheral};
    ATOMIC_INIT_NOTIFIER_HEAD(&y2_xceiv.notifier);
    musb->xceiv=&y2_xceiv;musb->is_host=false;musb->isr=y2_musb_interrupt;
    y2_musb=musb;
    y2_usb_phase(Y2_USB_REGISTER);
    return 0;
fail:
    y2_usb_fail(rc);
    y2_session_end(&session,&y2_session);
    return rc;
}
static int y2_musb_exit(struct musb *musb)
{
    struct y2_session_io session=y2_session_io(musb);
    writel(0,musb->mregs+0xa4);
    writeb(readb(musb->mregs+MUSB_POWER)&~MUSB_POWER_SOFTCONN,musb->mregs+MUSB_POWER);
    y2_session_end(&session,&y2_session);
    y2_musb=NULL;
    return 0;
}
static const struct musb_platform_ops y2_musb_ops={
    .quirks=MUSB_INDEXED_EP | MUSB_PRESERVE_SESSION,.init=y2_musb_init,.exit=y2_musb_exit,
    .enable=y2_musb_enable,.disable=y2_musb_disable,.set_mode=y2_musb_mode,
    .readb=y2_musb_readb,.writeb=y2_musb_writeb,.clearb=y2_musb_clearb,
    .readw=y2_musb_readw,.writew=y2_musb_writew,.clearw=y2_musb_clearw,
};
static const struct musb_fifo_cfg y2_fifo[]={
    MUSB_EP_FIFO_SINGLE(1,FIFO_TX,512),MUSB_EP_FIFO_SINGLE(1,FIFO_RX,512),
    MUSB_EP_FIFO_SINGLE(2,FIFO_TX,16),
    MUSB_EP_FIFO_SINGLE(3,FIFO_TX,512),MUSB_EP_FIFO_SINGLE(3,FIFO_RX,512),
    MUSB_EP_FIFO_SINGLE(4,FIFO_TX,16),
};
static const struct musb_hdrc_config y2_musb_config={
    .fifo_cfg=y2_fifo,.fifo_cfg_size=ARRAY_SIZE(y2_fifo),.multipoint=true,
    .num_eps=5,.ram_bits=11,.maximum_speed=USB_SPEED_HIGH,
};
static const struct musb_hdrc_platform_data y2_musb_data={
    .mode=MUSB_PERIPHERAL,.config=&y2_musb_config,.platform_ops=&y2_musb_ops,
};
/* One reference spans a live USB session. The polling worker and PHY session
 * transitions must not race MUSB's runtime save/restore of endpoint state.
 * Release after detach so absence can idle; system suspend still uses the
 * ordinary MUSB PM callbacks. This replaces the POWER-02 userspace `on` pin. */
static int y2_usb_runtime_get(void)
{
    int ret;
    if (y2_usb_pm_held) return 0;
    ret = pm_runtime_resume_and_get(&y2_usb_child->dev);
    if (ret < 0) return ret;
    y2_usb_pm_held = true;
    return 0;
}
static void y2_usb_runtime_put(void)
{
    if (!y2_usb_pm_held) return;
    y2_usb_pm_held = false;
    pm_runtime_mark_last_busy(&y2_usb_child->dev);
    pm_runtime_put_autosuspend(&y2_usb_child->dev);
}
static int y2_usb_register(void)
{
    struct device_node *np=of_find_compatible_node(NULL,NULL,"innioasis,y2-usb");
    struct resource resources[2]={
        {.start=Y2_USB_MAC_BASE,.end=Y2_USB_MAC_BASE+Y2_USB_MAC_BYTES-1,.flags=IORESOURCE_MEM},
        {.name="mc",.flags=IORESOURCE_IRQ},
    };
    struct platform_device_info info={.name="musb-hdrc",.id=PLATFORM_DEVID_AUTO,
        .parent=y2_usb_parent,
        .res=resources,.num_res=2,.data=&y2_musb_data,.size_data=sizeof(y2_musb_data)};
    int irq;
    if(!np) return -ENODEV;
    irq=of_irq_get(np,0);
    of_node_put(np);
    if(irq<=0) return irq ? irq : -ENODEV;
    resources[1].start=resources[1].end=irq;
    y2_usb_child=platform_device_register_full(&info);
    if(IS_ERR(y2_usb_child)) {int rc=PTR_ERR(y2_usb_child);y2_usb_child=NULL;return rc;}
    if(!y2_usb_child->dev.driver || !y2_musb) return -ENODEV;
    if (READ_ONCE(y2_live.result)) return READ_ONCE(y2_live.result);
    return y2_usb_runtime_get();
}
static void y2_usb_finish(void)
{
    unsigned long flags;
    y2_usb_finished=true;
    y2_usb_set_power(NULL,0);
    if(y2_musb && !y2_usb_runtime_get()) {
        spin_lock_irqsave(&y2_musb->lock,flags);
        writel(0,y2_musb->mregs+0xa4);
        writeb(readb(y2_musb->mregs+MUSB_POWER)&~MUSB_POWER_SOFTCONN,y2_musb->mregs+MUSB_POWER);
        spin_unlock_irqrestore(&y2_musb->lock,flags);
    }
    WRITE_ONCE(y2_live.configured,0);
    /* Tell PID1 to close ttyGS0 before gadget teardown waits for its user. */
    if(!READ_ONCE(y2_live.result)) y2_usb_phase(Y2_USB_STOPPED);
    if(y2_usb_child) {
        y2_usb_runtime_put();
        platform_device_unregister(y2_usb_child);
        y2_usb_child=NULL;
    }
    if(y2_usb_phy) {iounmap(y2_usb_phy);y2_usb_phy=NULL;
        release_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);}
    pr_info("Y2USB stopped stage=%u result=%d IRQ=%u events=%02x\n",
        y2_live.stage,y2_live.result,y2_live.irqs,y2_live.events);
}
/* The controller remains registered: no gadget-unregister wait on PID1's tty.
 * Upstream musb_g_disconnect requires the lock and invokes endpoint teardown.
 * The write callback blocks delayed gadget_work from restoring the pullup. */
static void y2_usb_detach(void)
{
    struct y2_session_io session=y2_session_io(y2_musb);
    unsigned long flags;
    WRITE_ONCE(y2_usb_detached,true);
    WRITE_ONCE(y2_live.configured,0);
    y2_usb_phase(Y2_USB_DETACHED);
    spin_lock_irqsave(&y2_musb->lock,flags);
    writel(0,y2_musb->mregs+0xa4);
    writeb(readb(y2_musb->mregs+MUSB_POWER)&~MUSB_POWER_SOFTCONN,y2_musb->mregs+MUSB_POWER);
    musb_g_disconnect(y2_musb);
    musb_stop(y2_musb);
    spin_unlock_irqrestore(&y2_musb->lock,flags);
    y2_session_end(&session,&y2_session);
    WRITE_ONCE(y2_live.devctl,readb(y2_musb->mregs+MUSB_DEVCTL));
    y2_usb_runtime_put();
    pr_info("Y2USB detached; PID1 continues; persistent reconnect enabled\n");
}
static int y2_usb_reconnect(void)
{
    struct y2_usb_clock_io clocks={.read=y2_clock_read};
    struct y2_platform_snapshot fresh=y2_power;
    struct y2_session_io session=y2_session_io(y2_musb);
    unsigned long flags;
    unsigned i;
    int rc;
    rc = y2_usb_runtime_get();
    if (rc) return rc;
    fresh.power=y2_usb_supply;
    y2_usb_clock_probe(&clocks,&fresh.power,&fresh.clock);
    if(!y2_usb_state_ready(&fresh) || !(fresh.power.chrdet&0x20)) return -ENODEV;
    if(readb(y2_usb_phy+0x6c)!=y2_session.before_c ||
       readb(y2_usb_phy+0x6d)!=y2_session.before_d ||
       (readb(y2_musb->mregs+MUSB_DEVCTL)&0x87)!=0x80) return -ENODEV;
    for(i=0;i<7;++i)
        if(readb(y2_usb_phy+y2_usb_mode_offsets[i])!=y2_power.wake.controls[i]) return -EIO;
    for(i=0;i<7;++i)
        if(i!=4 && i!=5 && readb(y2_usb_phy+0x68+i)!=y2_power.wake.after[i]) return -EIO;
    for(i=0;i<8;++i) if(readw(y2_musb->mregs+0x204+16*i)&1) return -ENODEV;
    rc=y2_session_start(&session,&y2_session);
    if(rc) return rc; /* terminal teardown restores any partially forced inputs */
    spin_lock_irqsave(&y2_musb->lock,flags);
    /* Start rechecks the retained FIFO layout; stale status is sampled W1C. */
    y2_musb_clearb(y2_musb->mregs,MUSB_INTRUSB);
    y2_musb_clearw(y2_musb->mregs,MUSB_INTRTX);
    y2_musb_clearw(y2_musb->mregs,MUSB_INTRRX);
    musb_start(y2_musb);
    if(!READ_ONCE(y2_live.result)) {
        WRITE_ONCE(y2_usb_detached,false);
        if(y2_musb->softconnect)
            y2_musb_writeb(y2_musb->mregs,MUSB_POWER,
                readb(y2_musb->mregs+MUSB_POWER)|MUSB_POWER_SOFTCONN);
    }
    spin_unlock_irqrestore(&y2_musb->lock,flags);
    if(READ_ONCE(y2_live.result)) return READ_ONCE(y2_live.result);
    y2_usb_phase(Y2_USB_READY);
    pr_info("Y2USB reconnect armed; host reset/configuration required\n");
    return 0;
}
static void y2_usb_worker(struct work_struct *work)
{
    struct y2_pwrap_snapshot power;
    int rc;
    mutex_lock(&y2_usb_lifecycle);
    if(y2_usb_finished) goto out;
    if(READ_ONCE(y2_live.result)) goto done;
    y2_pmic_snapshot(&power);
    ++y2_live.polls;
    if(power.result || power.valid!=7 || !(power.vusb&0x8000)) {
        spin_lock(&y2_usb_failure_lock);
        y2_live.power_failure=power;
        spin_unlock(&y2_usb_failure_lock);
        y2_live.chrdet=0x10000; /* visibly invalid, not cached absence */
        y2_usb_fail(power.result ? power.result : -ENODEV);goto done;
    }
    WRITE_ONCE(y2_live.chrdet,power.chrdet);
    y2_usb_supply=power;
    if (!(power.chrdet & 0x20)) y2_usb_source_invalidate();
    if(!y2_usb_child && (power.chrdet&0x20) && y2_usb_data_permitted()) {
        y2_usb_phase(Y2_USB_PREFLIGHT);
        rc=y2_usb_register();
        if(rc) {y2_usb_fail(rc);goto done;}
        y2_usb_phase(Y2_USB_READY);
    }
    if(y2_musb) {
        if(!(power.chrdet&0x20) || !y2_usb_data_permitted()) {
            if(!y2_usb_detached) {
                y2_usb_detach();
            }
            goto again;
        }
        if(y2_usb_detached) {
            rc=y2_usb_reconnect();
            if(rc) {y2_usb_fail(rc);goto done;}
        }
        WRITE_ONCE(y2_live.devctl,readb(y2_musb->mregs+MUSB_DEVCTL));
        WRITE_ONCE(y2_live.configured,READ_ONCE(y2_musb->g.state)==USB_STATE_CONFIGURED);
        if(y2_live.configured && y2_live.stage!=Y2_USB_CONFIGURED) y2_usb_phase(Y2_USB_CONFIGURED);
    }
again:
    queue_delayed_work(system_freezable_wq,&y2_usb_work,msecs_to_jiffies(250));
    goto out;
done:
    y2_usb_finish();
out:
    mutex_unlock(&y2_usb_lifecycle);
}
static void y2_usb_begin(void)
{
    if(y2_usb_started) return;
    y2_usb_started=true;
    y2_usb_phase(Y2_USB_PREFLIGHT);
    if(!y2_usb_state_ready(&y2_power) || y2_power.wake.result || !y2_power.wake.written ||
       y2_power.wake.after_valid!=0xfff) {
        y2_usb_fail(-ENODEV);return;
    }
    if(!request_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES,"y2-usb-phy")) {
        y2_usb_fail(-EBUSY);y2_usb_finish();return;
    }
    y2_usb_phy=ioremap(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);
    if(!y2_usb_phy) {release_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);
        y2_usb_fail(-ENOMEM);y2_usb_finish();return;}
    y2_usb_phase(Y2_USB_ATTACH);
    queue_delayed_work(system_freezable_wq,&y2_usb_work,msecs_to_jiffies(250));
}
static ssize_t y2_usb_status(char __user *buf)
{
    struct y2_usb_live s={.magic=Y2_USB_LIVE_MAGIC,
        .result=READ_ONCE(y2_live.result),.stage=READ_ONCE(y2_live.stage),
        .polls=READ_ONCE(y2_live.polls),.chrdet=READ_ONCE(y2_live.chrdet),
        .devctl=READ_ONCE(y2_live.devctl),.irqs=READ_ONCE(y2_live.irqs),
        .events=READ_ONCE(y2_live.events),.configured=READ_ONCE(y2_live.configured)};
    spin_lock(&y2_usb_failure_lock);
    s.power_failure=y2_live.power_failure;
    spin_unlock(&y2_usb_failure_lock);
    return copy_to_user(buf,&s,sizeof(s)) ? -EFAULT : sizeof(s);
}

static ssize_t status_show(struct device *dev, struct device_attribute *attr, char *buf)
{
    unsigned i; ssize_t n;
    n = sysfs_emit(buf, "stage=%u error=%d configured=%u polls=%u chrdet=%x irqs=%u events=%x\n",
        READ_ONCE(y2_live.stage), READ_ONCE(y2_live.result), READ_ONCE(y2_live.configured),
        READ_ONCE(y2_live.polls), READ_ONCE(y2_live.chrdet), READ_ONCE(y2_live.irqs), READ_ONCE(y2_live.events));
    n += sysfs_emit_at(buf, n, "recovery=%d writes=%u power=%d clock=%d\n",
        y2_power.wake.result, y2_power.wake.written, y2_power.power.result, y2_power.clock.result);
    for (i=0; i<Y2_USB_STATE_COUNT; i++)
        n += sysfs_emit_at(buf,n,"initial_%08x=%x valid=%u\n",y2_usb_registers[i].address,
            y2_power.usb.values[i],!!(y2_power.usb.valid & (1U<<i)));
    return n;
}
static DEVICE_ATTR_RO(status);
static struct attribute *y2_usb_attrs[] = { &dev_attr_status.attr, NULL };
ATTRIBUTE_GROUPS(y2_usb);
static int y2_usb_probe(struct platform_device *pdev)
{
    int ret;
    struct power_supply_config input_cfg = { .fwnode = dev_fwnode(&pdev->dev) };
    dev_t dev=MKDEV(Y2_TEXT_MAJOR,0);
    /* Providers may bind later. A missing provider is not a permanent boot failure. */
    y2_pmic_snapshot(&y2_power.power);
    if (y2_power.power.result) return y2_power.power.result;
    if (y2_power.power.valid != 7) return -EIO;
    y2_usb_input=devm_power_supply_register(&pdev->dev,&y2_usb_input_desc,&input_cfg);
    if(IS_ERR(y2_usb_input)) {ret=PTR_ERR(y2_usb_input);y2_usb_input=NULL;return ret;}
    ret=register_chrdev_region(dev,1,"y2diag");
    if(ret) return ret;
    cdev_init(&y2_cdev,&y2_fops);
    ret=cdev_add(&y2_cdev,dev,1);
    if(ret) { unregister_chrdev_region(dev,1); return ret; }
    y2_collect_power();
    y2_usb_parent = &pdev->dev;
    y2_usb_begin();
    dev_info(&pdev->dev,"peripheral USB platform initialized; status is observational\n");
    return 0;
}
static const struct of_device_id y2_usb_match[] = {
    { .compatible = "innioasis,y2-usb" }, { }
};
static int y2_usb_prepare(struct device *dev)
{
    /* Stop PHY/register polling before the MUSB child starts saving state. */
    cancel_delayed_work_sync(&y2_usb_work);
    return 0;
}
static void y2_usb_complete(struct device *dev)
{
    if (y2_usb_started && !y2_usb_finished)
        queue_delayed_work(system_freezable_wq,&y2_usb_work,msecs_to_jiffies(250));
}
static const struct dev_pm_ops y2_usb_pm = {
    .prepare = y2_usb_prepare, .complete = y2_usb_complete,
};
static struct platform_driver y2_usb_driver = {
    .probe = y2_usb_probe,
    .driver = { .name = "y2-usb", .of_match_table = y2_usb_match,
                .dev_groups = y2_usb_groups, .pm = pm_sleep_ptr(&y2_usb_pm), .suppress_bind_attrs = true },
};
builtin_platform_driver(y2_usb_driver);
