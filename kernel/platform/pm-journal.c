// SPDX-License-Identifier: GPL-2.0-only
/* Board-specific retained SRAM journal. Physical mapping and the allocation are
 * proven by exact Y2 mt_map_io and ram_console_early_init disassembly. Linux has
 * no other owner for that stock RAM-console buffer. Warm reset retention is the
 * hardware contract; cold power loss is covered by the helper's durable pre/post
 * file. Neither mechanism claims electrical qualification. */
#include <linux/io.h>
#include <linux/ioport.h>
#include <linux/init.h>
#include <linux/kobject.h>
#include <linux/firmware.h>
#include <linux/capability.h>
#include <linux/of.h>
#include <linux/spinlock.h>
#include <linux/string.h>
#include "pm-journal.h"
#include "pm-journal-policy.h"
static void __iomem *journal;
static unsigned journal_record[Y2_PM_WORDS], previous[Y2_PM_WORDS], slot;
static unsigned previous_reset_entry;
static DEFINE_RAW_SPINLOCK(journal_lock);
static bool ready;
bool y2_pm_journal_ready(void)
{
	return ready;
}
static const char *const names[] = { "NONE",
				     "SUSPEND_REQUEST",
				     "FILESYSTEM_SYNCED",
				     "DEVICES_SUSPENDED",
				     "SECONDARIES_OFF",
				     "CIRQ_CLONED",
				     "WAKE_MASK_PROGRAMMED",
				     "RTC_ARMED",
				     "PCM_INSTALLED",
				     "CPU_CONTEXT_SAVING",
				     "BEFORE_SPM_ENTRY",
				     "AFTER_SPM_RETURN",
				     "CPU_CONTEXT_RESTORED",
				     "CIRQ_REPLAYED",
				     "TIMER_RESTORED",
				     "SECONDARIES_ON",
				     "DEVICES_RESUMING",
				     "RADIOS_RESTORING",
				     "REBORN_READY",
				     "COMPLETE",
				     "UART_REQUEST",
				     "UART_ACK",
				     "NORMAL_PCM_RESTORED",
				     "ABORTED" };
static const char *stage_name(unsigned stage)
{
	return stage < ARRAY_SIZE(names) ? names[stage] : "INVALID";
}
static void commit(void)
{
	unsigned i;
	slot ^= 1;
	journal_record[1]++;
	journal_record[0] = Y2_PM_MAGIC;
	journal_record[Y2_PM_WORDS - 1] = y2_pm_checksum(journal_record);
	writel(0, journal + slot * 0x80);
	for (i = 1; i < Y2_PM_WORDS; i++)
		writel(journal_record[i], journal + slot * 0x80 + i * 4);
	wmb();
	writel(Y2_PM_MAGIC, journal + slot * 0x80);
	/* Drain to retained SRAM before the CPU/cluster can disappear. */
	readl(journal + slot * 0x80);
}
void y2_pm_mark(unsigned stage, int error)
{
	unsigned long flags;
	if (!journal || stage >= Y2_PM_STAGE_COUNT)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	if (stage == Y2_PM_SUSPEND_REQUEST) {
		unsigned sequence = journal_record[1];
		memset(journal_record, 0, sizeof(journal_record));
		journal_record[1] = sequence;
		writel(0,
		       journal +
			       0x100); /* stackless reset-resume entry stamp */
	}
	journal_record[2] = stage;
	if (error && !journal_record[4]) {
		journal_record[3] = stage;
		journal_record[4] = error;
	}
	commit();
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
void y2_pm_spm_snapshot(unsigned wake, unsigned r13, unsigned raw,
			unsigned pointer, unsigned length, unsigned mask,
			unsigned vector, unsigned enable, unsigned cirq,
			unsigned fsm, unsigned power, unsigned power_s)
{
	unsigned long flags;
	if (!journal)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	journal_record[5] = wake;
	journal_record[6] = r13;
	journal_record[7] = raw;
	journal_record[8] = pointer;
	journal_record[9] = length;
	journal_record[10] = mask;
	journal_record[11] = vector;
	journal_record[12] = enable;
	journal_record[13] = cirq;
	journal_record[14] = fsm;
	journal_record[15] = power;
	journal_record[16] = power_s;
	commit();
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
void y2_pm_pmic_snapshot(unsigned rtc_enable, unsigned mask0, unsigned mask1,
			 unsigned status0, unsigned status1)
{
	unsigned long flags;
	if (!journal)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	journal_record[17] = rtc_enable;
	journal_record[18] = mask0;
	journal_record[19] = mask1;
	journal_record[20] = status0;
	journal_record[21] = status1;
	commit();
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
void y2_pm_entry_snapshot(unsigned timer, unsigned watchdog, unsigned con1,
			  unsigned clock, unsigned cache)
{
	unsigned long flags;
	if (!journal)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	journal_record[22] = timer;
	journal_record[23] = watchdog;
	journal_record[24] = con1;
	journal_record[25] = clock;
	journal_record[26] = cache;
	commit();
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
static ssize_t emit_record(char *buf, unsigned *record, unsigned reset)
{
	return sysfs_emit(
		buf,
		"valid=%u sequence=%u stage=%s failed_stage=%s error=%d wake=%#x r13=%#x raw=%#x pcm_pointer=%#x pcm_length=%u wake_mask=%#x boot_vector=%#x boot_enable=%#x cirq=%#x fsm=%#x power=%#x power_s=%#x rtc_enable=%#x pmic_mask0=%#x pmic_mask1=%#x pmic_status0=%#x pmic_status1=%#x reset_resume_entry=%#x sleep_timer=%u watchdog_timer=%u pcm_control1=%#x clock_control=%#x ca7_cache_config=%#x retention=stock_internal_sram_warm_reset\n",
		y2_pm_valid(record), record[1], stage_name(record[2]),
		stage_name(record[3]), (int)record[4], record[5], record[6],
		record[7], record[8], record[9], record[10], record[11],
		record[12], record[13], record[14], record[15], record[16],
		record[17], record[18], record[19], record[20], record[21],
		reset, record[22], record[23], record[24], record[25],
		record[26]);
}
static ssize_t state_show(struct kobject *k, struct kobj_attribute *a,
			  char *buf)
{
	unsigned copy[Y2_PM_WORDS], reset;
	unsigned long flags;
	raw_spin_lock_irqsave(&journal_lock, flags);
	memcpy(copy, journal_record, sizeof(copy));
	reset = readl(journal + 0x100);
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	return emit_record(buf, copy, reset);
}
static ssize_t previous_show(struct kobject *k, struct kobj_attribute *a,
			     char *buf)
{
	return emit_record(buf, previous, previous_reset_entry);
}
static ssize_t stage_store(struct kobject *k, struct kobj_attribute *a,
			   const char *buf, size_t size)
{
	unsigned i;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	for (i = 1; i < ARRAY_SIZE(names); i++)
		if (sysfs_streq(buf, names[i])) {
			y2_pm_mark(i, 0);
			return size;
		}
	return -EINVAL;
}
static struct kobj_attribute state_attr = __ATTR_RO(state),
			     previous_attr = __ATTR_RO(previous);
static struct kobj_attribute stage_attr =
	__ATTR(stage, 0200, NULL, stage_store);
static struct attribute *attrs[] = { &state_attr.attr, &previous_attr.attr,
				     &stage_attr.attr, NULL };
static const struct attribute_group group = { .attrs = attrs };
static int __init journal_init(void)
{
	unsigned a[Y2_PM_WORDS], b[Y2_PM_WORDS], i;
	struct kobject *k;
	int ret;
	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	if (!request_mem_region(0x0010dc00, 0x104, "y2-suspend-journal"))
		return -EBUSY;
	journal = ioremap(0x0010dc00, 0x104);
	if (!journal) {
		release_mem_region(0x0010dc00, 0x104);
		return -ENOMEM;
	}
	for (i = 0; i < Y2_PM_WORDS; i++) {
		a[i] = readl(journal + i * 4);
		b[i] = readl(journal + 0x80 + i * 4);
	}
	slot = y2_pm_valid(b) && (!y2_pm_valid(a) || y2_pm_newer(b[1], a[1]));
	if (y2_pm_valid(slot ? b : a))
		memcpy(previous, slot ? b : a, sizeof(previous));
	memcpy(journal_record, previous, sizeof(journal_record));
	previous_reset_entry = readl(journal + 0x100);
	k = kobject_create_and_add("y2_pm", firmware_kobj);
	ret = k ? sysfs_create_group(k, &group) : -ENOMEM;
	if (ret) {
		if (k)
			kobject_put(k);
		iounmap(journal);
		journal = NULL;
		release_mem_region(0x0010dc00, 0x104);
	} else
		ready = true;
	return ret;
}
subsys_initcall(journal_init);
