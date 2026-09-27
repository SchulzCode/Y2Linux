/* SPDX-License-Identifier: GPL-2.0-only */
/* MT6582 mt_cirq: 155 inputs correspond to GIC hwirq 64..218.
 * Source: d53dd75c mt_cirq.c. Last bank has only 27 implemented bits. */
#ifndef Y2_CIRQ_POLICY_H
#define Y2_CIRQ_POLICY_H
#define Y2_CIRQ_BANKS 5
static inline unsigned y2_cirq_valid(unsigned bank)
{
	return bank == 4 ? 0x07ffffffU : ~0U;
}
static inline unsigned y2_cirq_sensitivity(unsigned low, unsigned high)
{
	unsigned i, level = 0;
	for (i = 0; i < 32; i++) {
		unsigned cfg = i < 16 ? low : high;
		if (!(cfg & (2U << ((i % 16) * 2)))) level |= 1U << i;
	}
	return level;
}
static inline unsigned y2_cirq_ack_mask(unsigned pending, unsigned enabled,
					 unsigned valid)
{
	/* Preserve an event already pending at the parent when IRQs stopped. */
	return ~(pending & enabled) & valid;
}
#endif
