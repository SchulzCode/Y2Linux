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
/* TOPCKGEN+4 is DCM_CFG (AXI bus DCM). Exact MT6582 BSP: dcm init leaves its
 * reset default 0 (DCM_ENABLE_DCM_CFG is undefined), bus_dcm_enable writes
 * 0x8f and bus_dcm_disable clears only bit 7. The stock idle baseline is
 * therefore 0x00 before the first slow/deep idle and 0x0f after it; the Y2
 * board reads 0x00. Bus DCM already enabled or unknown bits belong to another
 * owner and are refused. Entry still writes and verifies 0x8f and restores
 * the exact inherited value. */
#define Y2_BUS_DCM_IDLE 0x8fU
static inline int y2_bus_dcm_baseline(unsigned value)
{
	return value == 0x00 || value == 0x0f;
}
#endif
