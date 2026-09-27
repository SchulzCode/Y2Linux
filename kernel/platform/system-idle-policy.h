/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SYSTEM_IDLE_POLICY_H
#define Y2_SYSTEM_IDLE_POLICY_H
/* Source SLIDLE needs exactly one online CPU. Core parking is a slow system
 * inactivity decision, never made in the cpuidle IRQ-off entry callback. */
#define Y2_SYSTEM_QUIET_MS 30000
#define Y2_SYSTEM_PARK_STEP_MS 5000
#define Y2_SYSTEM_RESTORE_HOLD_MS 60000
static inline int y2_system_quiet(unsigned elapsed_ms, unsigned busy_percent,
				  int dark, int lease, int timer,
				  unsigned frequency)
{
	return elapsed_ms >= Y2_SYSTEM_QUIET_MS && busy_percent <= 10 && dark &&
	       !lease && timer && frequency && frequency <= 598000;
}
#endif
