/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SYSTEM_IDLE_H
#define Y2_SYSTEM_IDLE_H
#include <linux/kconfig.h>
#if IS_ENABLED(CONFIG_Y2_POWER)
void y2_system_idle_activity(void);
int y2_system_idle_restore(void);
bool y2_backlight_dark(void);
bool y2_workload_active(void);
bool y2_workload_idle_blocked(void);
#else
static inline void y2_system_idle_activity(void)
{
}
static inline int y2_system_idle_restore(void)
{
	return 0;
}
#endif
#endif
