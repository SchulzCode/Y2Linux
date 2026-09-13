// SPDX-License-Identifier: GPL-2.0-only
/* Architectural Cortex-A7 WFI only. No CPU power-down, SPM or lost context.
 * cpuidle-arm requires a deeper DT state; this driver registers WFI alone. */
#include <linux/cpuidle.h>
#include <linux/module.h>
#include <linux/of.h>
#include <asm/cpuidle.h>
#include <asm/proc-fns.h>

static int y2_enter_idle(struct cpuidle_device *dev, struct cpuidle_driver *drv, int index)
{
	cpu_do_idle();
	return index; /* IRQ state is restored by the cpuidle core. */
}
static struct cpuidle_driver y2_idle_driver = {
	.name = "mt6582-wfi", .owner = THIS_MODULE,
	.states = { ARM_CPUIDLE_WFI_STATE },
	.state_count = 1,
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
