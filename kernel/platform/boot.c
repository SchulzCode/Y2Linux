// SPDX-License-Identifier: GPL-2.0-only
/* Production boot decision transport, shared by USB and rescue userspace. */
#include <linux/capability.h>
#include <linux/firmware.h>
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kobject.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>
#include <linux/sysfs.h>
#include "boot.h"
#include "boot-policy.h"

static struct y2_boot_info boot_info;
static bool normal_boot;
static BLOCKING_NOTIFIER_HEAD(normal_boot_notifiers);
static DEFINE_MUTEX(normal_boot_transition);

int y2_boot_register_notifier(struct notifier_block *nb)
{
	return blocking_notifier_chain_register(&normal_boot_notifiers, nb);
}
void y2_boot_unregister_notifier(struct notifier_block *nb)
{
	blocking_notifier_chain_unregister(&normal_boot_notifiers, nb);
}

bool y2_normal_boot_enabled(void)
{
	return READ_ONCE(normal_boot);
}

static ssize_t metadata_show(struct kobject *kobj, struct kobj_attribute *attr, char *buf)
{
	return sysfs_emit(buf, "valid=%d boot_mode=%d boot_reason=%d offline=%d\n",
		boot_info.valid, boot_info.mode, boot_info.reason, y2_boot_offline(&boot_info));
}
static struct kobj_attribute metadata_attr = __ATTR_RO(metadata);

static ssize_t normal_boot_show(struct kobject *kobj, struct kobj_attribute *attr, char *buf)
{
	return sysfs_emit(buf, "%u\n", y2_normal_boot_enabled());
}
static ssize_t normal_boot_store(struct kobject *kobj, struct kobj_attribute *attr,
				 const char *buf, size_t count)
{
	int ret;
	if (!capable(CAP_SYS_BOOT)) return -EPERM;
	/* One-way handover: userspace may not pull the running USB stack back
	 * into offline mode. Poweroff uses the ordinary device shutdown path. */
	if (!sysfs_streq(buf, "1")) return -EINVAL;
	mutex_lock(&normal_boot_transition);
	WRITE_ONCE(normal_boot, true);
	/* Storage, wheel and audio intentionally defer before this one-way
	 * decision. Probe through the normal driver core before root discovery. */
	ret = bus_rescan_devices(&platform_bus_type);
	if (ret) pr_warn("Y2BOOT: platform rescan result=%d\n", ret);
	ret = notifier_to_errno(blocking_notifier_call_chain(&normal_boot_notifiers, 1, NULL));
	mutex_unlock(&normal_boot_transition);
	return ret ? ret : count;
}
static struct kobj_attribute normal_boot_attr = __ATTR_RW(normal_boot);
static struct attribute *y2_boot_attrs[] = { &metadata_attr.attr, &normal_boot_attr.attr, NULL };
static const struct attribute_group y2_boot_group = { .attrs = y2_boot_attrs };

static int __init y2_boot_init(void)
{
	struct kobject *kobj;
	int ret;
	/* Board DT reserves 0x80000000..0x80003fff. LK's fixed ATAG address is
	 * 0x80000100. It is ordinary retained DRAM, already in the linear map.
	 * No ATAG is written and no unbounded loader pointer is dereferenced. */
	boot_info = y2_boot_parse(phys_to_virt(0x80000100), 0x3f00);
	pr_info("Y2BOOT: loader metadata valid=%d mode=%d reason=%d offline=%d; userspace handover pending\n",
		boot_info.valid, boot_info.mode, boot_info.reason, y2_boot_offline(&boot_info));
	kobj = kobject_create_and_add("y2_boot", firmware_kobj);
	if (!kobj) return -ENOMEM;
	ret = sysfs_create_group(kobj, &y2_boot_group);
	if (ret) kobject_put(kobj);
	return ret;
}
subsys_initcall(y2_boot_init);
