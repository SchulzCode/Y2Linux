// SPDX-License-Identifier: GPL-2.0-only
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include "cpu-options.h"
static bool safe, dvfs_off, idle_off, deep_off, suspend_safe;
module_param(safe, bool, 0400);
module_param(dvfs_off, bool, 0400);
module_param(idle_off, bool, 0400);
module_param(deep_off, bool, 0400);
module_param(suspend_safe, bool, 0400);
bool y2_cpu_safe(void) { return safe; }
bool y2_dvfs_disabled(void) { return safe || dvfs_off; }
bool y2_idle_disabled(void) { return safe || idle_off; }
bool y2_deep_idle_disabled(void) { return safe || idle_off || deep_off; }
bool y2_suspend_disabled(void) { return safe || suspend_safe; }
static int __init safe_option(char *s) { safe = strcmp(s, "0") != 0; return 1; }
static int __init dvfs_option(char *s) { dvfs_off = strcmp(s, "on") != 0; return 1; }
static int __init idle_option(char *s) { idle_off = strcmp(s, "on") != 0; return 1; }
static int __init deep_option(char *s) { deep_off = strcmp(s, "on") != 0; return 1; }
static int __init suspend_option(char *s) { suspend_safe = strcmp(s, "0") != 0; return 1; }
__setup("y2.cpu_safe=", safe_option);
__setup("y2.dvfs=", dvfs_option);
__setup("y2.cpuidle=", idle_option);
__setup("y2.deep_idle=", deep_option);
__setup("y2.suspend_safe=", suspend_option);
