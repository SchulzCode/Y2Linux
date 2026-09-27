/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SPM_IDLE_POLICY_H
#define Y2_SPM_IDLE_POLICY_H
#include "spm-suspend-policy.h"
static inline int y2_spm_idle_arm(const struct y2_spm_io *io, unsigned address)
{
	unsigned val, n;
	/* Exact Y2 spm_cpusys_can_power_down: both copies must show CPUs1-3 off. */
	if ((io->read(io->context, SPM_PWR_STATUS) | io->read(io->context, SPM_PWR_STATUS_S)) & Y2_SPM_SECONDARY_CPU_MASK)
		return -EBUSY;
	y2_spm_reset_pcm(io);
	io->write(io->context, SPM_PCM_IM_PTR, address);
	io->write(io->context, SPM_PCM_IM_LEN, 479);
	if (io->read(io->context, SPM_PCM_IM_PTR) != address ||
	    io->read(io->context, SPM_PCM_IM_LEN) != 479) return -EIO;
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
	io->write(io->context, SPM_PCM_EVENT_VECTOR1, EVENT_VEC(12, 1, 0, 9));
	io->write(io->context, SPM_PCM_EVENT_VECTOR2, EVENT_VEC(30, 1, 0, 28));
	io->write(io->context, SPM_PCM_EVENT_VECTOR3, EVENT_VEC(31, 1, 0, 68));
	for (n = SPM_PCM_EVENT_VECTOR4; n <= SPM_PCM_EVENT_VECTOR7; n += 4)
		io->write(io->context, n, 0);
	io->write(io->context, SPM_APMCU_PWRCTL, 1U << 6); /* no MT6333 */
	io->write(io->context, SPM_AP_STANBY_CON, (3U << 19) | (1U << 4));
	for (n = SPM_CORE0_WFI_SEL; n <= SPM_CORE3_WFI_SEL; n += 4) io->write(io->context, n, 1);
	io->write(io->context, SPM_PCM_TIMER_VAL, 0xfff0ffffU);
	y2_spm_update(io, SPM_PCM_CON1, 0, CON1_CFG_KEY | CON1_PCM_TIMER_EN);
	io->write(io->context, SPM_SLEEP_WAKEUP_EVENT_MASK, ~(Y2_SPM_WAKE | WAKE_SRC_GPT | WAKE_SRC_AFE | WAKE_SRC_USB_PDN | WAKE_SRC_CIRQ));
	io->write(io->context, SPM_SLEEP_ISR_MASK, ISRM_PCM_IRQ_AUX | ISR_TWAM);
	/* CPU shutdown, retained infrastructure and DDRPHY. Do not silently
	 * turn an unimplemented infrastructure restore into a power-loss test. */
	y2_spm_update(io, SPM_CLK_CON, CC_DISABLE_DORM_PWR | CC_DISABLE_INFRA_PWR,
		CC_DISABLE_INFRA_PWR | CC_LOCK_INFRA_DCM);
	io->write(io->context, SPM_PCM_MAS_PAUSE_MASK, 0xffffffff);
	io->write(io->context, SPM_PCM_PWR_IO_EN, PCM_PWRIO_EN_R0 | PCM_PWRIO_EN_R7);
	y2_spm_update(io, SPM_CLK_CON, 0, CC_SRCLKENA_MASK);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS | CON0_PCM_KICK);
	io->write(io->context, SPM_PCM_CON0, CON0_CFG_KEY | CON0_IM_SLEEP_DVS);
	return 0;
}
#endif
