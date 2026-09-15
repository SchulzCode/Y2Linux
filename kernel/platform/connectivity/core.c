// SPDX-License-Identifier: GPL-2.0-only
/* One CONSYS owner. Framework suppliers retain their own MMIO and locks.
 * The only bootstrap control is root-only factory handoff/activation; radio
 * settings are normal cfg80211/rfkill and BlueZ/HCI operations. */
#include <linux/etherdevice.h>
#include <linux/iopoll.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/of_address.h>
#include <linux/of_platform.h>
#include <linux/pm_runtime.h>
#include <linux/pm_wakeup.h>
#include <net/bluetooth/bluetooth.h>
#include <net/bluetooth/hci_core.h>
#include "conn.h"
#include "../boot.h"
#include "../spm.h"

int y2_conn_rail(struct y2_conn *c, unsigned rail, bool enable)
{
	static const unsigned reg[] = {0,0,0x416,0x418};
	static const unsigned hw[] = {0,0,BIT(5),BIT(14)};
	int ret = 0;
	if (rail >= ARRAY_SIZE(c->rails)) return -EINVAL;
	if (enable && !c->rail_on[rail]) {
		ret = regulator_enable(c->rails[rail].consumer);
		if (ret) return ret;
		c->rail_on[rail] = true;
	}
	if (hw[rail]) ret = regmap_update_bits(c->pmic,reg[rail],hw[rail],enable ? hw[rail] : 0);
	if (!enable && !ret && c->rail_on[rail]) {
		ret = regulator_disable(c->rails[rail].consumer);
		if (!ret) c->rail_on[rail] = false;
	}
	return ret;
}
static int pmic_prepare(struct y2_conn *c)
{
	/* Exact PMIC_INIT_SETTING_V1 connectivity subset from retained source.
	 * Establish SRCLKEN/OSC hardware policy once, only in a normal boot. */
	static const unsigned fields[][3] = {
		{0x102,BIT(6),BIT(6)}, {0x102,BIT(11),0}, {0x102,BIT(15),BIT(15)},
		{0x120,BIT(4),BIT(4)}, {0x120,BIT(5),BIT(5)},
		{0x148,BIT(1),BIT(1)}, {0x148,BIT(3),BIT(3)},
		{0x402,BIT(0),BIT(0)}, {0x402,BIT(11),0},
	};
	int ret;
	if (c->pmic_ready) return 0;
	for (unsigned i=0;i<ARRAY_SIZE(fields);i++) {
		ret=regmap_update_bits(c->pmic,fields[i][0],fields[i][1],fields[i][2]);
		if (ret) return ret;
	}
	msleep(5000); c->pmic_ready = true;
	return 0;
}
static int power_off(struct y2_conn *c)
{
	int ret, result, stop_error;
	if (!c->powered && !c->clock_on && !c->dma_active &&
	    !c->rail_on[0] && !c->rail_on[1] && !c->rail_on[2] && !c->rail_on[3]) return 0;
	stop_error = y2_btif_stop(c); ret=0;
	y2_stp_reset(c);
	/* A DMA STOP timeout retains buffers/clocks, but isolating the remote
	 * producer is still safe. Never free its DMA buffers on this path. */
	result = reset_control_assert(c->reset);
	if (result) ret = result;
	result = y2_spm_radio_power(0,0);
	if (result) return result;
	c->powered = false;
	if (c->dma_active) {
		result=y2_btif_abort(c);
		if (result) return result;
		/* Confirmed local reset resolves only the earlier DMA STOP error. */
		stop_error=0;
	}
	if (!ret) ret=stop_error;
	if (c->clock_on) { clk_disable_unprepare(c->clocks[0].clk); c->clock_on=false; }
	for (int i=3;i>=0;i--) { result=y2_conn_rail(c,i,false); if (!ret) ret=result; }
	return ret;
}
static int power_on(struct y2_conn *c)
{
	unsigned id; int ret;
	if (!y2_normal_boot_enabled() || !c->factory_ready || c->removing) return -EHOSTDOWN;
	ret = pmic_prepare(c);
	if (!ret && !c->calibrated) {
		ret = y2_md_calibrate(c);
		if (!ret) c->calibrated = true;
	}
	if (ret) return ret;
	if (READ_ONCE(c->removing)) return -ESHUTDOWN;
	/* Keep the MCU reset until its memory/remap and DMA receiver are ready. */
	ret = reset_control_assert(c->reset);
	if (!ret) ret = y2_spm_radio_power(0,0);
	if (!ret) ret = regmap_update_bits(c->pmic,0x512,BIT(1),0);
	if (!ret) ret = regmap_update_bits(c->pmic,0x41c,BIT(14),0); /* co-clock: VCN28 stays off */
	if (!ret) ret = y2_conn_rail(c,0,true);
	if (ret) goto fail;
	usleep_range(150,250);
	memset_io(c->emi,0,0x100000);
	ret = y2_ccf_radio_remap(0);
	if (!ret) ret = y2_spm_radio_power(0,1);
	if (ret) goto fail;
	c->powered = true;
	ret = clk_prepare_enable(c->clocks[0].clk);
	if (ret) goto fail;
	c->clock_on = true;
	ret = readl_poll_timeout(c->mcu+8,id,(id & 0xffff)==0x6582,1000,200000);
	if (!ret) ret = y2_btif_start(c);
	if (!ret) ret = reset_control_deassert(c->reset);
	if (ret) goto fail;
	msleep(5);
	WRITE_ONCE(c->failure,0);
	ret = y2_wmt_boot(c);
	if (!ret) return 0;
fail:
	dev_err(c->dev,"connectivity start failed: %d\n",ret);
	int stopped = power_off(c);
	return stopped ? stopped : ret;
}
static int runtime_resume(struct device *dev)
{
	struct y2_conn *c=dev_get_drvdata(dev); int ret;
	mutex_lock(&c->lifecycle);
	ret=power_on(c);
	if (ret) WRITE_ONCE(c->failure,ret);
	mutex_unlock(&c->lifecycle);
	return ret;
}
static int runtime_suspend(struct device *dev)
{
	struct y2_conn *c=dev_get_drvdata(dev); int ret;
	mutex_lock(&c->lifecycle);
	ret=c->functions ? -EBUSY : power_off(c);
	mutex_unlock(&c->lifecycle);
	return ret;
}
int y2_conn_get(struct y2_conn *c, unsigned function)
{
	int ret;
	if (function!=Y2_CONN_BT && function!=Y2_CONN_WIFI) return -EINVAL;
	if (!READ_ONCE(c->activated) || READ_ONCE(c->recovering) || c->removing) return -EHOSTDOWN;
	ret=pm_runtime_resume_and_get(c->dev);
	if (ret<0) { y2_conn_failed(c,ret); return ret; }
	mutex_lock(&c->lifecycle);
	/* Recovery may have isolated the controller while an old HCI reference
	 * still kept runtime PM active. Re-open must boot hardware in that case. */
	if (!c->powered && !c->failure) {
		ret=power_on(c);
		if (ret) { WRITE_ONCE(c->failure,ret); goto unused; }
	}
	if (c->functions & BIT(function)) { ret=0; goto unused; }
	ret=c->failure ? c->failure : y2_conn_rail(c,function==Y2_CONN_BT ? 2 : 3,true);
	if (!ret) ret=y2_wmt_function(c,function,true);
	if (ret) { y2_conn_rail(c,function==Y2_CONN_BT ? 2 : 3,false); goto unused; }
	c->functions |= BIT(function); c->wanted |= BIT(function);
	mutex_unlock(&c->lifecycle);
	return 0;
unused:
	mutex_unlock(&c->lifecycle); pm_runtime_put(c->dev);
	return ret;
}
int y2_conn_put(struct y2_conn *c, unsigned function)
{
	int ret=0, rail;
	if (function!=Y2_CONN_BT && function!=Y2_CONN_WIFI) return -EINVAL;
	mutex_lock(&c->lifecycle);
	c->wanted &= ~BIT(function);
	if (!(c->functions & BIT(function))) { mutex_unlock(&c->lifecycle); return 0; }
	if (!c->failure && c->powered) ret=y2_wmt_function(c,function,false);
	rail=y2_conn_rail(c,function==Y2_CONN_BT ? 2 : 3,false);
	if (!ret) ret=rail;
	c->functions &= ~BIT(function);
	mutex_unlock(&c->lifecycle);
	pm_runtime_mark_last_busy(c->dev); pm_runtime_put_autosuspend(c->dev);
	if (ret) y2_conn_failed(c,ret);
	return ret;
}
static int recover_locked(struct y2_conn *c)
{
	int ret;
	mutex_lock(&c->lifecycle);
	if (!c->activated || c->removing || c->recovering) { mutex_unlock(&c->lifecycle); return -EHOSTDOWN; }
	if (time_after(jiffies,c->last_recovery+msecs_to_jiffies(300000))) c->automatic_recoveries=0;
	if (c->automatic_recoveries++ >= 3) { mutex_unlock(&c->lifecycle); return -EIO; }
	c->last_recovery=jiffies; c->recovering=true;
	WRITE_ONCE(c->hci_paused,true);
	mutex_unlock(&c->lifecycle);
	/* Quiesce consumers outside the lifecycle lock: their remove/close
	 * callbacks release function references through this same owner. */
	if (c->wifi) device_release_driver(&c->wifi->dev);
	if (c->hdev) cancel_work_sync(&c->hci_work);
	pm_runtime_disable(c->dev);
	mutex_lock(&c->lifecycle);
	ret=power_off(c);
	if (!ret && c->md_owned) {
		ret=y2_spm_radio_power(1,0);
		if (!ret) c->md_owned=false;
	}
	if (!ret) { WRITE_ONCE(c->failure,0); c->recoveries++; }
	/* Clear runtime PM's latched callback error after hardware is off.
	 * Open consumers retain their counted references until HCI closes. */
	if (!ret) pm_runtime_set_suspended(c->dev);
	pm_runtime_enable(c->dev);
	c->recovering=false;
	mutex_unlock(&c->lifecycle);
	if (!ret && c->wifi) {
		int attached=device_attach(&c->wifi->dev);
		if (attached<0) dev_warn(c->dev,"Wi-Fi recovery rebind: %d\n",attached);
	}
	return ret;
}
static void recover(struct work_struct *work)
{
	struct y2_conn *c=container_of(work,struct y2_conn,recovery_work);
	mutex_lock(&c->recovery_lock);
	if (!recover_locked(c) && c->hdev && c->hci_open) {
		c->hci_recovery_pending=true;
		if (hci_reset_dev(c->hdev)) c->hci_recovery_pending=false;
	}
	mutex_unlock(&c->recovery_lock);
}
void y2_conn_hci_error(struct y2_conn *c)
{
	/* HCI's hardware-error worker closes/reopens after this callback returns.
	 * Complete shared recovery synchronously before allowing that reopen. */
	mutex_lock(&c->recovery_lock);
	if (c->hci_recovery_pending) c->hci_recovery_pending=false;
	else {
		WRITE_ONCE(c->failure,-EIO); complete_all(&c->response); wake_up_all(&c->tx_wait);
		recover_locked(c);
	}
	mutex_unlock(&c->recovery_lock);
}
void y2_conn_failed(struct y2_conn *c, int error)
{
	if (error>=0) error=-EIO;
	if (!cmpxchg(&c->failure,0,error)) c->transport_errors++;
	complete_all(&c->response); wake_up_all(&c->tx_wait);
	if (READ_ONCE(c->activated) && !READ_ONCE(c->removing) && !READ_ONCE(c->recovering))
		schedule_work(&c->recovery_work);
}
static ssize_t factory_write(struct file *file, struct kobject *kobj, const struct bin_attribute *attr,
			    char *data, loff_t offset, size_t size)
{
	struct y2_conn *c=dev_get_drvdata(kobj_to_dev(kobj)); int ret=size;
	if (offset || size!=576 || !y2_normal_boot_enabled()) return -EINVAL;
	mutex_lock(&c->lifecycle);
	if (c->activated) ret=-EBUSY;
	else if (y2_conn_le16(data)!=0x104 || data[196]!=1 || data[197] ||
		 !is_valid_ether_addr(data+4) || !is_valid_ether_addr(data+512) ||
		 !memcmp(data+4,data+512,6)) ret=-EINVAL;
	else {
		memcpy(c->wifi_factory,data,512); memcpy(c->bt_factory,data+512,64); c->factory_ready=true;
	}
	mutex_unlock(&c->lifecycle);
	return ret;
}
static const struct bin_attribute bin_attr_factory = {
	.attr={.name="factory",.mode=0200}, .size=576, .write=factory_write,
};
static ssize_t activate_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_conn *c=dev_get_drvdata(dev); bool value; int ret;
	if (kstrtobool(buf,&value) || !value || !y2_normal_boot_enabled()) return -EINVAL;
	mutex_lock(&c->lifecycle);
	if (!c->factory_ready) { mutex_unlock(&c->lifecycle); return -ENODATA; }
	if (c->activated) { mutex_unlock(&c->lifecycle); return n; }
	c->activated=true; mutex_unlock(&c->lifecycle);
	ret=y2_hci_register(c);
	if (ret) { c->activated=false; return ret; }
	struct platform_device_info info={.parent=dev,.name="y2-mt6582-wifi",.id=PLATFORM_DEVID_NONE,
		.data=&c,.size_data=sizeof(c)};
	c->wifi=platform_device_register_full(&info);
	if (IS_ERR(c->wifi)) {
		ret=PTR_ERR(c->wifi); c->wifi=NULL;
		y2_hci_unregister(c); c->activated=false; return ret;
	}
	return n;
}
static ssize_t status_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_conn *c=dev_get_drvdata(dev);
	return sysfs_emit(buf,"activated=%u powered=%u functions=%#x calibrated=%u chip=%04x hvr=%04x fvr=%04x error=%d transport_errors=%u recoveries=%u\n",
		c->activated,c->powered,c->functions,c->calibrated,c->chip,c->hvr,c->fvr,
		c->failure,c->transport_errors,c->recoveries);
}
static DEVICE_ATTR_WO(activate);
static DEVICE_ATTR_RO(status);
static ssize_t recover_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_conn *c=dev_get_drvdata(dev); bool value;
	if (kstrtobool(buf,&value) || !value || !y2_normal_boot_enabled()) return -EINVAL;
	mutex_lock(&c->lifecycle);
	if (c->functions || c->recovering || !c->activated || c->removing) {
		mutex_unlock(&c->lifecycle); return -EBUSY;
	}
	c->automatic_recoveries=0;
	mutex_unlock(&c->lifecycle);
	y2_conn_failed(c,-EIO);
	return n;
}
static DEVICE_ATTR_WO(recover);
static struct attribute *attrs[]={&dev_attr_activate.attr,&dev_attr_status.attr,&dev_attr_recover.attr,NULL};
static const struct bin_attribute *const bin_attrs[]={&bin_attr_factory,NULL};
static const struct attribute_group group={.attrs=attrs,.bin_attrs=bin_attrs};
static int suspend(struct device *dev)
{
	struct y2_conn *c=dev_get_drvdata(dev);
	/* Production full-suspend policy is radio-off, restored by y2-suspend.
	 * Refuse an uncoordinated deep suspend with active radios/audio/transfers. */
	if (READ_ONCE(c->functions) || READ_ONCE(c->recovering) || READ_ONCE(c->md_owned)) return -EBUSY;
	return pm_runtime_force_suspend(dev);
}
static const struct dev_pm_ops pm={
	SET_RUNTIME_PM_OPS(runtime_suspend,runtime_resume,NULL)
	.suspend=suspend,.resume=pm_runtime_force_resume,
	.freeze=suspend,.thaw=pm_runtime_force_resume,
	.poweroff=suspend,.restore=pm_runtime_force_resume,
};
/* This board has one fixed wiring/layout. Reject a malformed DT before any
 * DMA or modem copy can touch memory outside the established exclusion. */
static int validate_resources(struct platform_device *pdev)
{
	static const struct { const char *name; u32 start, size; } ranges[] = {
		{"mcu",0x18070000,0x1000}, {"conn-emi",0xbdf00000,0x100000},
		{"btif",0x1100c000,0x100}, {"tx-dma",0x11000780,0x80}, {"rx-dma",0x11000800,0x80},
		{"ccif",0x1020a000,0x200}, {"md-rom",0xbe000000,0x1600000}, {"md-smem",0xbf600000,0x1c4000},
		{"md-wdt",0x20050000,4}, {"md-key",0x2019379c,4}, {"md-vector",0x20190000,4},
		{"md-enable",0x20195488,4}, {"wifi",0x180f0000,0x5c},
	};
	struct device_node *node; struct resource memory; int ret;
	for (unsigned i=0;i<ARRAY_SIZE(ranges);i++) {
		struct resource *r=platform_get_resource_byname(pdev,IORESOURCE_MEM,ranges[i].name);
		if (!r || r->start!=ranges[i].start || resource_size(r)!=ranges[i].size) return -EINVAL;
	}
	node=of_parse_phandle(pdev->dev.of_node,"memory-region",0);
	if (!node) return -EINVAL;
	ret=of_address_to_resource(node,0,&memory);
	if (!of_property_read_bool(node,"no-map")) ret=-EINVAL;
	of_node_put(node);
	if (ret || memory.start!=0xbdf00000 || resource_size(&memory)!=0x2100000) return -EINVAL;
	return 0;
}
static int probe(struct platform_device *pdev)
{
	struct device *dev=&pdev->dev;
	struct y2_conn *c=devm_kzalloc(dev,sizeof(*c),GFP_KERNEL);
	struct device_node *node; struct platform_device *supplier; int ret;
	if (!c) return -ENOMEM;
	ret=validate_resources(pdev); if (ret) return ret;
	c->dev=dev; mutex_init(&c->lifecycle); mutex_init(&c->command);
	mutex_init(&c->recovery_lock);
	mutex_init(&c->dma_tx); mutex_init(&c->dma_rx); init_completion(&c->response);
	y2_stp_init(c); INIT_WORK(&c->recovery_work,recover);
	c->clocks[0].id="connmcu"; c->clocks[1].id="btif"; c->clocks[2].id="dma";
	ret=devm_clk_bulk_get(dev,3,c->clocks); if (ret) return ret;
	c->rails[0].supply="vcn18"; c->rails[1].supply="vcn28";
	c->rails[2].supply="vcn33-bt"; c->rails[3].supply="vcn33-wifi";
	ret=devm_regulator_bulk_get(dev,4,c->rails); if (ret) return ret;
	c->reset=devm_reset_control_get_exclusive(dev,"conn"); if (IS_ERR(c->reset)) return PTR_ERR(c->reset);
	node=of_parse_phandle(dev->of_node,"innioasis,pwrap",0);
	if (!node) return -EINVAL;
	supplier=of_find_device_by_node(node); of_node_put(node);
	if (!supplier) return -EPROBE_DEFER;
	c->pmic=dev_get_regmap(&supplier->dev,NULL);
	put_device(&supplier->dev); if (!c->pmic) return -EPROBE_DEFER;
	c->mcu=devm_platform_ioremap_resource_byname(pdev,"mcu");
	c->emi=devm_platform_ioremap_resource_byname(pdev,"conn-emi");
	if (IS_ERR(c->mcu)) return PTR_ERR(c->mcu);
	if (IS_ERR(c->emi)) return PTR_ERR(c->emi);
	ret=y2_btif_probe(pdev,c); if (ret) return ret;
	ret=y2_md_probe(pdev,c); if (ret) return ret;
	platform_set_drvdata(pdev,c);
	pm_runtime_set_suspended(dev); pm_runtime_set_autosuspend_delay(dev,1000);
	/* Both consumer interfaces use this owner's explicit function refs. */
	pm_suspend_ignore_children(dev,true);
	pm_runtime_use_autosuspend(dev); pm_runtime_enable(dev);
	ret=devm_device_add_group(dev,&group);
	if (ret) { pm_runtime_disable(dev); y2_md_unregister(c); return ret; }
	dev_info(dev,"MT6582 connectivity owner ready; radios await normal-boot factory provider\n");
	return 0;
}
static void shutdown(struct platform_device *pdev)
{
	struct y2_conn *c=platform_get_drvdata(pdev);
	c->removing=true; y2_md_abort(c); cancel_work_sync(&c->recovery_work);
	if (c->wifi) { platform_device_unregister(c->wifi); c->wifi=NULL; }
	y2_hci_unregister(c); pm_runtime_disable(c->dev);
	mutex_lock(&c->lifecycle); power_off(c); mutex_unlock(&c->lifecycle);
}
static const struct of_device_id match[]={ {.compatible="innioasis,y2-mt6582-connectivity"}, {} };
static struct platform_driver y2_conn_driver={ .probe=probe,.shutdown=shutdown,
	.driver={.name="y2-connectivity",.of_match_table=match,.pm=&pm,.suppress_bind_attrs=true} };
builtin_platform_driver(y2_conn_driver);
