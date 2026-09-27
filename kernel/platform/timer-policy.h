/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_TIMER_POLICY_H
#define Y2_TIMER_POLICY_H
/* GPT control: ENABLE[0], self-clearing CLEAR[1], MODE[5:4].
 * Firmware ownership ends at Linux timer discovery. A running free counter
 * is adopted without stopping/clearing it; known stopped/reset states may
 * be prepared. Reserved bits are never silently overwritten. */
static inline const char *y2_gpt6_predicate(unsigned rate, unsigned pfr1,
					    unsigned control, unsigned clock)
{
	if (rate != 13000000)
		return "gpt2_rate_not_13mhz";
	if (((pfr1 >> 16) & 15) != 1)
		return "generic_timer_feature_absent";
	if (control & ~0x33U)
		return "gpt6_reserved_control_bits";
	if (clock & ~0x1fU)
		return "gpt6_reserved_clock_bits";
	return 0;
}
static inline int y2_gpt6_adopt(unsigned control, unsigned clock)
{
	return (control & ~2U) == 0x31 && clock == 0;
}
static inline int y2_gpt_rate_matches(unsigned delta,
				      unsigned long long elapsed)
{
	return delta >= 26000 && elapsed * 100 >= delta * 98ULL &&
	       elapsed * 100 <= delta * 102ULL;
}
#endif
