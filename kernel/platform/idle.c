// SPDX-License-Identifier: GPL-2.0-only
/* Architectural Cortex-A7 WFI only. No CPU power-down, SPM or lost context.
 * cpuidle-arm requires a deeper DT state; this driver registers WFI alone. */
#include <linux/cpuidle.h>
#include <linux/module.h>
#include <linux/of.h>
#include <asm/cpuidle.h>
#include <asm/proc-fns.h>
#include <linux/atomic.h>
#include "clocks.h"

static unsigned long slow_entries, slow_aborts, slow_failures;
static bool slow_broken;
module_param(slow_entries, ulong, 0400);
module_param(slow_aborts, ulong, 0400);
module_param(slow_failures, ulong, 0400);
module_param(slow_broken, bool, 0400);
static int y2_enter_slow_idle(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	int ret = READ_ONCE(slow_broken) ? -EIO : y2_ccf_slow_idle();
	if (!ret) { slow_entries++; return index; }
	slow_aborts++;
	if (ret == -EIO) { slow_failures++; WRITE_ONCE(slow_broken, true); }
	cpu_do_idle();
	return 0;
}

static int y2_enter_idle(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	cpu_do_idle();
	return index; /* IRQ state is restored by the cpuidle core. */
}
static struct cpuidle_driver y2_idle_driver = {
	.name = "mt6582-wfi", .owner = THIS_MODULE,
	.states = { ARM_CPUIDLE_WFI_STATE, {
		.name = "SLIDLE", .desc = "Stock bus DCM WFI; experimental",
		.flags = CPUIDLE_FLAG_OFF,
		.exit_latency = 50, .target_residency = 1000,
		.enter = y2_enter_slow_idle,
	} },
	.state_count = 2,
};
static int __init y2_idle_init(void)
{
	if (!of_machine_is_compatible("innioasis,y2")) return -ENODEV;
	y2_idle_driver.states[0].enter = y2_enter_idle;
	y2_idle_driver.states[0].enter_s2idle = y2_enter_idle;
	return cpuidle_register(&y2_idle_driver, NULL);
}
device_initcall(y2_idle_init);
MODULE_LICENSE("GPL");
