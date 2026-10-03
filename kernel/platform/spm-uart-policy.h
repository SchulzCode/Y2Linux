/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SPM_UART_POLICY_H
#define Y2_SPM_UART_POLICY_H
#include "spm-policy.h"
#include "spm-regs.h"
struct y2_spm_uart_sleep {
	unsigned attempts, successes, timeouts, restore_failures;
	unsigned power_before, power_request, power_after, r13_before, r13_ack;
	unsigned request, ack;
	int result;
};
static inline int y2_spm_uart_request(const struct y2_spm_io *io,
		struct y2_spm_uart_sleep *s)
{
	unsigned n;
	s->attempts++;
	s->request = s->ack = 0;
	s->power_before = io->read(io->context, SPM_POWER_ON_VAL1);
	s->power_request = s->power_before;
	s->r13_before = io->read(io->context, SPM_PCM_REG13_DATA);
	s->r13_ack = s->r13_before;
	s->power_after = s->power_before;
	if (s->power_before & R7_UART_CLK_OFF_REQ) return s->result = -EBUSY;
	io->write(io->context, SPM_POWER_ON_VAL1,
		s->power_before | R7_UART_CLK_OFF_REQ);
	s->power_request = io->read(io->context, SPM_POWER_ON_VAL1);
	s->request = !!(s->power_request & R7_UART_CLK_OFF_REQ);
	if (s->power_request != (s->power_before | R7_UART_CLK_OFF_REQ)) {
		s->result = -EIO;
		goto unwind;
	}
	/* Stock WAIT_UART_ACK_TIMES=10: initial read plus ten 10us waits. */
	for (n = 0; n <= 10; n++) {
		s->r13_ack = io->read(io->context, SPM_PCM_REG13_DATA);
		if (s->r13_ack & R13_UART_CLK_OFF_ACK) {
			s->ack = 1;
			s->successes++;
			return s->result = 0;
		}
		if (n < 10) io->delay(10);
	}
	s->timeouts++;
	s->result = -EBUSY; /* exact stock WR_UART_BUSY; never enter without ACK */
unwind:
	io->write(io->context, SPM_POWER_ON_VAL1, s->power_before);
	s->power_after = io->read(io->context, SPM_POWER_ON_VAL1);
	if (s->power_after != s->power_before) {
		s->restore_failures++;
		s->result = -EIO;
	}
	return s->result;
}
#endif
