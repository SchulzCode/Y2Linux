/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CPU_OPTIONS_H
#define Y2_CPU_OPTIONS_H
#include <linux/types.h>
bool y2_cpu_safe(void);
bool y2_dvfs_disabled(void);
bool y2_idle_disabled(void);
bool y2_deep_idle_disabled(void);
bool y2_suspend_disabled(void);
#endif
