/* SPDX-License-Identifier: GPL-2.0-only */
/* MT6582 register definitions adapted from Chris Hendrickson's GPL-2.0
 * mt6582-afe-common.h / mt6582-afe-pcm.c in the pinned donorSource snapshot.
 * DL1 -> I05/I06 -> O00/O01 -> I2S_CON3 is also retained stock HAL evidence.
 * FM capture is the stock HAL's indirect path (AudioFMResourceManager,
 * AudioDigitalControl, AudioInterConnection table of the v3.2.0 FM image):
 * the MT6627's 32 kHz master I2S enters the second I2S input as slave,
 * the I2S ASRC converts it to 48 kHz, I00/I01 -> O05/O06 feed AWB (memif 5).
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
#define AFE_I2S_CON 0x0018
#define AFE_CONN2 0x0028
#define AFE_CONN4 0x0030
#define AFE_I2S_CON3 0x004c
#define AFE_AWB_BASE 0x0070
#define AFE_AWB_END 0x0078
#define AFE_AWB_CUR 0x007c
#define FPGA_CFG1 0x04c4
#define AFE_ASRC_CON0 0x0500
#define AFE_ASRC_CON13 0x0550
#define AFE_ASRC_CON14 0x0554
#define AFE_ASRC_CON15 0x0558
#define AFE_ASRC_CON16 0x055c
#define AFE_ASRC_CON17 0x0560
#define AFE_ASRC_CON20 0x056c
#define AFE_IRQ_CON 0x03a0
#define AFE_IRQ_STATUS 0x03a4
#define AFE_IRQ_CLR 0x03a8
#define AFE_IRQ_CNT1 0x03ac
#define AFE_IRQ_CNT2 0x03b0
#define DL1_ROUTE ((1U << 5) | (1U << 22))
#define DL1_ON (1U << 1)
#define AFE_ON 1U
#define I2S_ON 1U
#define IRQ1 1U
#define IRQ2 2U
/* FM capture: memif enable bit (memif+1), AWB rate/mono in DAC_CON1, the
 * I2S-in word (CON bit31, slave, I2S format, 16-bit), ASRC 32k->48k words
 * (SetI2SASRCConfig, 48 kHz case), I00->O05 / I01->O06 in AFE_CONN2. */
#define AWB_ON (1U << 6)
#define AWB_RATE_SHIFT 12
#define AWB_MONO (1U << 24)
#define I2S_IN_WORD ((1U << 31) | (1U << 3) | (1U << 2))
#define I2S_IN_SLAVE_CLOCK (1U << 8)
#define ASRC_BYPASS (1U << 30)
#define ASRC_ON 0x41U
#define FM_ROUTE ((1U << 16) | (1U << 22))
#define FM_RATE 48000U
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
