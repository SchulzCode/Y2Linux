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
/* Exact MT6582 platform_uart.h: DMA_EN[1:0] enables RX/TX DMA;
 * bit2 only resets its timeout counter. Keep every other unknown bit busy.
 * IER, DLAB, RX data, incomplete TX and line/FIFO errors remain blockers. */
static inline int y2_unused_uart_busy(unsigned lcr, unsigned ier,
		unsigned lsr, unsigned dma)
{
	return (lcr & 0x80) || ier || (dma & ~4U) || lsr != 0x60;
}
static inline int y2_unused_nfi_busy(unsigned control, unsigned status,
		unsigned master, unsigned fifo)
{
	return (control & 0x310) || (status & 0x1f0f030f) ||
		(master & 0xffff) || (fifo & 0x1f1f);
}
static inline int y2_unused_spi_busy(unsigned command, unsigned status)
{
	/* Exact d53/3be93a68/krillin MT6582 spi_is_busy: STATUS1[0]=1
	 * means idle. Zero is busy, not a quiet reset default. Reject reset,
	 * pause, ACT/RESUME, DMA and unknown status; do not read STATUS0,
	 * whose read acknowledges an interrupt. */
	return (command & 0xc17) || status != 1;
}
#endif
