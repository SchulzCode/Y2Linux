/* SPDX-License-Identifier: GPL-2.0-only */
/* Derived from Copyright (C) 2011-2014 MediaTek Inc., mt_spm{,_sleep}.c.
 * Host protocol for the exact Y2 suspend PCM. CPU and IRQ context belongs
 * to Linux, not to this sequencer. Infrastructure/DDRPHY power is retained.
 * This explicitly supported BSP mode avoids assuming an unported peripheral
 * register restore path. PCM still controls DRAM retention and system clocks. */
#ifndef Y2_SPM_SUSPEND_POLICY_H
#define Y2_SPM_SUSPEND_POLICY_H
#include "spm-regs.h"
#define Y2_SPM_WAKE (WAKE_SRC_EINT | WAKE_SRC_USB_CD | WAKE_SRC_THERM | WAKE_SRC_SYSPWREQ)
#define Y2_SPM_TIMER (600U * 32768U)

static inline void y2_spm_reset_pcm(const struct y2_spm_io *io)
{
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_PCM_SW_RESET);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS);
	io->write(io->context, SPM_PCM_CON1, CON1_CFG_KEY | CON1_SPM_SRAM_ISO_B |
		CON1_SPM_SRAM_SLP_B | CON1_IM_NONRP_EN | CON1_MIF_APBEN);
}
static inline void y2_spm_normal(const struct y2_spm_io *io, unsigned address)
{
	io->write(io->context, SPM_POWERON_CONFIG_SET, 0x0b160001);
	io->write(io->context, SPM_PCM_PWR_IO_EN, 0);
	y2_spm_reset_pcm(io);
	io->write(io->context, SPM_POWER_ON_VAL0, 0);
	io->write(io->context, SPM_POWER_ON_VAL1, 0x15820);
	io->write(io->context, SPM_CLK_CON, CC_CXO32K_RM_EN_MD);
	io->write(io->context, SPM_PCM_SRC_REQ, 2);
	io->write(io->context, SPM_SLEEP_ISR_MASK, ISRM_ALL);
	io->write(io->context, SPM_SLEEP_ISR_STATUS, ISRC_ALL);
	io->write(io->context, SPM_PCM_SW_INT_CLEAR, PCM_SW_INT_ALL);
	io->write(io->context, SPM_PCM_CON1, CON1_CFG_KEY | CON1_SPM_SRAM_ISO_B |
		CON1_SPM_SRAM_SLP_B | CON1_MIF_APBEN);
	io->write(io->context, SPM_PCM_IM_PTR, address);
	io->write(io->context, SPM_PCM_IM_LEN, 27);
	io->write(io->context, SPM_SLEEP_WAKEUP_EVENT_MASK, ~WAKE_SRC_THERM);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_KICK | CON0_PCM_KICK);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY);
}
static inline int y2_spm_suspend_arm(const struct y2_spm_io *io, unsigned address)
{
	unsigned val, settle, n;
	/* Exact Y2 spm_cpusys_can_power_down: both copies must show CPUs1-3 off. */
	if ((io->read(io->context, SPM_PWR_STATUS) | io->read(io->context, SPM_PWR_STATUS_S)) & Y2_SPM_SECONDARY_CPU_MASK)
		return -EBUSY;
	y2_spm_update(io, SPM_CLK_CON, CC_SYSSETTLE_SEL, CC_SYSSETTLE_SEL);
	io->write(io->context, SPM_CLK_SETTLE, 0);
	settle = io->read(io->context, SPM_CLK_SETTLE);
	if (settle > 99) return -ERANGE; /* do not underflow the vendor subtraction */
	io->write(io->context, SPM_CLK_SETTLE, 99 - settle);
	y2_spm_reset_pcm(io);
	io->write(io->context, SPM_PCM_IM_PTR, address);
	io->write(io->context, SPM_PCM_IM_LEN, 596);
	if (io->read(io->context, SPM_PCM_IM_PTR) != address ||
	    io->read(io->context, SPM_PCM_IM_LEN) != 596) return -EIO;
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS | CON0_IM_KICK);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS);
	val = io->read(io->context, SPM_POWER_ON_VAL1);
	io->write(io->context, SPM_POWER_ON_VAL1, val | R7_UART_CLK_OFF_REQ);
	for (n = 0; n < 10; n++) {
		if (io->read(io->context, SPM_PCM_REG13_DATA) & R13_UART_CLK_OFF_ACK) break;
		io->delay(10);
	}
	if (n == 10) {
		io->write(io->context, SPM_POWER_ON_VAL1, val);
		return -EBUSY;
	}
	io->write(io->context, SPM_PCM_REG_DATA_INI, io->read(io->context, SPM_POWER_ON_VAL0));
	io->write(io->context, SPM_PCM_PWR_IO_EN, PCM_RF_SYNC_R0);
	io->write(io->context, SPM_PCM_PWR_IO_EN, 0);
	io->write(io->context, SPM_PCM_REG_DATA_INI, io->read(io->context, SPM_POWER_ON_VAL1));
	io->write(io->context, SPM_PCM_PWR_IO_EN, PCM_RF_SYNC_R7);
	io->write(io->context, SPM_PCM_PWR_IO_EN, 0);
	io->write(io->context, SPM_PCM_REG_DATA_INI, 0);
	io->write(io->context, SPM_PCM_EVENT_VECTOR0, EVENT_VEC(11, 1, 0, 0));
	io->write(io->context, SPM_PCM_EVENT_VECTOR1, EVENT_VEC(12, 1, 0, 26));
	io->write(io->context, SPM_PCM_EVENT_VECTOR2, EVENT_VEC(30, 1, 0, 55));
	io->write(io->context, SPM_PCM_EVENT_VECTOR3, EVENT_VEC(31, 1, 0, 99));
	for (n = SPM_PCM_EVENT_VECTOR4; n <= SPM_PCM_EVENT_VECTOR7; n += 4)
		io->write(io->context, n, 0);
	io->write(io->context, SPM_APMCU_PWRCTL, 0); /* no MT6333 */
	io->write(io->context, SPM_AP_STANBY_CON, (3U << 19) | (1U << 4));
	for (n = SPM_CORE0_WFI_SEL; n <= SPM_CORE3_WFI_SEL; n += 4) io->write(io->context, n, 1);
	io->write(io->context, SPM_PCM_TIMER_VAL, Y2_SPM_TIMER);
	y2_spm_update(io, SPM_PCM_CON1, 0, CON1_CFG_KEY | CON1_PCM_TIMER_EN);
	io->write(io->context, SPM_SLEEP_WAKEUP_EVENT_MASK, ~Y2_SPM_WAKE);
	io->write(io->context, SPM_SLEEP_ISR_MASK, ISRM_PCM_IRQ_AUX | ISR_TWAM);
	/* CPU shutdown, retained infrastructure and DDRPHY. Do not silently
	 * turn an unimplemented infrastructure restore into a power-loss test. */
	y2_spm_update(io, SPM_CLK_CON, CC_DISABLE_DORM_PWR | CC_DISABLE_INFRA_PWR,
		CC_DISABLE_INFRA_PWR | CC_LOCK_INFRA_DCM);
	io->write(io->context, SPM_PCM_MAS_PAUSE_MASK, 0xffffffff);
	io->write(io->context, SPM_PCM_PWR_IO_EN, PCM_PWRIO_EN_R0 | PCM_PWRIO_EN_R7);
	y2_spm_update(io, SPM_CLK_CON, 0, CC_SRCLKENA_MASK);
	io->write(io->context, SPM_PCM_WDT_TIMER_VAL, Y2_SPM_TIMER + 30U * 32768U);
	y2_spm_update(io, SPM_PCM_CON1, CON1_PCM_WDT_WAKE_MODE, CON1_CFG_KEY | CON1_PCM_WDT_EN);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS | CON0_PCM_KICK);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS);
	return 0;
}
static inline void y2_spm_suspend_clean(const struct y2_spm_io *io)
{
	y2_spm_update(io, SPM_PCM_CON1, CON1_PCM_WDT_EN | CON1_PCM_TIMER_EN, CON1_CFG_KEY);
	y2_spm_update(io, SPM_POWER_ON_VAL1, R7_UART_CLK_OFF_REQ, 0);
	y2_spm_update(io, SPM_CLK_CON, CC_SRCLKENA_MASK | CC_LOCK_INFRA_DCM, 0);
	io->write(io->context, SPM_PCM_PWR_IO_EN, 0);
	io->write(io->context, SPM_SLEEP_CPU_WAKEUP_EVENT, 0);
	io->write(io->context, SPM_SLEEP_WAKEUP_EVENT_MASK, ~WAKE_SRC_THERM);
	io->write(io->context, SPM_SLEEP_ISR_MASK, ISRM_ALL);
	io->write(io->context, SPM_SLEEP_ISR_STATUS, ISRC_ALL_EXC_TWAM);
	io->write(io->context, SPM_PCM_SW_INT_CLEAR, PCM_SW_INT0);
}
#endif
