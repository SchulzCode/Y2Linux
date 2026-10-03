// SPDX-License-Identifier: GPL-2.0-only
/* Per-open bounded QoS leases. Linux cpufreq/thermal retain final authority. */
#include <linux/cpufreq.h>
#include "system-idle.h"
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/atomic.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/pm_qos.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/workqueue.h>
#include <linux/list.h>
#include <linux/jiffies.h>

static LIST_HEAD(leases);
static DEFINE_MUTEX(leases_lock);
static atomic_t idle_blocking_leases = ATOMIC_INIT(0);
struct y2_workload {
	struct list_head node;
	char name[32];
	unsigned long deadline;
	struct cpufreq_policy *policy;
	struct freq_qos_request minimum, maximum;
	struct pm_qos_request latency;
	struct delayed_work expiry;
	struct mutex lock;
	bool idle_blocking;
};

/* IRQ-disabled idle entry cannot walk the mutex-protected lease list.
 * Writers/expiry hold hint->lock; an expired pending worker stays conservative. */
static void y2_workload_idle_blocker(struct y2_workload *hint, bool active)
{
	if (hint->idle_blocking == active) return;
	if (active) atomic_inc(&idle_blocking_leases);
	else atomic_dec(&idle_blocking_leases);
	hint->idle_blocking = active;
}
bool y2_workload_idle_blocked(void) { return atomic_read(&idle_blocking_leases) != 0; }

static void y2_workload_idle(struct y2_workload *hint)
{
	y2_workload_idle_blocker(hint, false);
	strscpy(hint->name, "Idle", sizeof(hint->name));
	hint->deadline = 0;
	freq_qos_update_request(&hint->minimum, 0);
	cpu_latency_qos_update_request(&hint->latency, PM_QOS_DEFAULT_VALUE);
}

static void y2_workload_expire(struct work_struct *work)
{
	struct y2_workload *hint = container_of(to_delayed_work(work), struct y2_workload, expiry);
	mutex_lock(&hint->lock);
	if (hint->deadline && time_before(jiffies, hint->deadline))
		mod_delayed_work(system_wq, &hint->expiry, hint->deadline - jiffies);
	else y2_workload_idle(hint);
	mutex_unlock(&hint->lock);
}

static int y2_workload_open(struct inode *inode, struct file *file)
{
	struct y2_workload *hint;
	int ret;
	hint = kzalloc(sizeof(*hint), GFP_KERNEL);
	if (!hint) return -ENOMEM;
	hint->policy = cpufreq_cpu_get(0);
	if (!hint->policy) { ret = -EAGAIN; goto free; }
	ret = freq_qos_add_request(&hint->policy->constraints, &hint->minimum, FREQ_QOS_MIN, 0);
	if (ret < 0) goto put;
	ret = freq_qos_add_request(&hint->policy->constraints, &hint->maximum, FREQ_QOS_MAX, FREQ_QOS_MAX_DEFAULT_VALUE);
	if (ret < 0) goto minimum;
	mutex_init(&hint->lock);
	INIT_DELAYED_WORK(&hint->expiry, y2_workload_expire);
	cpu_latency_qos_add_request(&hint->latency, PM_QOS_DEFAULT_VALUE);
	strscpy(hint->name, "Idle", sizeof(hint->name));
	mutex_lock(&leases_lock);
	list_add_tail(&hint->node, &leases);
	mutex_unlock(&leases_lock);
	file->private_data = hint;
	return nonseekable_open(inode, file);
minimum:
	freq_qos_remove_request(&hint->minimum);
put:
	cpufreq_cpu_put(hint->policy);
free:
	kfree(hint);
	return ret;
}

static ssize_t y2_workload_write(struct file *file, const char __user *buffer,
			       size_t bytes, loff_t *offset)
{
	struct y2_workload *hint = file->private_data;
	char input[64], name[32], extra;
	unsigned ms, floor = 0;
	int latency = PM_QOS_DEFAULT_VALUE, ret;
	if (!bytes || bytes >= sizeof(input)) return -EINVAL;
	if (copy_from_user(input, buffer, bytes)) return -EFAULT;
	input[bytes] = 0;
	if (sscanf(input, "%31s %u %c", name, &ms, &extra) != 2 || !ms || ms > 30000)
		return -EINVAL;
	if (!strcmp(name, "Idle") || !strcmp(name, "PlaybackNormal")) {
		/* No unmeasured playback floor. schedutil follows actual demand. */
	} else if (!strcmp(name, "Interactive") || !strcmp(name, "ArtworkDecode")) {
		if (!strcmp(name, "Interactive") && ms > 500) return -EINVAL;
		floor = 747500; latency = 1000;
	} else if (!strcmp(name, "PlaybackHeavy")) {
		floor = 747500; latency = 1000;
	} else if (!strcmp(name, "LibraryScan")) {
		floor = 747500;
	} else if (!strcmp(name, "NetworkTransfer") || !strcmp(name, "Maintenance")) {
		floor = 598000;
	} else return -EINVAL;
	/* Restore available cores before acknowledging a real workload lease.
	 * Never hold hint/QoS locks across the hotplug transition. */
	if (strcmp(name, "Idle")) {
		ret = y2_system_idle_restore();
		if (ret) return ret;
	}
	mutex_lock(&hint->lock);
	ret = freq_qos_update_request(&hint->minimum, floor);
	if (ret >= 0) {
		y2_workload_idle_blocker(hint, strcmp(name, "Idle") != 0);
		strscpy(hint->name, name, sizeof(hint->name));
		hint->deadline = jiffies + msecs_to_jiffies(ms);
		cpu_latency_qos_update_request(&hint->latency, latency);
		mod_delayed_work(system_wq, &hint->expiry, msecs_to_jiffies(ms));
	}
	mutex_unlock(&hint->lock);
	return ret < 0 ? ret : bytes;
}

static int y2_workload_release(struct inode *inode, struct file *file)
{
	struct y2_workload *hint = file->private_data;
	mutex_lock(&leases_lock);
	list_del(&hint->node);
	mutex_unlock(&leases_lock);
	cancel_delayed_work_sync(&hint->expiry);
	y2_workload_idle_blocker(hint, false);
	cpu_latency_qos_remove_request(&hint->latency);
	freq_qos_remove_request(&hint->minimum);
	freq_qos_remove_request(&hint->maximum);
	cpufreq_cpu_put(hint->policy);
	kfree(hint);
	return 0;
}

bool y2_workload_active(void)
{
	struct y2_workload *hint; bool active = false;
	mutex_lock(&leases_lock);
	list_for_each_entry(hint, &leases, node) {
		mutex_lock(&hint->lock);
		active = hint->deadline && time_before(jiffies, hint->deadline) && strcmp(hint->name, "Idle");
		mutex_unlock(&hint->lock);
		if (active) break;
	}
	mutex_unlock(&leases_lock);
	return active;
}
static int leases_get(char *buf, const struct kernel_param *kp)
{
	struct y2_workload *hint;
	int used = 0;
	mutex_lock(&leases_lock);
	list_for_each_entry(hint, &leases, node) {
		if (used > PAGE_SIZE - 128) break;
		mutex_lock(&hint->lock);
		used += scnprintf(buf + used, PAGE_SIZE - used, "%s remaining_ms=%u min_khz=%d\n",
			hint->name, hint->deadline && time_before(jiffies, hint->deadline) ?
			jiffies_to_msecs(hint->deadline - jiffies) : 0, hint->minimum.pnode.prio);
		mutex_unlock(&hint->lock);
	}
	mutex_unlock(&leases_lock);
	return used;
}
static const struct kernel_param_ops leases_ops = { .get = leases_get };
module_param_cb(leases, &leases_ops, NULL, 0400);
static const struct file_operations y2_workload_ops = {
	.owner = THIS_MODULE, .open = y2_workload_open, .write = y2_workload_write,
	.release = y2_workload_release,
};
static struct miscdevice y2_workload_device = {
	.minor = MISC_DYNAMIC_MINOR, .name = "y2-workload", .mode = 0600,
	.fops = &y2_workload_ops,
};
static int __init y2_workload_init(void)
{
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	return misc_register(&y2_workload_device);
}
device_initcall(y2_workload_init);
MODULE_LICENSE("GPL");
