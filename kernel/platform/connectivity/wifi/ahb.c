// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 fullmac AHB PIO transport. The register protocol follows MediaTek's
 * GPL ahb.c; Linux owns resource/IRQ/rfkill lifetimes and the parent owns power.
 * PIO is the production path, not a temporary raw-MMIO bootstrap. */
#include "gl_os.h"
#include "precomp.h"
#include <linux/platform_device.h>
#include <linux/rfkill.h>
#include <linux/unaligned.h>

struct y2_wifi {
	struct platform_device *pdev;
	struct y2_conn *conn;
	struct rfkill *rfkill;
	struct work_struct work;
	void __iomem *base;
	int irq;
	bool wanted, running, removing, irq_on;
	struct net_device *netdev;
};
static probe_card card_probe;
static remove_card card_remove;
static struct y2_wifi *single;

static void radio_work(struct work_struct *work)
{
	struct y2_wifi *w=container_of(work,struct y2_wifi,work);
	for (;;) {
		bool wanted=READ_ONCE(w->wanted) && !READ_ONCE(w->removing);
		int ret;
		if (wanted==w->running) break;
		if (!wanted) {
			card_remove(); w->running=false;
			y2_conn_put(w->conn,Y2_CONN_WIFI);
			continue;
		}
		ret=y2_conn_get(w->conn,Y2_CONN_WIFI);
		if (!ret) {
			ret=card_probe(&w->pdev->dev);
			if (ret) y2_conn_put(w->conn,Y2_CONN_WIFI);
		}
		if (ret) {
			dev_err(&w->pdev->dev,"Wi-Fi enable failed: %d\n",ret);
			WRITE_ONCE(w->wanted,false); rfkill_set_sw_state(w->rfkill,true);
			break;
		}
		w->running=true;
	}
}
static int set_block(void *data, bool blocked)
{
	struct y2_wifi *w=data;
	WRITE_ONCE(w->wanted,!blocked);
	w->conn->wifi_wanted=!blocked;
	/* Firmware boot can include cold calibration. It never holds the global
	 * rfkill lock or blocks player boot; interface appearance reports success. */
	schedule_work(&w->work);
	return 0;
}
static const struct rfkill_ops radio_ops={.set_block=set_block};
static int wifi_probe(struct platform_device *pdev)
{
	struct y2_wifi *w; struct resource *r; int ret;
	if (single) return -EBUSY;
	w=devm_kzalloc(&pdev->dev,sizeof(*w),GFP_KERNEL); if (!w) return -ENOMEM;
	w->pdev=pdev; w->conn=*(struct y2_conn **)dev_get_platdata(&pdev->dev);
	struct platform_device *parent=to_platform_device(pdev->dev.parent);
	r=platform_get_resource_byname(parent,IORESOURCE_MEM,"wifi");
	w->base=devm_ioremap_resource(&pdev->dev,r); if (IS_ERR(w->base)) return PTR_ERR(w->base);
	w->irq=platform_get_irq_byname(parent,"wifi"); if (w->irq<0) return w->irq;
	INIT_WORK(&w->work,radio_work); platform_set_drvdata(pdev,w);
	w->rfkill=rfkill_alloc("y2-wifi",&pdev->dev,RFKILL_TYPE_WLAN,&radio_ops,w);
	if (!w->rfkill) return -ENOMEM;
	w->wanted=w->conn->wifi_wanted;
	rfkill_init_sw_state(w->rfkill,!w->wanted);
	single=w;
	ret=rfkill_register(w->rfkill);
	if (ret) { single=NULL; rfkill_destroy(w->rfkill); return ret; }
	if (w->wanted) schedule_work(&w->work);
	return 0;
}
static void wifi_remove(struct platform_device *pdev)
{
	struct y2_wifi *w=platform_get_drvdata(pdev);
	WRITE_ONCE(w->removing,true); rfkill_unregister(w->rfkill);
	cancel_work_sync(&w->work);
	if (w->running) { card_remove(); w->running=false; y2_conn_put(w->conn,Y2_CONN_WIFI); }
	rfkill_destroy(w->rfkill); single=NULL;
}
static struct platform_driver driver={.probe=wifi_probe,.remove=wifi_remove,
	.driver={.name="y2-mt6582-wifi"}};
WLAN_STATUS glRegisterBus(probe_card probe, remove_card remove)
{
	card_probe=probe; card_remove=remove;
	return platform_driver_register(&driver) ? WLAN_STATUS_FAILURE : WLAN_STATUS_SUCCESS;
}
VOID glUnregisterBus(remove_card remove) { platform_driver_unregister(&driver); }
BOOL glBusInit(PVOID data) { return dev_get_drvdata(data)!=NULL; }
VOID glBusRelease(PVOID data) {}
VOID glSetHifInfo(P_GLUE_INFO_T glue, UINT_32 cookie)
{
	struct device *dev=(void *)(uintptr_t)cookie;
	struct y2_wifi *w=dev_get_drvdata(dev);
	glue->rHifInfo.Dev=dev; glue->rHifInfo.bus=w;
	glue->rHifInfo.HifRegBaseAddr=w->base;
	glue->rHifInfo.ChipID=readl(w->base+MCR_WCIR)&0xffff;
	glue->rHifInfo.fgDmaEnable=FALSE;
	init_waitqueue_head(&glue->waitq);
}
VOID glClearHifInfo(P_GLUE_INFO_T glue) { glue->rHifInfo.bus=NULL; }
VOID glGetChipInfo(GLUE_INFO_T *glue, UINT_8 *buf) { sprintf(buf,"MT%04x AHB",glue->rHifInfo.ChipID); }
VOID glResetHif(GLUE_INFO_T *glue) { /* No Wi-Fi DMA owner; parent resets shared hardware. */ }
VOID glSetPowerState(P_GLUE_INFO_T glue, UINT_32 mode) {}
static bool available(GLUE_INFO_T *glue)
{
	struct y2_wifi *w=glue->rHifInfo.bus;
	return w && READ_ONCE(w->conn->powered) && !READ_ONCE(w->conn->failure);
}
static irqreturn_t irq(int n, void *data)
{
	struct net_device *net=data;
	P_GLUE_INFO_T glue=*(P_GLUE_INFO_T *)netdev_priv(net);
	if (!available(glue)) return IRQ_HANDLED;
	writel(WHLPCR_INT_EN_CLR,glue->rHifInfo.HifRegBaseAddr+MCR_WHLPCR);
	if (!(glue->u4Flag & GLUE_FLAG_HALT)) {
		set_bit(GLUE_FLAG_INT_BIT,&glue->u4Flag); wake_up_interruptible(&glue->waitq);
	}
	return IRQ_HANDLED;
}
INT_32 glBusSetIrq(PVOID dev, PVOID isr, PVOID cookie)
{
	GLUE_INFO_T *glue=cookie; struct y2_wifi *w=glue->rHifInfo.bus;
	int ret=request_irq(w->irq,irq,0,"y2-wifi",dev);
	if (!ret) { w->irq_on=true; w->netdev=dev; }
	return ret;
}
VOID glBusFreeIrq(PVOID dev, PVOID cookie)
{
	GLUE_INFO_T *glue=cookie; struct y2_wifi *w=glue->rHifInfo.bus;
	if (w && w->irq_on) { free_irq(w->irq,w->netdev); w->irq_on=false; w->netdev=NULL; }
}
BOOL kalDevRegRead(GLUE_INFO_T *glue, UINT_32 offset, PUINT_32 value)
{
	if (!value || (offset&3) || offset>=0x5c || !available(glue)) { if(value) *value=0; return FALSE; }
	*value=readl(glue->rHifInfo.HifRegBaseAddr+offset); return TRUE;
}
BOOL kalDevRegWrite(GLUE_INFO_T *glue, UINT_32 offset, UINT_32 value)
{
	if ((offset&3) || offset>=0x5c || !available(glue)) return FALSE;
	writel(value,glue->rHifInfo.HifRegBaseAddr+offset); return TRUE;
}
static bool port_config(GLUE_INFO_T *glue, unsigned port, unsigned size, bool tx)
{
	unsigned target; void __iomem *base=glue->rHifInfo.HifRegBaseAddr;
	if (!size || size>0x3fffc || !available(glue)) return false;
	if (tx) {
		if (port==MCR_WTDR0) target=0; else if (port==MCR_WTDR1) target=1; else return false;
	} else {
		if (port==MCR_WRDR0) target=2; else if (port==MCR_WRDR1) target=3;
		else if (port==MCR_WHISR) target=4; else return false;
	}
	/* Stock HIF 92-byte/4-byte boundary workaround: access non-func0 first. */
	readl(base+MCR_WHIER); readl(base+MCR_HSTCR);
	writel((target<<20)|ALIGN(size,4),base+MCR_HSTCR);
	return true;
}
BOOL kalDevPortRead(GLUE_INFO_T *glue, UINT_16 port, UINT_16 size, PUINT_8 buf, UINT_16 capacity)
{
	if (!buf || size>capacity || !port_config(glue,port,size,false)) return FALSE;
	for (unsigned i=0;i<size;i+=4) {
		u32 value=readl(glue->rHifInfo.HifRegBaseAddr+port);
		unsigned char word[4]; put_unaligned_le32(value,word);
		memcpy(buf+i,word,min_t(unsigned,4,size-i));
	}
	return TRUE;
}
BOOL kalDevPortWrite(GLUE_INFO_T *glue, UINT_16 port, UINT_16 size, PUINT_8 buf, UINT_16 capacity)
{
	if (!buf || size>capacity || !port_config(glue,port,size,true)) return FALSE;
	for (unsigned i=0;i<size;i+=4) {
		unsigned char word[4]={0}; memcpy(word,buf+i,min_t(unsigned,4,size-i));
		writel(get_unaligned_le32(word),glue->rHifInfo.HifRegBaseAddr+port);
	}
	return TRUE;
}
BOOL kalDevWriteWithSdioCmd52(GLUE_INFO_T *glue, UINT_32 offset, UINT_8 value)
{
	if (offset>=0x5c || !available(glue)) return FALSE;
	writeb(value,glue->rHifInfo.HifRegBaseAddr+offset); return TRUE;
}
const unsigned char *y2_wifi_factory(P_GLUE_INFO_T glue) { return glue->rHifInfo.bus->conn->wifi_factory; }
void y2_wifi_error(P_GLUE_INFO_T glue) { y2_conn_failed(glue->rHifInfo.bus->conn,-EIO); }
void glSendResetRequest(void) { if (single) y2_conn_failed(single->conn,-EIO); }
BOOLEAN kalIsResetting(void) { return single && (single->conn->recovering || single->conn->failure); }
