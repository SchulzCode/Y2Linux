/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PM_JOURNAL_POLICY_H
#define Y2_PM_JOURNAL_POLICY_H
#define Y2_PM_MAGIC 0x5932504dU
#define Y2_PM_WORDS 28
/* Commit marker is stored last. Both slots stay within the stock Y2 retained
 * INTER_SRAM RAM-console allocation at physical 0010dc00..0010f7ff.
 * No filesystem, PMIC spare/boot flags or DRAM allocation is used in noirq. */
static inline unsigned y2_pm_checksum(const unsigned *words)
{
	unsigned i, value = 0x658232U;
	for (i = 1; i < Y2_PM_WORDS - 1; i++)
		value = (value << 5 | value >> 27) ^ words[i];
	return value;
}
static inline int y2_pm_valid(const unsigned *words)
{
	return words[0] == Y2_PM_MAGIC &&
	       words[Y2_PM_WORDS - 1] == y2_pm_checksum(words);
}
static inline int y2_pm_newer(unsigned a, unsigned b)
{
	return (int)(a - b) > 0;
}
/* Fix02 retained layout inside the proven 0010dc00..0010f7ff allocation,
 * kept below 0010e100: retained Y2 LK references 0010f000.. and preloader
 * 0010f040.., never this window. Slots and the Boot-ROM stamp are unchanged. */
#define Y2_PM_REGION 0x500
#define Y2_PM_SLOT(n) ((n) * 0x80)
#define Y2_PM_STAMP 0x100
#define Y2_PM_RESUME_STAMP 0x59325253U
/* Cold SRAM is undefined; only the exact stackless resume-vector stamp is
 * evidence that Boot ROM dispatched Linux resume. */
static inline unsigned y2_pm_resume_stamp(unsigned raw)
{
	return raw == Y2_PM_RESUME_STAMP ? raw : 0;
}
#define Y2_PM_SELFTEST 0x140
#define Y2_PM_SELFTEST_WORDS 16
#define Y2_PM_RING_HEADER 0x180
/* Fix02 ring (20-byte names, no time) is recognized and not decoded. */
#define Y2_PM_RING_MAGIC_FIX02 0x5932444eU
#define Y2_PM_RING_MAGIC 0x5932444fU
#define Y2_PM_RING 0x1a0
#define Y2_PM_RING_ENTRIES 24
#define Y2_PM_RING_WORDS 8
#define Y2_PM_RING_NAME 16
/* Entry: w0 sequence (written last, 0 = torn/empty), w1 stage<<16 | phase<<8
 * | leave, w2 result, w3..w6 name, w7 local_clock milliseconds. Device PM
 * callbacks use phases 1..8, platform notes 0xff, stage marks 0xfe (named by
 * the stage field). The sequence never becomes 0. */
#define Y2_PM_PHASE_NOTE 0xffU
#define Y2_PM_PHASE_MARK 0xfeU
static inline unsigned y2_pm_ring_next(unsigned sequence)
{
	return sequence + 1 ? sequence + 1 : 1;
}
static inline unsigned y2_pm_ring_offset(unsigned sequence)
{
	return Y2_PM_RING + (sequence % Y2_PM_RING_ENTRIES) * Y2_PM_RING_WORDS * 4;
}
static inline void y2_pm_ring_name(unsigned *words, const char *name)
{
	unsigned i;
	unsigned char *bytes = (unsigned char *)words;
	for (i = 0; i < Y2_PM_RING_NAME; i++) {
		bytes[i] = name && *name ? (unsigned char)*name++ : 0;
	}
}
/* MT6582 RGU WDT_STATUS (+0x0c), exact BSP mt_wdt.h bits. Read once at
 * watchdog probe, before the driver writes WDT_MODE. Whether the loader
 * preserves it across its own RGU use is a physical question; zero is
 * reported as "none_reported", never as a power-on proof. */
#define Y2_RGU_HW_WATCHDOG 0x80000000U
#define Y2_RGU_SW_RESET 0x40000000U
#define Y2_RGU_IRQ_WATCHDOG 0x20000000U
#define Y2_RGU_DEBUG_RESET 0x00080000U
#define Y2_RGU_SPM_WATCHDOG 0x00000001U
static inline const char *y2_rgu_cause(unsigned raw)
{
	if (raw & Y2_RGU_HW_WATCHDOG)
		return "watchdog_timeout";
	if (raw & Y2_RGU_SW_RESET)
		return "software_reset";
	if (raw & Y2_RGU_IRQ_WATCHDOG)
		return "watchdog_irq";
	if (raw & Y2_RGU_DEBUG_RESET)
		return "debug_reset";
	if (raw & Y2_RGU_SPM_WATCHDOG)
		return "spm_watchdog";
	return raw ? "unknown" : "none_reported";
}
#endif
