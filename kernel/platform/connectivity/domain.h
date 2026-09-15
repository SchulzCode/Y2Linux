/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_RADIO_DOMAIN_H
#define Y2_RADIO_DOMAIN_H
#include "../spm-policy.h"
/* Called by the existing SPM owner with its shared lock. Bus isolation is
 * established by the CCF owner before power-off and released after power-on.
 * Live-domain protection/ack timeouts propagate. Only the explicitly verified
 * unstarted LK MD state has a separate cold-normalization entry point. */
static inline int y2_radio_domain_sequence(const struct y2_spm_io *io,
					 unsigned domain, int on)
{
	unsigned reg, bit; int ret;
	if (domain > 1) return -EINVAL;
	reg = domain ? 0x284 : 0x280; bit = domain ? 1 : 2;
	io->write(io->context, 0, 0x0b160001);
	if (on) {
		y2_spm_update(io, reg, 4, 4);
		y2_spm_update(io, reg, 8, 8);
		ret = y2_spm_poll(io, 0x60c, bit, bit);
		if (!ret) ret = y2_spm_poll(io, 0x610, bit, bit);
		if (ret) return ret;
		y2_spm_update(io, reg, 0x10, 0);
		y2_spm_update(io, reg, 2, 0);
		y2_spm_update(io, reg, 1, 1);
		y2_spm_update(io, reg, 0x100, 0);
	} else {
		y2_spm_update(io, reg, 0x100, 0x100);
		y2_spm_update(io, reg, 2, 2);
		y2_spm_update(io, reg, 0x11, 0x10);
		y2_spm_update(io, reg, 0xc, 0);
		ret = y2_spm_poll(io, 0x60c, bit, 0);
		if (!ret) ret = y2_spm_poll(io, 0x610, bit, 0);
		if (ret) return ret;
	}
	return 0;
}
#endif
