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
#endif
