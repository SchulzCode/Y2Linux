/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PM_JOURNAL_H
#define Y2_PM_JOURNAL_H
#include <linux/kconfig.h>
#include <linux/types.h>
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
	/* Fix02: earlier and finer PM boundaries; appended to keep old records. */
	Y2_PM_HELPER_REQUEST,
	Y2_PM_TASKS_FROZEN,
	Y2_PM_PLATFORM_BEGIN,
	Y2_PM_DPM_PREPARE_BEGIN,
	Y2_PM_DPM_PREPARED,
	Y2_PM_LATE_SUSPENDED,
	Y2_PM_NOIRQ_SUSPENDED,
	Y2_PM_SECONDARIES_DISABLING,
	Y2_PM_SYSCORE_SUSPENDED,
	Y2_PM_PLATFORM_ENTER,
	Y2_PM_TEST_RETURN,
	Y2_PM_SELFTEST_A,
	Y2_PM_SELFTEST_B,
	Y2_PM_BACKSTOP_STARTED,
	Y2_PM_EXIT,
	/* Fix03: the Fix02 RTC attempt stopped between dpm_resume_end and
	 * Y2_PM_EXIT with no mark. Every call in that window is now bracketed;
	 * appended to keep old records. */
	Y2_PM_DEVICES_RESUMED,
	Y2_PM_CONSOLE_RESUMED,
	Y2_PM_PLATFORM_ENDED,
	Y2_PM_TASKS_THAWED,
	Y2_PM_FILESYSTEMS_THAWED,
	Y2_PM_POST_SUSPEND_NOTIFIED,
	Y2_PM_CONSOLE_RESTORED,
	Y2_PM_DORMANT_BEGIN,
	Y2_PM_DORMANT_CONTEXT,
	Y2_PM_DORMANT_FINISH,
	Y2_PM_DORMANT_RETURN,
	Y2_PM_DORMANT_COMPLETE,
	Y2_PM_DORMANT_ABORTED,
	Y2_PM_STAGE_COUNT
};
/* Device PM callback phases recorded in the retained callback ring. */
enum y2_pm_phase {
	Y2_PM_PHASE_PREPARE = 1,
	Y2_PM_PHASE_SUSPEND,
	Y2_PM_PHASE_LATE,
	Y2_PM_PHASE_NOIRQ,
	Y2_PM_PHASE_RESUME_NOIRQ,
	Y2_PM_PHASE_EARLY,
	Y2_PM_PHASE_RESUME,
	Y2_PM_PHASE_COMPLETE,
};
struct device;
/* Kernel-owned RGU backstop provider (registered by the watchdog driver). */
struct y2_pm_backstop_ops {
	int (*start)(unsigned int seconds);
	void (*ping)(void);
	void (*stop)(void);
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
void y2_pm_device(const struct device *dev, unsigned phase, int result, bool leave);
void y2_pm_note(const char *name, int result);
void y2_pm_reset_status(unsigned raw);
void y2_pm_backstop_register(const struct y2_pm_backstop_ops *ops);
void y2_pm_backstop_begin(bool staged);
void y2_pm_backstop_ping(void);
void y2_pm_backstop_pause(bool pause);
void y2_pm_backstop_end(void);
bool y2_pm_dormant_backstop_ready(void);
bool y2_pm_dormant_backstop_running(void);
extern void y2_cpu_resume(void);
#else
static inline void y2_pm_device(const struct device *dev, unsigned phase,
				int result, bool leave)
{
}
static inline void y2_pm_note(const char *name, int result)
{
}
static inline void y2_pm_reset_status(unsigned raw)
{
}
static inline void y2_pm_backstop_register(const struct y2_pm_backstop_ops *ops)
{
}
static inline void y2_pm_backstop_begin(bool staged)
{
}
static inline void y2_pm_backstop_ping(void)
{
}
static inline void y2_pm_backstop_pause(bool pause)
{
}
static inline void y2_pm_backstop_end(void)
{
}
static inline void y2_pm_mark(unsigned stage, int error)
{
}
static inline void y2_pm_pmic_snapshot(unsigned a, unsigned b, unsigned c,
				       unsigned d, unsigned e)
{
}
#endif
#endif
