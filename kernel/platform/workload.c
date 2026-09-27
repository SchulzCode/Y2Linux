// SPDX-License-Identifier: GPL-2.0-only
/* Per-open bounded QoS leases. Linux cpufreq/thermal retain final authority. */
#include <linux/cpufreq.h>
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/pm_qos.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/workqueue.h>

struct y2_workload {
	struct cpufreq_policy *policy;
	struct freq_qos_request minimum, maximum;
	struct pm_qos_request latency;
	struct delayed_work expiry;
	struct mutex lock;
};

static void y2_workload_idle(struct y2_workload *hint)
{
	freq_qos_update_request(&hint->minimum, 0);
	cpu_latency_qos_update_request(&hint->latency, PM_QOS_DEFAULT_VALUE);
}

static void y2_workload_expire(struct work_struct *work)
{
	struct y2_workload *hint = container_of(to_delayed_work(work), struct y2_workload, expiry);
	mutex_lock(&hint->lock);
	y2_workload_idle(hint);
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
	ret = freq_qos_add_request(&hint->policy->constraints, &hint->maximum, FREQ_QOS_MAX, 1040000);
	if (ret < 0) goto minimum;
	mutex_init(&hint->lock);
	INIT_DELAYED_WORK(&hint->expiry, y2_workload_expire);
	cpu_latency_qos_add_request(&hint->latency, PM_QOS_DEFAULT_VALUE);
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
	} else if (!strcmp(name, "Interactive")) {
		if (ms > 500) return -EINVAL;
		floor = 747500; latency = 1000;
	} else if (!strcmp(name, "PlaybackHeavy")) {
		floor = 747500; latency = 1000;
	} else if (!strcmp(name, "LibraryScan")) {
		floor = 747500;
	} else if (!strcmp(name, "NetworkTransfer") || !strcmp(name, "Maintenance")) {
		floor = 598000;
	} else return -EINVAL;
	mutex_lock(&hint->lock);
	ret = freq_qos_update_request(&hint->minimum, floor);
	if (ret >= 0) {
		cpu_latency_qos_update_request(&hint->latency, latency);
		mod_delayed_work(system_wq, &hint->expiry, msecs_to_jiffies(ms));
	}
	mutex_unlock(&hint->lock);
	return ret < 0 ? ret : bytes;
}

static int y2_workload_release(struct inode *inode, struct file *file)
{
	struct y2_workload *hint = file->private_data;
	cancel_delayed_work_sync(&hint->expiry);
	cpu_latency_qos_remove_request(&hint->latency);
	freq_qos_remove_request(&hint->minimum);
	freq_qos_remove_request(&hint->maximum);
	cpufreq_cpu_put(hint->policy);
	kfree(hint);
	return 0;
}

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
