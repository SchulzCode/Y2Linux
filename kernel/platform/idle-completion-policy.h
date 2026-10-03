/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_IDLE_COMPLETION_POLICY_H
#define Y2_IDLE_COMPLETION_POLICY_H
#ifdef __KERNEL__
#include <linux/math64.h>
#endif
/* Exact MT6582: 13 MHz GPT, stock minimum 26000 ticks (2 ms).
 * Linux owns programming GPT4. These helpers validate its real handoff;
 * they never reprogram a second clockevent or round a past deadline up. */
#define Y2_DORMANT_MIN_TICKS 26000U
static inline int y2_dormant_opp(unsigned khz)
{
	return khz == 598000 || khz == 747500;
}
static inline unsigned long long y2_gpt_ticks_ns(unsigned ticks)
{
#ifdef __KERNEL__
	return div_u64((u64)ticks * 1000, 13);
#else
	return (unsigned long long)ticks * 1000 / 13;
#endif
}
static inline int y2_dormant_gpt_ready(unsigned control, unsigned clock,
		unsigned irq_enable, unsigned irq_pending, unsigned count,
		unsigned compare)
{
	/* One-shot, 13 MHz, armed IRQ, no already pending/expired wake. */
	return (control & ~2U) == 0x01 && !clock &&
		(irq_enable & 8) && !(irq_pending & 8) && compare > count &&
		compare - count >= Y2_DORMANT_MIN_TICKS;
}
#endif
