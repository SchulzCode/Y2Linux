// SPDX-License-Identifier: GPL-2.0-only
/* Temporary Y2-only M2 ownership adapter, included by the board diagnostic.
 * Exact source/access contract: docs/knowledge/m2-usb-enumeration.md.
 * No supply/clock enable, host mode, USB DMA, or analog calibration writes. */
#include <linux/platform_device.h>
#include <linux/of_irq.h>
#include <linux/usb/phy.h>
#include "/src/drivers/usb/musb/musb_core.h"
#include "session.h"
#include "gate.h"

static void __iomem *y2_usb_pmic, *y2_usb_phy;
static struct platform_device *y2_usb_child;
static struct musb *y2_musb;
static struct usb_phy y2_xceiv;
static struct usb_otg y2_otg;
static struct y2_session y2_session;
static struct y2_pwrap_snapshot y2_usb_supply;
static struct y2_usb_live y2_live = { .magic=Y2_USB_LIVE_MAGIC,.devctl=0x100 };
static bool y2_usb_started, y2_usb_finished;
static unsigned long y2_usb_deadline, y2_irq_tick;
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
    if(offset==MUSB_POWER && READ_ONCE(y2_live.result)) value &= ~MUSB_POWER_SOFTCONN;
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
    /* EP0=64; EP1 TX/RX=512 each; EP2 TX=512. Single buffering, 1600 B. */
    writeb(1,b+MUSB_INDEX);
    ok=readb(b+MUSB_TXFIFOSZ)==6 && readb(b+MUSB_RXFIFOSZ)==6 &&
        readw(b+MUSB_TXFIFOADD)==8 && readw(b+MUSB_RXFIFOADD)==72;
    writeb(2,b+MUSB_INDEX);
    ok=ok && readb(b+MUSB_TXFIFOSZ)==6 && readw(b+MUSB_TXFIFOADD)==136;
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
        /* No host/SRP/OTG handling in this physical peripheral experiment. */
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
    if(!y2_usb_takeover_ready(&fresh)) goto fail;
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
    for(i=0;i<8;++i) if(readw(musb->mregs+0x204+16*i)) goto fail;
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
    y2_xceiv=(struct usb_phy){.dev=musb->controller,.label="Y2 guarded integrated USB2 PHY",
        .type=USB_PHY_TYPE_USB2,.otg=&y2_otg,.init=y2_phy_init,.last_event=USB_EVENT_VBUS};
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
    .quirks=MUSB_INDEXED_EP,.init=y2_musb_init,.exit=y2_musb_exit,
    .enable=y2_musb_enable,.disable=y2_musb_disable,.set_mode=y2_musb_mode,
    .readb=y2_musb_readb,.writeb=y2_musb_writeb,.clearb=y2_musb_clearb,
    .readw=y2_musb_readw,.writew=y2_musb_writew,.clearw=y2_musb_clearw,
};
static const struct musb_fifo_cfg y2_fifo[]={
    MUSB_EP_FIFO_SINGLE(1,FIFO_TX,512),MUSB_EP_FIFO_SINGLE(1,FIFO_RX,512),
    MUSB_EP_FIFO_SINGLE(2,FIFO_TX,512),
};
static const struct musb_hdrc_config y2_musb_config={
    .fifo_cfg=y2_fifo,.fifo_cfg_size=ARRAY_SIZE(y2_fifo),.multipoint=true,
    .num_eps=3,.ram_bits=11,.maximum_speed=USB_SPEED_HIGH,
};
static const struct musb_hdrc_platform_data y2_musb_data={
    .mode=MUSB_PERIPHERAL,.config=&y2_musb_config,.platform_ops=&y2_musb_ops,
};
static int y2_usb_register(void)
{
    struct device_node *np=of_find_compatible_node(NULL,NULL,"innioasis,y2-usb-experiment");
    struct resource resources[2]={
        {.start=Y2_USB_MAC_BASE,.end=Y2_USB_MAC_BASE+Y2_USB_MAC_BYTES-1,.flags=IORESOURCE_MEM},
        {.name="mc",.flags=IORESOURCE_IRQ},
    };
    struct platform_device_info info={.name="musb-hdrc",.id=PLATFORM_DEVID_AUTO,
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
    return READ_ONCE(y2_live.result);
}
static void y2_usb_finish(void)
{
    unsigned long flags;
    y2_usb_finished=true;
    if(y2_musb) {
        spin_lock_irqsave(&y2_musb->lock,flags);
        writel(0,y2_musb->mregs+0xa4);
        writeb(readb(y2_musb->mregs+MUSB_POWER)&~MUSB_POWER_SOFTCONN,y2_musb->mregs+MUSB_POWER);
        spin_unlock_irqrestore(&y2_musb->lock,flags);
    }
    WRITE_ONCE(y2_live.configured,0);
    /* Tell PID1 to close ttyGS0 before gadget teardown waits for its user. */
    if(!READ_ONCE(y2_live.result)) y2_usb_phase(Y2_USB_STOPPED);
    if(y2_usb_child) {
        platform_device_unregister(y2_usb_child);
        y2_usb_child=NULL;
    }
    if(y2_usb_phy) {iounmap(y2_usb_phy);y2_usb_phy=NULL;
        release_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);}
    if(y2_usb_pmic) {iounmap(y2_usb_pmic);y2_usb_pmic=NULL;
        release_mem_region(Y2_PWRAP_BASE,Y2_PWRAP_BYTES);}
    pr_info("Y2USB stopped stage=%u result=%d IRQ=%u events=%02x\n",
        y2_live.stage,y2_live.result,y2_live.irqs,y2_live.events);
}
static void y2_usb_worker(struct work_struct *work)
{
    struct y2_pwrap_io io={y2_usb_pmic,y2_power_read,y2_power_write,y2_power_delay};
    struct y2_pwrap_snapshot power;
    int rc;
    if(y2_usb_finished) return;
    if(READ_ONCE(y2_live.result) || time_after_eq(jiffies,y2_usb_deadline)) goto done;
    y2_pwrap_probe(&io,&power);
    ++y2_live.polls;
    if(power.result || power.valid!=7 || !(power.vusb&0x8000)) {
        y2_live.chrdet=0x10000; /* visibly invalid, not cached absence */
        y2_usb_fail(power.result ? power.result : -ENODEV);goto done;
    }
    WRITE_ONCE(y2_live.chrdet,power.chrdet);
    y2_usb_supply=power;
    if(!y2_usb_child && (power.chrdet&0x20)) {
        y2_usb_phase(Y2_USB_PREFLIGHT);
        rc=y2_usb_register();
        if(rc) {y2_usb_fail(rc);goto done;}
        y2_usb_phase(Y2_USB_READY);
    }
    if(y2_musb) {
        if(!(power.chrdet&0x20)) goto done; /* bounded first attachment only */
        WRITE_ONCE(y2_live.devctl,readb(y2_musb->mregs+MUSB_DEVCTL));
        WRITE_ONCE(y2_live.configured,READ_ONCE(y2_musb->g.state)==USB_STATE_CONFIGURED);
        if(y2_live.configured && y2_live.stage!=Y2_USB_CONFIGURED) y2_usb_phase(Y2_USB_CONFIGURED);
    }
    schedule_delayed_work(&y2_usb_work,msecs_to_jiffies(250));
    return;
done:
    y2_usb_finish();
}
static void y2_usb_begin(void)
{
    if(y2_usb_started) return;
    y2_usb_started=true;
    y2_usb_phase(Y2_USB_PREFLIGHT);
    if(!y2_usb_state_ready(&y2_power) || y2_power.wake.result || !y2_power.wake.written ||
       y2_power.wake.after_valid!=0xfff || (y2_power.power.chrdet&0x20)) {
        y2_usb_fail(-ENODEV);return;
    }
    if(!request_mem_region(Y2_PWRAP_BASE,Y2_PWRAP_BYTES,"y2-usb-pmic")) {
        y2_usb_fail(-EBUSY);return;
    }
    y2_usb_pmic=ioremap(Y2_PWRAP_BASE,Y2_PWRAP_BYTES);
    if(!y2_usb_pmic) {release_mem_region(Y2_PWRAP_BASE,Y2_PWRAP_BYTES);y2_usb_fail(-ENOMEM);return;}
    if(!request_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES,"y2-usb-phy")) {
        y2_usb_fail(-EBUSY);y2_usb_finish();return;
    }
    y2_usb_phy=ioremap(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);
    if(!y2_usb_phy) {release_mem_region(Y2_USB_PHY_BASE,Y2_USB_PHY_BYTES);
        y2_usb_fail(-ENOMEM);y2_usb_finish();return;}
    y2_usb_deadline=jiffies+msecs_to_jiffies(50000);
    y2_usb_phase(Y2_USB_ATTACH);
    schedule_delayed_work(&y2_usb_work,msecs_to_jiffies(250));
}
static ssize_t y2_usb_status(char __user *buf)
{
    struct y2_usb_live s={.magic=Y2_USB_LIVE_MAGIC,
        .result=READ_ONCE(y2_live.result),.stage=READ_ONCE(y2_live.stage),
        .polls=READ_ONCE(y2_live.polls),.chrdet=READ_ONCE(y2_live.chrdet),
        .devctl=READ_ONCE(y2_live.devctl),.irqs=READ_ONCE(y2_live.irqs),
        .events=READ_ONCE(y2_live.events),.configured=READ_ONCE(y2_live.configured)};
    return copy_to_user(buf,&s,sizeof(s)) ? -EFAULT : sizeof(s);
}
