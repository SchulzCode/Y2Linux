/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SPM_H
#define Y2_SPM_H
#include <linux/types.h>
int y2_spm_cpu_disable(unsigned cpu);
int y2_spm_cpu_kill(unsigned cpu);
int y2_spm_cpu_boot(unsigned cpu, unsigned long entry);
int y2_ccf_boot_vector(unsigned long entry);
int y2_spm_radio_power(unsigned domain, int on);
int y2_spm_unstarted_md_off(void);
int y2_ccf_md_unconfigured(void);
int y2_spm_radio_status(unsigned domain);
int y2_ccf_radio_protect(unsigned domain, bool protect);
int y2_ccf_radio_remap(unsigned domain);
#endif
