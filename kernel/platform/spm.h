/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SPM_H
#define Y2_SPM_H
int y2_spm_cpu_disable(unsigned cpu);
int y2_spm_cpu_kill(unsigned cpu);
int y2_spm_cpu_boot(unsigned cpu, unsigned long entry);
int y2_ccf_boot_vector(unsigned long entry);
#endif
