/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_UART_IDLE_POLICY_H
#define Y2_UART_IDLE_POLICY_H
#include "spm-policy.h"

/* Exact MT6582 UART startup: non-DMA ports enable DPIDLE by their clock
 * bit and set UART_SLEEP_EN. The global SPM ACK, not LSR, admits sleep.
 * This narrow owner adopts only unowned UART1; it never resets or gates it. */
#define Y2_UART1_CLOCK (1U << 17)
#define Y2_DPIDLE_PERI_MASK (0x02fe87fdU | 0x7800U)
#define Y2_UART_SLEEP_EN 0x48U
struct y2_uart_idle_save {
	unsigned sleep, lcr, ier, dma, prepared, sampled;
};
static inline unsigned y2_dpidle_peri_blockers(unsigned pdn, unsigned deferred)
{
	/* Only explicitly adopted UART1 may defer to the SPM handshake. */
	return ~pdn & Y2_DPIDLE_PERI_MASK & ~(deferred & Y2_UART1_CLOCK);
}
static inline int y2_uart_idle_ready(const struct y2_spm_io *io,
		struct y2_uart_idle_save *s)
{
	s->prepared = 0;
	s->sampled = 0;
	s->sleep = s->ier = s->dma = 0;
	s->lcr = io->read(io->context, 0x0c);
	s->sampled = 1;
	if (s->lcr & ~0x7fU) return -EBUSY; /* bank aliases or unknown bits */
	s->ier = io->read(io->context, 0x04);
	s->dma = io->read(io->context, 0x4c);
	s->sampled = 7;
	if (s->ier || (s->dma & ~4U)) return -EBUSY;
	s->sleep = io->read(io->context, Y2_UART_SLEEP_EN);
	s->sampled = 15;
	return s->sleep > 1 ? -EBUSY : 0;
}
static inline int y2_uart_idle_prepare(const struct y2_spm_io *io,
		struct y2_uart_idle_save *s)
{
	int ret = y2_uart_idle_ready(io, s);
	if (ret) return ret;
	s->prepared = 1; /* unwind even a dropped/ambiguous sleep-enable write */
	if (!s->sleep) io->write(io->context, Y2_UART_SLEEP_EN, 1);
	return io->read(io->context, Y2_UART_SLEEP_EN) == 1 ? 0 : -EIO;
}
static inline int y2_uart_idle_restore(const struct y2_spm_io *io,
		struct y2_uart_idle_save *s)
{
	int ret = 0;
	if (!s->prepared) return 0;
	if (!s->sleep) io->write(io->context, Y2_UART_SLEEP_EN, s->sleep);
	if (io->read(io->context, Y2_UART_SLEEP_EN) != s->sleep ||
	    io->read(io->context, 0x0c) != s->lcr ||
	    io->read(io->context, 0x04) != s->ier ||
	    io->read(io->context, 0x4c) != s->dma) ret = -EIO;
	s->prepared = 0;
	return ret;
}
#endif
