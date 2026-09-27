/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CPU_DVFS_H
#define Y2_CPU_DVFS_H
#include <linux/kconfig.h>
#include <linux/errno.h>
#if IS_ENABLED(CONFIG_Y2_POWER)
unsigned long y2_cpu_dvfs_max(void);
void y2_cpu_dvfs_fault(void);
int y2_spm_cpu_voltage_request(unsigned slot);
#else
static inline unsigned long y2_cpu_dvfs_max(void) { return 1040000000; }
static inline void y2_cpu_dvfs_fault(void) { }
static inline int y2_spm_cpu_voltage_request(unsigned slot) { return -EOPNOTSUPP; }
#endif
int y2_pmic_cpu_dvfs_prepare(void);
int y2_pmic_cpu_voltage_get(void);
int y2_pmic_cpu_voltage_set(unsigned selector);
#endif
