/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PM_JOURNAL_H
#define Y2_PM_JOURNAL_H
#include <linux/kconfig.h>
enum y2_pm_stage {
	Y2_PM_NONE,
	Y2_PM_SUSPEND_REQUEST,
	Y2_PM_FILESYSTEM_SYNCED,
	Y2_PM_DEVICES_SUSPENDED,
	Y2_PM_SECONDARIES_OFF,
	Y2_PM_CIRQ_CLONED,
	Y2_PM_WAKE_MASK_PROGRAMMED,
	Y2_PM_RTC_ARMED,
	Y2_PM_PCM_INSTALLED,
	Y2_PM_CPU_CONTEXT_SAVING,
	Y2_PM_BEFORE_SPM_ENTRY,
	Y2_PM_AFTER_SPM_RETURN,
	Y2_PM_CPU_CONTEXT_RESTORED,
	Y2_PM_CIRQ_REPLAYED,
	Y2_PM_TIMER_RESTORED,
	Y2_PM_SECONDARIES_ON,
	Y2_PM_DEVICES_RESUMING,
	Y2_PM_RADIOS_RESTORING,
	Y2_PM_REBORN_READY,
	Y2_PM_COMPLETE,
	Y2_PM_UART_REQUEST,
	Y2_PM_UART_ACK,
	Y2_PM_NORMAL_PCM_RESTORED,
	Y2_PM_ABORTED,
	Y2_PM_STAGE_COUNT
};
#if IS_ENABLED(CONFIG_Y2_POWER)
bool y2_pm_journal_ready(void);
void y2_pm_entry_snapshot(unsigned timer, unsigned watchdog, unsigned con1,
			  unsigned clock, unsigned cache);
void y2_pm_mark(unsigned stage, int error);
void y2_pm_spm_snapshot(unsigned wake, unsigned r13, unsigned raw,
			unsigned pointer, unsigned length, unsigned mask,
			unsigned vector, unsigned enable, unsigned cirq,
			unsigned fsm, unsigned power, unsigned power_s);
void y2_pm_pmic_snapshot(unsigned rtc_enable, unsigned mask0, unsigned mask1,
			 unsigned status0, unsigned status1);
extern void y2_cpu_resume(void);
#else
static inline void y2_pm_mark(unsigned stage, int error)
{
}
static inline void y2_pm_pmic_snapshot(unsigned a, unsigned b, unsigned c,
				       unsigned d, unsigned e)
{
}
#endif
#endif
