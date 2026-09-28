/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_MSDC_RPM_POLICY_H
#define Y2_MSDC_RPM_POLICY_H
/* MT6582 MSDC runtime clock ownership. Register provenance: pinned MT6582
 * BSP mt_sd.h. Upstream msdc_save_reg also accesses PATCH_BIT2 (0xb8),
 * PAD_DS/CMD_TUNE (0x188/0x18c), EMMC50_CFG0/3 and SDC_FIFO_CFG, which do not
 * exist on MT6582. Stock sd.c msdc_clksrc_onoff gates only the PERI MSDC CG,
 * selecting MSDC_CFG MODE=MS before the gate and SD/MMC after 10 us, then
 * waits for CKSTB. PERI clock gating does not remove register power; the
 * explicit save/verify/restore below proves retention instead of assuming it. */
#define Y2_MSDC_CFG 0x00
#define Y2_MSDC_INT 0x0c
#define Y2_MSDC_INTEN 0x10
#define Y2_MSDC_FIFOCS 0x14
#define Y2_SDC_STS 0x3c
#define Y2_MSDC_DMA_CFG 0x9c
#define Y2_MSDC_CFG_MODE (1U << 0)
#define Y2_MSDC_CFG_RST (1U << 2)
#define Y2_MSDC_CFG_BV18PSS (1U << 6)	/* read only */
#define Y2_MSDC_CFG_CKSTB (1U << 7)	/* read only */
#define Y2_MSDC_CFG_VOLATILE (Y2_MSDC_CFG_RST | Y2_MSDC_CFG_BV18PSS | Y2_MSDC_CFG_CKSTB)
#define Y2_MSDC_RETAINED 12
/* MSDC_CFG first: it is always rewritten on resume (MODE and clock). */
static const unsigned short y2_msdc_retained[Y2_MSDC_RETAINED] = {
	0x00, /* MSDC_CFG */
	0x04, /* MSDC_IOCON */
	0x10, /* MSDC_INTEN */
	0x30, /* SDC_CFG */
	0xb0, /* MSDC_PATCH_BIT0 */
	0xb4, /* MSDC_PATCH_BIT1 */
	0xe0, /* MSDC_PAD_CTL0 */
	0xe4, /* MSDC_PAD_CTL1 */
	0xe8, /* MSDC_PAD_CTL2 */
	0xec, /* MSDC_PAD_TUNE */
	0xf0, /* MSDC_DAT_RDDLY0 */
	0xf4, /* MSDC_DAT_RDDLY1 */
};
enum y2_msdc_rpm_busy {
	Y2_MSDC_IDLE,
	Y2_MSDC_BUSY_REQUEST,
	Y2_MSDC_BUSY_DMA,
	Y2_MSDC_BUSY_CONTROLLER,
	Y2_MSDC_BUSY_FIFO,
	Y2_MSDC_BUSY_INTERRUPT,
	Y2_MSDC_BUSY_REASONS,
};
static const char *const y2_msdc_busy_names[Y2_MSDC_BUSY_REASONS] = {
	"idle", "request", "dma", "controller", "fifo", "interrupt",
};
/* Every live owner blocks the gate. Nothing here resets or clears state. */
static inline enum y2_msdc_rpm_busy y2_msdc_rpm_busy(int request, unsigned dma_cfg,
						     unsigned sdc_sts, unsigned fifocs,
						     unsigned status, unsigned enabled)
{
	if (request)
		return Y2_MSDC_BUSY_REQUEST;
	if (dma_cfg & 1)
		return Y2_MSDC_BUSY_DMA;
	if (sdc_sts & 3)	/* SDCBUSY | CMDBUSY */
		return Y2_MSDC_BUSY_CONTROLLER;
	if (fifocs & 0x00ff00ffU)	/* RXCNT | TXCNT */
		return Y2_MSDC_BUSY_FIFO;
	if (status & enabled)
		return Y2_MSDC_BUSY_INTERRUPT;
	return Y2_MSDC_IDLE;
}
/* Saved MSDC_CFG to rewrite after ungating: never replay RST. */
static inline unsigned y2_msdc_cfg_restore(unsigned saved)
{
	return saved & ~Y2_MSDC_CFG_VOLATILE;
}
/* Bit i set when retained register i differs from its saved value. The
 * resume path always rewrites MSDC_CFG, so index 0 is not compared. */
static inline unsigned y2_msdc_mismatch(const unsigned *saved, const unsigned *now)
{
	unsigned i, mask = 0;
	for (i = 1; i < Y2_MSDC_RETAINED; i++)
		if (saved[i] != now[i])
			mask |= 1U << i;
	return mask;
}
#endif
