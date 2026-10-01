// SPDX-License-Identifier: GPL-2.0-only
/* Stock OPP qualification admission; no automatic high-frequency promotion. */
#include <linux/cpufreq.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/workqueue.h>
#include "cpu-dvfs.h"
#include "cpu-dvfs-policy.h"
#include "cpu-options.h"

static DEFINE_MUTEX(qualification_lock);
static struct freq_qos_request ceiling;
static unsigned qualification_max_khz = 1040000;
static bool bin_supported, voltage_fault;
static int admission_error;
module_param(admission_error, int, 0400);
module_param(bin_supported, bool, 0400);
module_param(voltage_fault, bool, 0400);

static void fault_ceiling_work(struct work_struct *work)
{
	mutex_lock(&qualification_lock);
	if (freq_qos_request_active(&ceiling))
		freq_qos_update_request(&ceiling, y2_cpu_safe() ? 598000 : 1040000);
	WRITE_ONCE(qualification_max_khz, 1040000);
	mutex_unlock(&qualification_lock);
}
static DECLARE_WORK(fault_work, fault_ceiling_work);

unsigned long y2_cpu_dvfs_max(void)
{
	if (y2_cpu_safe()) return 598000000;
	if (READ_ONCE(voltage_fault)) return 1040000000;
	return READ_ONCE(qualification_max_khz) * 1000UL;
}

void y2_cpu_dvfs_fault(void)
{
	WRITE_ONCE(voltage_fault, true);
	/* Do not call cpufreq synchronously while its CCF transition is locked. */
	schedule_work(&fault_work);
	pr_err_ratelimited("Y2DVFS: uncertain transition; high OPPs refused until reboot, voltage retained\n");
}

static int qualification_set(const char *value, const struct kernel_param *kp)
{
	unsigned target, old;
	int ret = kstrtouint(value, 0, &target);
	if (ret) return ret;
	if (target != 1040000 && target != 1196000 && target != 1300000) return -EINVAL;
	mutex_lock(&qualification_lock);
	old = qualification_max_khz;
	if (!freq_qos_request_active(&ceiling)) { ret = -EAGAIN; goto out; }
	if (target > 1040000) {
		if (y2_dvfs_disabled()) { ret = -EOPNOTSUPP; goto out; }
		if (!bin_supported || READ_ONCE(voltage_fault)) { ret = -EOPNOTSUPP; goto out; }
		ret = y2_pmic_cpu_dvfs_prepare();
		if (ret) goto out;
	}
	/* Clock admission rises first, QoS falls first. The frequency core owns
	 * transitions; this control never programs a clock behind cpufreq. */
	if (target > qualification_max_khz) WRITE_ONCE(qualification_max_khz, target);
	ret = freq_qos_update_request(&ceiling, target);
	if (ret >= 0) { WRITE_ONCE(qualification_max_khz, target); ret = 0; }
	else WRITE_ONCE(qualification_max_khz, old);
out:
	mutex_unlock(&qualification_lock);
	return ret;
}
static const struct kernel_param_ops qualification_ops = {
	.set = qualification_set, .get = param_get_uint,
};
module_param_cb(qualification_max_khz, &qualification_ops, &qualification_max_khz, 0600);
MODULE_PARM_DESC(qualification_max_khz, "Stock-bin automatic ceiling; optional owner ceiling within stock OPPs");

/* Both policy creation and SPM publication can be last. A bounded asynchronous
 * admission pass avoids calling PMIC/CCF under cpufreq's transition locks. */
static void stock_admission_work(struct work_struct *work)
{
	int ret;
	mutex_lock(&qualification_lock);
	if (!bin_supported || READ_ONCE(voltage_fault) || y2_dvfs_disabled() ||
	    !freq_qos_request_active(&ceiling)) goto out;
	ret = y2_pmic_cpu_dvfs_prepare();
	if (!ret) {
		WRITE_ONCE(qualification_max_khz, 1300000);
		ret = freq_qos_update_request(&ceiling, 1300000);
		/* 1 means the constraint changed: success, not an error code. */
		if (ret < 0) WRITE_ONCE(qualification_max_khz, 1040000);
		else ret = 0;
	}
	admission_error = ret;
	pr_info("Y2DVFS: stock-bin automatic admission result=%d ceiling=%u kHz\n",
		ret, qualification_max_khz);
out:
	mutex_unlock(&qualification_lock);
}
static DECLARE_WORK(stock_work, stock_admission_work);
void y2_cpu_dvfs_ready(void) { schedule_work(&stock_work); }
static int policy_notify(struct notifier_block *nb, unsigned long event, void *data)
{
	struct cpufreq_policy *policy = data;
	int ret = 0;
	if (!cpumask_test_cpu(0, policy->related_cpus)) return NOTIFY_DONE;
	mutex_lock(&qualification_lock);
	if (event == CPUFREQ_CREATE_POLICY) {
		WRITE_ONCE(qualification_max_khz, 1040000);
		ret = freq_qos_add_request(&policy->constraints, &ceiling, FREQ_QOS_MAX,
			y2_cpu_safe() ? 598000 : 1040000);
		y2_cpu_dvfs_ready();
		if (ret < 0) y2_cpu_dvfs_fault();
	} else if (event == CPUFREQ_REMOVE_POLICY && freq_qos_request_active(&ceiling)) {
		freq_qos_remove_request(&ceiling);
		WRITE_ONCE(qualification_max_khz, 1040000);
	}
	mutex_unlock(&qualification_lock);
	return notifier_from_errno(ret < 0 ? ret : 0);
}
static struct notifier_block policy_notifier = { .notifier_call = policy_notify };
static int __init y2_cpu_dvfs_init(void)
{
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	bin_supported = y2_cpu_bin0(phys_to_virt(0x80000100), 0x3f00);
	pr_info("Y2DVFS: stock bin admission=%u; default ceiling=1040000 kHz\n", bin_supported);
	return cpufreq_register_notifier(&policy_notifier, CPUFREQ_POLICY_NOTIFIER);
}
subsys_initcall(y2_cpu_dvfs_init);
MODULE_LICENSE("GPL");
