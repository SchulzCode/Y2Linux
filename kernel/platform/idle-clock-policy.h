/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_IDLE_CLOCK_POLICY_H
#define Y2_IDLE_CLOCK_POLICY_H
/* MSDC SDC_STS[1:0]/DMA_CFG[0], I2C START[0]/APDMA EN[0]. A busy
 * firmware-owned engine remains a real blocker; never clear/reset it. */
static inline int y2_unused_clock_busy(int msdc, unsigned controller,
                                      unsigned engine)
{
	return (controller & (msdc ? 3 : 1)) || (engine & 1);
}
#endif
