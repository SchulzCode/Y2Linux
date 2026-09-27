// SPDX-License-Identifier: GPL-2.0-only
/* Slow stock-topology coordinator. schedutil and cpuidle still own policy.
 * Only coordinator-owned offlines are restored; manual topology is preserved.
 * Playback/scan/transfer leases prevent parking. Input and frequency demand
 * schedule an immediate high-priority restore, and a workload producer restores
 * synchronously before its request is acknowledged. */
#include <linux/cpu.h>
#include <linux/math64.h>
#include <linux/slab.h>
#include <linux/cpufreq.h>
#include <linux/input.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/suspend.h>
#include <linux/tick.h>
#include <linux/workqueue.h>
#include "system-idle.h"
#include "system-idle-policy.h"
#include "cpu-options.h"
#include "local-timer.h"
#include "boot.h"
static DEFINE_MUTEX(idle_lock);
static struct workqueue_struct *idle_wq;
static bool enabled = true, paused, broken;
static unsigned parked_mask, park_count, restore_count, busy_percent;
static int last_error;
static unsigned long quiet_since, hold_until, last_park;
static u64 sample_idle[4], sample_wall[4];
static atomic_t demand = ATOMIC_INIT(0);
static const struct kernel_param_ops enable_ops;
module_param_cb(enabled, &enable_ops, &enabled, 0600);
module_param(parked_mask, uint, 0400);
module_param(park_count, uint, 0400);
module_param(restore_count, uint, 0400);
module_param(busy_percent, uint, 0400);
module_param(last_error, int, 0400);
module_param(broken, bool, 0400);
static int restore_locked(void)
{
	unsigned cpu;
	int ret, first = 0;
	for (cpu = 1; cpu < 4; cpu++)
		if (parked_mask & BIT(cpu)) {
			ret = cpu_online(cpu) ? 0 : add_cpu(cpu);
			if (ret < 0) {
				if (!first)
					first = ret;
				broken = true;
			} else {
				parked_mask &= ~BIT(cpu);
				restore_count++;
			}
		}
	quiet_since = 0;
	hold_until = jiffies + msecs_to_jiffies(Y2_SYSTEM_RESTORE_HOLD_MS);
	last_error = first;
	return first;
}
int y2_system_idle_restore(void)
{
	int ret;
	if (!idle_wq)
		return 0;
	atomic_set(&demand, 1);
	mutex_lock(&idle_lock);
	ret = restore_locked();
	mutex_unlock(&idle_lock);
	return ret;
}
static void restore_work_fn(struct work_struct *work)
{
	y2_system_idle_restore();
}
static DECLARE_WORK(restore_work, restore_work_fn);
void y2_system_idle_activity(void)
{
	atomic_set(&demand, 1);
	if (idle_wq && READ_ONCE(parked_mask))
		queue_work(idle_wq, &restore_work);
}
static unsigned sample_busy(void)
{
	unsigned cpu;
	u64 total = 0, idle = 0, wall, value, delta;
	for_each_online_cpu(cpu) {
		if (cpu > 3)
			continue;
		value = get_cpu_idle_time_us(cpu, &wall);
		if (value == -1ULL)
			return 100;
		if (sample_wall[cpu] && wall > sample_wall[cpu]) {
			delta = wall - sample_wall[cpu];
			total += delta;
			idle += min(value - sample_idle[cpu], delta);
		}
		sample_idle[cpu] = value;
		sample_wall[cpu] = wall;
	}
	return total ? div64_u64((total - idle) * 100, total) : 100;
}
static void sample_work_fn(struct work_struct *work);
static DECLARE_DELAYED_WORK(sample_work, sample_work_fn);
static void sample_work_fn(struct work_struct *work)
{
	unsigned cpu, frequency;
	bool eligible;
	mutex_lock(&idle_lock);
	busy_percent = sample_busy();
	frequency = cpufreq_quick_get(0);
	eligible = enabled && !broken && !paused && y2_normal_boot_enabled() &&
		   !y2_cpu_safe() && !y2_idle_disabled() &&
		   y2_local_events_ready() && y2_backlight_dark() &&
		   !y2_workload_active() && busy_percent <= 10 && frequency &&
		   frequency <= 598000;
	if (atomic_xchg(&demand, 0) || !eligible) {
		quiet_since = 0;
		if (parked_mask)
			restore_locked();
	} else if (time_after_eq(jiffies, hold_until)) {
		if (!quiet_since)
			quiet_since = jiffies;
		if (y2_system_quiet(jiffies_to_msecs(jiffies - quiet_since),
				    busy_percent, true, false, true,
				    frequency) &&
		    time_after_eq(
			    jiffies,
			    last_park +
				    msecs_to_jiffies(Y2_SYSTEM_PARK_STEP_MS))) {
			/* Recheck producer demand before each slow offline. A race during
    * remove_cpu immediately schedules restoration after it returns. */
			for (cpu = 3; cpu > 0; cpu--)
				if (cpu_online(cpu)) {
					if (atomic_read(&demand))
						break;
					last_error = remove_cpu(cpu);
					if (last_error < 0) {
						broken = true;
						restore_locked();
						break;
					}
					parked_mask |= BIT(cpu);
					park_count++;
					last_park = jiffies;
					if (atomic_read(&demand))
						restore_locked();
					break;
				}
		}
	}
	mutex_unlock(&idle_lock);
	queue_delayed_work(idle_wq, &sample_work, msecs_to_jiffies(250));
}
static int enabled_set(const char *value, const struct kernel_param *kp)
{
	int ret = param_set_bool(value, kp);
	if (!ret && !enabled)
		ret = y2_system_idle_restore();
	return ret;
}
static const struct kernel_param_ops enable_ops = { .set = enabled_set,
						    .get = param_get_bool };
static int frequency_event(struct notifier_block *nb, unsigned long event,
			   void *data)
{
	struct cpufreq_freqs *f = data;
	if (event == CPUFREQ_PRECHANGE && f->new > 598000)
		y2_system_idle_activity();
	return NOTIFY_OK;
}
static struct notifier_block frequency_nb = { .notifier_call =
						      frequency_event };
static int suspend_event(struct notifier_block *nb, unsigned long event,
			 void *data)
{
	int ret = 0;
	if (event == PM_SUSPEND_PREPARE) {
		mutex_lock(&idle_lock);
		paused = true;
		ret = restore_locked();
		mutex_unlock(&idle_lock);
	} else if (event == PM_POST_SUSPEND) {
		mutex_lock(&idle_lock);
		paused = false;
		quiet_since = 0;
		hold_until =
			jiffies + msecs_to_jiffies(Y2_SYSTEM_RESTORE_HOLD_MS);
		mutex_unlock(&idle_lock);
	}
	return ret ? NOTIFY_BAD : NOTIFY_OK;
}
static struct notifier_block suspend_nb = { .notifier_call = suspend_event };
static void idle_input_event(struct input_handle *handle, unsigned type,
			     unsigned code, int value)
{
	if (type != EV_SYN)
		y2_system_idle_activity();
}
static int input_connect(struct input_handler *handler, struct input_dev *dev,
			 const struct input_device_id *id)
{
	struct input_handle *h = kzalloc(sizeof(*h), GFP_KERNEL);
	int ret;
	if (!h)
		return -ENOMEM;
	h->dev = dev;
	h->handler = handler;
	h->name = "y2-system-idle";
	ret = input_register_handle(h);
	if (ret) {
		kfree(h);
		return ret;
	}
	ret = input_open_device(h);
	if (ret) {
		input_unregister_handle(h);
		kfree(h);
	}
	return ret;
}
static void input_disconnect(struct input_handle *h)
{
	input_close_device(h);
	input_unregister_handle(h);
	kfree(h);
}
static const struct input_device_id input_ids[] = { { .driver_info = 1 }, {} };
static struct input_handler input_handler = { .event = idle_input_event,
					      .connect = input_connect,
					      .disconnect = input_disconnect,
					      .name = "y2-system-idle",
					      .id_table = input_ids };
static int __init system_idle_init(void)
{
	int ret;
	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	idle_wq = alloc_workqueue("y2-idle-demand",
				  WQ_UNBOUND | WQ_HIGHPRI | WQ_FREEZABLE, 1);
	if (!idle_wq)
		return -ENOMEM;
	ret = cpufreq_register_notifier(&frequency_nb,
					CPUFREQ_TRANSITION_NOTIFIER);
	if (ret)
		goto fail;
	ret = register_pm_notifier(&suspend_nb);
	if (ret)
		goto frequency;
	ret = input_register_handler(&input_handler);
	if (ret)
		goto pm;
	hold_until = jiffies + msecs_to_jiffies(Y2_SYSTEM_RESTORE_HOLD_MS);
	queue_delayed_work(idle_wq, &sample_work, msecs_to_jiffies(250));
	return 0;
pm:
	unregister_pm_notifier(&suspend_nb);
frequency:
	cpufreq_unregister_notifier(&frequency_nb, CPUFREQ_TRANSITION_NOTIFIER);
fail:
	destroy_workqueue(idle_wq);
	idle_wq = NULL;
	return ret;
}
device_initcall(system_idle_init);
MODULE_LICENSE("GPL");
