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
#include <linux/device.h>
#include <linux/mutex.h>
#include <linux/spinlock.h>
#include <linux/string.h>
#include <linux/suspend.h>
#include "pm-journal.h"
#include "pm-journal-policy.h"
#define Y2_PM_PHYS 0x0010dc00
static void __iomem *journal;
static unsigned journal_record[Y2_PM_WORDS], previous[Y2_PM_WORDS], slot;
static unsigned previous_reset_entry;
static DEFINE_RAW_SPINLOCK(journal_lock);
static bool ready;
/* Retained device-callback ring and what the previous boot left in SRAM. */
static unsigned ring_sequence;
static unsigned previous_ring[Y2_PM_RING_ENTRIES][Y2_PM_RING_WORDS];
static unsigned previous_ring_header[8];
static unsigned previous_scratch[Y2_PM_SELFTEST_WORDS];
static DEFINE_MUTEX(selftest_lock);
static char selftest_result[256] = "result=not_run\n";
/* Kernel-owned RGU warm-reset backstop for owner qualification only. */
static const struct y2_pm_backstop_ops *backstop_ops;
static unsigned backstop_armed, backstop_seconds;
static bool backstop_running, backstop_paused, backstop_staged;
static int backstop_error;
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
				     "ABORTED",
				     "HELPER_REQUEST",
				     "TASKS_FROZEN",
				     "PLATFORM_BEGIN",
				     "DPM_PREPARE_BEGIN",
				     "DPM_PREPARED",
				     "LATE_SUSPENDED",
				     "NOIRQ_SUSPENDED",
				     "SECONDARIES_DISABLING",
				     "SYSCORE_SUSPENDED",
				     "PLATFORM_ENTER",
				     "TEST_RETURN",
				     "SELFTEST_A",
				     "SELFTEST_B",
				     "BACKSTOP_STARTED",
				     "EXIT" };
static_assert(ARRAY_SIZE(names) == Y2_PM_STAGE_COUNT);
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
	writel(0, journal + Y2_PM_SLOT(slot));
	for (i = 1; i < Y2_PM_WORDS; i++)
		writel(journal_record[i], journal + Y2_PM_SLOT(slot) + i * 4);
	wmb();
	writel(Y2_PM_MAGIC, journal + Y2_PM_SLOT(slot));
	/* Drain to retained SRAM before the CPU/cluster can disappear. */
	readl(journal + Y2_PM_SLOT(slot));
}
/* Caller holds journal_lock. A new cycle starts an empty callback ring. */
static void ring_reset(void)
{
	unsigned i;
	for (i = 0; i < Y2_PM_RING_ENTRIES; i++)
		writel(0, journal + Y2_PM_RING + i * Y2_PM_RING_WORDS * 4);
	writel(Y2_PM_RING_MAGIC, journal + Y2_PM_RING_HEADER);
	writel(journal_record[1] + 1, journal + Y2_PM_RING_HEADER + 4);
	writel(0, journal + Y2_PM_RING_HEADER + 8);
	writel(backstop_armed, journal + Y2_PM_RING_HEADER + 12);
}
/* Caller holds journal_lock. Sequence is stored last; torn entries read 0. */
static void ring_write(const char *name, unsigned phase, int result, bool leave)
{
	unsigned words[Y2_PM_RING_WORDS] = { 0 }, offset, i;
	ring_sequence = y2_pm_ring_next(ring_sequence);
	offset = y2_pm_ring_offset(ring_sequence);
	words[1] = phase << 8 | leave;
	words[2] = result;
	y2_pm_ring_name(words + 3, name);
	writel(0, journal + offset);
	for (i = 1; i < Y2_PM_RING_WORDS; i++)
		writel(words[i], journal + offset + i * 4);
	wmb();
	writel(ring_sequence, journal + offset);
	writel(ring_sequence, journal + Y2_PM_RING_HEADER + 8);
	readl(journal + offset);
}
/* Out-of-band platform fault (phase 0xff), e.g. the first terminal USB fault,
 * so it survives an owner restart together with the PM callback trail. */
void y2_pm_note(const char *name, int result)
{
	unsigned long flags;
	if (!journal)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	ring_write(name, 0xff, result, true);
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
void y2_pm_device(const struct device *dev, unsigned phase, int result, bool leave)
{
	unsigned long flags;
	const char *name;
	size_t length;
	if (!journal || !dev)
		return;
	/* Keep the distinguishing tail of long device names. */
	name = dev_name(dev);
	length = strlen(name);
	if (length > Y2_PM_RING_NAME)
		name += length - Y2_PM_RING_NAME;
	raw_spin_lock_irqsave(&journal_lock, flags);
	ring_write(name, phase, result, leave);
	raw_spin_unlock_irqrestore(&journal_lock, flags);
}
void y2_pm_mark(unsigned stage, int error)
{
	unsigned long flags;
	if (!journal || stage >= Y2_PM_STAGE_COUNT)
		return;
	raw_spin_lock_irqsave(&journal_lock, flags);
	/* A helper request, or a kernel request not preceded by one, starts a
	 * new cycle. The helper's earlier stage is kept for the same attempt. */
	if (stage == Y2_PM_HELPER_REQUEST || stage == Y2_PM_SELFTEST_A ||
	    (stage == Y2_PM_SUSPEND_REQUEST && journal_record[2] != Y2_PM_HELPER_REQUEST)) {
		unsigned sequence = journal_record[1];
		memset(journal_record, 0, sizeof(journal_record));
		journal_record[1] = sequence;
		if (stage != Y2_PM_SELFTEST_A) {
			writel(0, journal + Y2_PM_STAMP); /* stackless reset-resume entry stamp */
			ring_reset();
		}
	}
	journal_record[2] = stage;
	if (error && !journal_record[4]) {
		journal_record[3] = stage;
		journal_record[4] = error;
	}
	commit();
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	/* Stage progress extends the backstop; a stall of its full period resets. */
	y2_pm_backstop_ping();
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
	reset = readl(journal + Y2_PM_STAMP);
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
void y2_pm_backstop_register(const struct y2_pm_backstop_ops *ops)
{
	WRITE_ONCE(backstop_ops, ops);
}
/* One-shot: arming applies to the next pm_suspend only. Staged pm_test runs
 * keep the RGU counting through every phase; a full sleep pauses it only
 * around SPM entry, so a stall anywhere else becomes a warm reset that keeps
 * this SRAM, instead of a hang that needs an owner power cycle. */
void y2_pm_backstop_begin(bool staged)
{
	const struct y2_pm_backstop_ops *ops = READ_ONCE(backstop_ops);
	unsigned seconds = xchg(&backstop_armed, 0);
	if (!seconds)
		return;
	if (!ops) {
		backstop_error = -ENODEV;
		return;
	}
	backstop_error = ops->start(seconds);
	if (backstop_error)
		return;
	backstop_seconds = seconds;
	backstop_staged = staged;
	backstop_paused = false;
	backstop_running = true;
	y2_pm_mark(Y2_PM_BACKSTOP_STARTED, 0);
}
void y2_pm_backstop_ping(void)
{
	const struct y2_pm_backstop_ops *ops = READ_ONCE(backstop_ops);
	if (READ_ONCE(backstop_running) && !READ_ONCE(backstop_paused) && ops)
		ops->ping();
}
void y2_pm_backstop_pause(bool pause)
{
	const struct y2_pm_backstop_ops *ops = READ_ONCE(backstop_ops);
	if (!READ_ONCE(backstop_running) || !ops || backstop_paused == pause)
		return;
	if (pause) {
		ops->stop();
		backstop_paused = true;
	} else {
		backstop_error = ops->start(backstop_seconds);
		backstop_paused = !!backstop_error;
	}
}
void y2_pm_backstop_end(void)
{
	const struct y2_pm_backstop_ops *ops = READ_ONCE(backstop_ops);
	if (!backstop_running)
		return;
	if (ops && !backstop_paused)
		ops->stop();
	backstop_running = false;
	backstop_paused = false;
}
static ssize_t backstop_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	return sysfs_emit(buf, "provider=%u armed_s=%u running=%u paused=%u staged=%u seconds=%u error=%d\n",
		!!READ_ONCE(backstop_ops), READ_ONCE(backstop_armed), backstop_running,
		backstop_paused, backstop_staged, backstop_seconds, backstop_error);
}
static ssize_t backstop_store(struct kobject *k, struct kobj_attribute *a,
			      const char *buf, size_t size)
{
	unsigned seconds;
	int ret;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	ret = kstrtouint(buf, 0, &seconds);
	if (ret)
		return ret;
	if (seconds && (seconds < 10 || seconds > 30))
		return -ERANGE;
	if (seconds && !READ_ONCE(backstop_ops))
		return -ENODEV;
	WRITE_ONCE(backstop_armed, seconds);
	return size;
}
static ssize_t emit_ring(char *buf, unsigned (*entries)[Y2_PM_RING_WORDS],
			 const unsigned *header)
{
	unsigned order[Y2_PM_RING_ENTRIES], count = 0, i, j, t;
	int n;
	n = sysfs_emit(buf, "ring=%s cycle=%u last=%u backstop_s=%u\n",
		header[0] == Y2_PM_RING_MAGIC ? "valid" : "absent", header[1], header[2], header[3]);
	if (header[0] != Y2_PM_RING_MAGIC)
		return n;
	for (i = 0; i < Y2_PM_RING_ENTRIES; i++)
		if (entries[i][0])
			order[count++] = i;
	for (i = 1; i < count; i++)	/* insertion sort by wrapping sequence */
		for (j = i; j && y2_pm_newer(entries[order[j - 1]][0], entries[order[j]][0]); j--) {
			t = order[j]; order[j] = order[j - 1]; order[j - 1] = t;
		}
	for (i = 0; i < count; i++) {
		unsigned *e = entries[order[i]];
		char name[Y2_PM_RING_NAME + 1];
		memcpy(name, e + 3, Y2_PM_RING_NAME);
		name[Y2_PM_RING_NAME] = 0;
		n += sysfs_emit_at(buf, n, "%u phase=%u %s result=%d device=%s\n", e[0],
			e[1] >> 8, (e[1] & 1) ? "leave" : "enter", (int)e[2], name);
	}
	return n;
}
static void read_ring(unsigned (*entries)[Y2_PM_RING_WORDS], unsigned *header)
{
	unsigned i, w;
	for (i = 0; i < 8; i++)
		header[i] = i < 4 ? readl(journal + Y2_PM_RING_HEADER + i * 4) : 0;
	for (i = 0; i < Y2_PM_RING_ENTRIES; i++)
		for (w = 0; w < Y2_PM_RING_WORDS; w++)
			entries[i][w] = readl(journal + Y2_PM_RING + (i * Y2_PM_RING_WORDS + w) * 4);
}
static ssize_t devices_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	static unsigned entries[Y2_PM_RING_ENTRIES][Y2_PM_RING_WORDS];
	unsigned header[8];
	unsigned long flags;
	ssize_t n;
	mutex_lock(&selftest_lock);
	raw_spin_lock_irqsave(&journal_lock, flags);
	read_ring(entries, header);
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	n = emit_ring(buf, entries, header);
	mutex_unlock(&selftest_lock);
	return n;
}
static ssize_t devices_previous_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	return emit_ring(buf, previous_ring, previous_ring_header);
}
static unsigned scratch_pattern(unsigned sequence, unsigned i)
{
	return 0x59325400U ^ (sequence * 16 + i) ^ (i << 24);
}
/* What the previous boot's final self-test left, as found by this boot. A
 * match after an ordinary RGU warm reboot proves electrical retention. */
static ssize_t retention_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	unsigned i, match = y2_pm_valid(previous) && previous[2] == Y2_PM_SELFTEST_B;
	for (i = 0; match && i < Y2_PM_SELFTEST_WORDS; i++)
		match = previous_scratch[i] == scratch_pattern(previous[1], i);
	return sysfs_emit(buf, "phys=%#x size=%#x previous_valid=%u previous_sequence=%u previous_stage=%s selftest_scratch=%s previous_ring=%s previous_reset_entry=%#x\n",
		Y2_PM_PHYS, Y2_PM_REGION, y2_pm_valid(previous), previous[1],
		stage_name(previous[2]), match ? "retained" : "absent",
		previous_ring_header[0] == Y2_PM_RING_MAGIC ? "valid" : "absent",
		previous_reset_entry);
}
static int words_equal(void __iomem *base, unsigned offset, const unsigned *words, unsigned count)
{
	unsigned i;
	for (i = 0; i < count; i++)
		if (readl(base + offset + i * 4) != words[i])
			return 0;
	return 1;
}
/* Awake proof of the diagnostic path: every write is read back through a
 * second, independent mapping of the same physical SRAM. Serialized with
 * system sleep; never enters suspend and never reboots. */
static const char *selftest(void __iomem *alias, unsigned *a_sequence, unsigned *b_sequence,
			    unsigned *a_slot, unsigned *b_slot)
{
	unsigned copy[Y2_PM_WORDS], words[Y2_PM_WORDS], stamp, i, sa, sb;
	unsigned long flags;
	y2_pm_mark(Y2_PM_SELFTEST_A, 0);
	raw_spin_lock_irqsave(&journal_lock, flags);
	memcpy(copy, journal_record, sizeof(copy));
	sa = slot;
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	*a_sequence = copy[1];
	*a_slot = sa;
	if (!words_equal(alias, Y2_PM_SLOT(sa), copy, Y2_PM_WORDS))
		return "stage_a_readback";
	for (i = 0; i < Y2_PM_WORDS; i++)
		words[i] = readl(alias + Y2_PM_SLOT(sa) + i * 4);
	if (!y2_pm_valid(words) || words[2] != Y2_PM_SELFTEST_A)
		return "stage_a_checksum";
	y2_pm_mark(Y2_PM_SELFTEST_B, 0);
	raw_spin_lock_irqsave(&journal_lock, flags);
	memcpy(copy, journal_record, sizeof(copy));
	sb = slot;
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	*b_sequence = copy[1];
	*b_slot = sb;
	if (sb == sa)
		return "slot_not_alternated";
	if (!y2_pm_newer(copy[1], *a_sequence) || copy[1] != *a_sequence + 1)
		return "sequence";
	if (!words_equal(alias, Y2_PM_SLOT(sb), copy, Y2_PM_WORDS))
		return "stage_b_readback";
	for (i = 0; i < Y2_PM_WORDS; i++)
		words[i] = readl(alias + Y2_PM_SLOT(sa) + i * 4);
	if (!y2_pm_valid(words) || words[2] != Y2_PM_SELFTEST_A || words[1] != *a_sequence)
		return "previous_record_lost";
	/* Boot-ROM reset-dispatch stamp word: writable, then restored. */
	stamp = readl(journal + Y2_PM_STAMP);
	writel(stamp ^ 0xa55a5aa5U, journal + Y2_PM_STAMP);
	i = readl(alias + Y2_PM_STAMP) == (stamp ^ 0xa55a5aa5U);
	writel(stamp, journal + Y2_PM_STAMP);
	if (!i || readl(alias + Y2_PM_STAMP) != stamp)
		return "reset_stamp";
	/* Retention marker for an optional ordinary warm reboot. */
	for (i = 0; i < Y2_PM_SELFTEST_WORDS; i++)
		writel(scratch_pattern(copy[1], i), journal + Y2_PM_SELFTEST + i * 4);
	for (i = 0; i < Y2_PM_SELFTEST_WORDS; i++)
		if (readl(alias + Y2_PM_SELFTEST + i * 4) != scratch_pattern(copy[1], i))
			return "scratch";
	raw_spin_lock_irqsave(&journal_lock, flags);
	ring_reset();
	ring_write("y2-pm-selftest", 0, 0, false);
	ring_write("y2-pm-selftest", 0, 0, true);
	i = ring_sequence;
	raw_spin_unlock_irqrestore(&journal_lock, flags);
	if (readl(alias + Y2_PM_RING_HEADER) != Y2_PM_RING_MAGIC ||
	    readl(alias + y2_pm_ring_offset(i)) != i ||
	    (readl(alias + y2_pm_ring_offset(i) + 4) & 1) != 1)
		return "ring";
	/* The preloader rewrites 0010dc00 only when it holds its own magic. */
	if (readl(alias + Y2_PM_SLOT(0)) == 0x43474244U)
		return "ram_console_magic";
	return NULL;
}
static ssize_t selftest_show(struct kobject *k, struct kobj_attribute *a, char *buf)
{
	ssize_t n;
	mutex_lock(&selftest_lock);
	n = sysfs_emit(buf, "%s", selftest_result);
	mutex_unlock(&selftest_lock);
	return n;
}
static ssize_t selftest_store(struct kobject *k, struct kobj_attribute *a,
			      const char *buf, size_t size)
{
	unsigned sa = 0, sb = 0, slot_a = 0, slot_b = 0;
	void __iomem *alias;
	const char *failure;
	unsigned int sleep_flags;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	if (!sysfs_streq(buf, "run"))
		return -EINVAL;
	sleep_flags = lock_system_sleep();
	mutex_lock(&selftest_lock);
	alias = ioremap(Y2_PM_PHYS, Y2_PM_REGION);
	failure = alias ? selftest(alias, &sa, &sb, &slot_a, &slot_b) : "alias_mapping";
	if (alias)
		iounmap(alias);
	snprintf(selftest_result, sizeof(selftest_result),
		 "result=%s failure=%s phys=%#x size=%#x sequence_a=%u sequence_b=%u slot_a=%u slot_b=%u stamp=%s ring=%s\n",
		 failure ? "fail" : "pass", failure ? failure : "none", Y2_PM_PHYS, Y2_PM_REGION,
		 sa, sb, slot_a, slot_b, failure ? "unverified" : "restored",
		 failure ? "unverified" : "verified");
	mutex_unlock(&selftest_lock);
	unlock_system_sleep(sleep_flags);
	return failure ? -EIO : size;
}
static struct kobj_attribute state_attr = __ATTR_RO(state),
			     previous_attr = __ATTR_RO(previous),
			     devices_attr = __ATTR_RO(devices),
			     devices_previous_attr = __ATTR_RO(devices_previous),
			     retention_attr = __ATTR_RO(retention);
static struct kobj_attribute stage_attr =
	__ATTR(stage, 0200, NULL, stage_store);
static struct kobj_attribute selftest_attr =
	__ATTR(selftest, 0600, selftest_show, selftest_store);
static struct kobj_attribute backstop_attr =
	__ATTR(backstop_s, 0600, backstop_show, backstop_store);
static struct attribute *attrs[] = { &state_attr.attr, &previous_attr.attr,
				     &stage_attr.attr, &devices_attr.attr,
				     &devices_previous_attr.attr, &retention_attr.attr,
				     &selftest_attr.attr, &backstop_attr.attr, NULL };
static const struct attribute_group group = { .attrs = attrs };
static int __init journal_init(void)
{
	unsigned a[Y2_PM_WORDS], b[Y2_PM_WORDS], i;
	struct kobject *k;
	int ret;
	if (!of_machine_is_compatible("innioasis,y2"))
		return -ENODEV;
	if (!request_mem_region(Y2_PM_PHYS, Y2_PM_REGION, "y2-suspend-journal"))
		return -EBUSY;
	journal = ioremap(Y2_PM_PHYS, Y2_PM_REGION);
	if (!journal) {
		release_mem_region(Y2_PM_PHYS, Y2_PM_REGION);
		return -ENOMEM;
	}
	for (i = 0; i < Y2_PM_WORDS; i++) {
		a[i] = readl(journal + Y2_PM_SLOT(0) + i * 4);
		b[i] = readl(journal + Y2_PM_SLOT(1) + i * 4);
	}
	slot = y2_pm_valid(b) && (!y2_pm_valid(a) || y2_pm_newer(b[1], a[1]));
	if (y2_pm_valid(slot ? b : a))
		memcpy(previous, slot ? b : a, sizeof(previous));
	memcpy(journal_record, previous, sizeof(journal_record));
	previous_reset_entry = readl(journal + Y2_PM_STAMP);
	for (i = 0; i < Y2_PM_SELFTEST_WORDS; i++)
		previous_scratch[i] = readl(journal + Y2_PM_SELFTEST + i * 4);
	read_ring(previous_ring, previous_ring_header);
	if (previous_ring_header[0] == Y2_PM_RING_MAGIC)
		ring_sequence = previous_ring_header[2];
	k = kobject_create_and_add("y2_pm", firmware_kobj);
	ret = k ? sysfs_create_group(k, &group) : -ENOMEM;
	if (ret) {
		if (k)
			kobject_put(k);
		iounmap(journal);
		journal = NULL;
		release_mem_region(Y2_PM_PHYS, Y2_PM_REGION);
	} else
		ready = true;
	return ret;
}
subsys_initcall(journal_init);
