// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 automatic idle hierarchy, Linux tick broadcast/context ownership. */
#include <linux/cpuidle.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/atomic.h>
#include <linux/ktime.h>
#include <asm/cpuidle.h>
#include <asm/proc-fns.h>
#include "clocks.h"
#include "spm.h"
#include "cpu-options.h"
static unsigned long slow_entries, slow_aborts, slow_failures;
static bool slow_broken;
static atomic_t dormant_aborts = ATOMIC_INIT(0);
static int last_error;
module_param(slow_entries, ulong, 0400);
module_param(slow_aborts, ulong, 0400);
module_param(slow_failures, ulong, 0400);
module_param(slow_broken, bool, 0400);
module_param(last_error, int, 0400);
static int y2_enter_slow_idle(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	int ret = READ_ONCE(slow_broken) || y2_idle_disabled() ? -EIO : y2_ccf_slow_idle();
	/* Stock SLIDLE is CPU0 only, so these counters have a single writer. */
	if (!ret) { slow_entries++; return index; }
	if (!smp_processor_id()) {
		slow_aborts++;
		if (ret == -EIO) { slow_failures++; WRITE_ONCE(slow_broken, true); }
	}
	WRITE_ONCE(last_error, ret);
	cpu_do_idle();
	return 0;
}
static int y2_enter_dormant(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	int ret;
	/* Stock minimum 26000 ticks at 13 MHz; Linux provides a monotonic
	 * deadline. Do not round a near/past deadline into a long GPT sleep. */
	if (ktime_to_ns(ktime_sub(READ_ONCE(dev->next_hrtimer), ktime_get())) < 2000000)
		ret = -ETIME;
	else ret = y2_spm_dormant_idle();
	if (!ret) return index;
	atomic_inc(&dormant_aborts);
	WRITE_ONCE(last_error, ret);
	return y2_enter_slow_idle(dev, drv, 1);
}
static int y2_enter_idle(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	cpu_do_idle();
	return index;
}
static struct cpuidle_driver y2_idle_driver = {
	.name = "mt6582-idle", .owner = THIS_MODULE,
	.states = { ARM_CPUIDLE_WFI_STATE, {
		.name = "SLIDLE", .desc = "Bus DCM WFI",
		.exit_latency = 50, .target_residency = 1000,
		.enter = y2_enter_slow_idle,
	}, {
		.name = "DORMANT", .desc = "CPU0 off, L2/infra retained",
		.flags = CPUIDLE_FLAG_TIMER_STOP | CPUIDLE_FLAG_OFF,
		.exit_latency = 500, .target_residency = 5000,
		.enter = y2_enter_dormant,
	} },
	.state_count = 3,
};
static int __init y2_idle_init(void)
{
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	if (y2_idle_disabled()) y2_idle_driver.state_count = 1;
	else if (y2_deep_idle_disabled()) y2_idle_driver.state_count = 2;
	y2_idle_driver.states[0].enter = y2_enter_idle;
	y2_idle_driver.states[0].enter_s2idle = y2_enter_idle;
	return cpuidle_register(&y2_idle_driver, NULL);
}
device_initcall(y2_idle_init);
MODULE_LICENSE("GPL");
