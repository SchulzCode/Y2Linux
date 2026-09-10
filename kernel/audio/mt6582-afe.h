/* SPDX-License-Identifier: GPL-2.0-only */
/* MT6582 register definitions adapted from Chris Hendrickson's GPL-2.0
 * mt6582-afe-common.h / mt6582-afe-pcm.c in the pinned donorSource snapshot.
 * DL1 -> I05/I06 -> O00/O01 -> I2S_CON3 is also retained stock HAL evidence.
 */
#ifndef MT6582_AFE_H
#define MT6582_AFE_H
#define AUDIO_TOP_CON0 0x0000
#define AFE_DAC_CON0 0x0010
#define AFE_DAC_CON1 0x0014
#define AFE_CONN0 0x0020
#define AFE_DL1_BASE 0x0040
#define AFE_DL1_CUR 0x0044
#define AFE_DL1_END 0x0048
#define AFE_I2S_CON3 0x004c
#define AFE_IRQ_CON 0x03a0
#define AFE_IRQ_STATUS 0x03a4
#define AFE_IRQ_CLR 0x03a8
#define AFE_IRQ_CNT1 0x03ac
#define DL1_ROUTE ((1U << 5) | (1U << 22))
#define DL1_ON (1U << 1)
#define AFE_ON 1U
#define I2S_ON 1U
#define IRQ1 1U
#define PCM_MAX_BYTES (256U * 1024)
/* Pure bounds/rate helpers shared with the targeted host tests. */
static inline int mt6582_rate_code(unsigned rate)
{
	return rate == 44100 ? 9 : rate == 48000 ? 10 : -1;
}
static inline int mt6582_dma_valid(unsigned long long addr, unsigned bytes)
{
	return bytes >= 1024 && bytes <= PCM_MAX_BYTES && !(addr & 15) &&
	       !(bytes & 15) && addr <= 0xffffffffULL &&
	       addr + bytes - 1 <= 0xffffffffULL;
}
static inline unsigned mt6582_pointer_bytes(unsigned base, unsigned cur, unsigned bytes)
{
	/* CUR may be zero before the first fetch. Reject stale/out-of-buffer DMA. */
	return cur && cur >= base && cur - base < bytes ? (cur - base) & ~3U : 0;
}
#endif
