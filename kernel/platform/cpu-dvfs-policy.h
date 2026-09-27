/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CPU_DVFS_POLICY_H
#define Y2_CPU_DVFS_POLICY_H
#include "boot-policy.h"

/* Only the measured Y2 bin is admitted. Other MT6582 bins are not guessed. */
static inline int y2_cpu_bin0(const unsigned char *p, unsigned bytes)
{
	int seen = 0, valid = 0;
	if (bytes < 8 || y2_boot_word(p) != 2 || y2_boot_word(p + 4) != 0x54410001)
		return 0;
	while (bytes >= 8) {
		unsigned words = y2_boot_word(p), tag = y2_boot_word(p + 4);
		if (!words && !tag) return seen && valid;
		if (words < 2 || words > bytes / 4) return 0;
		if (tag == 0x41000804) {
			if (seen++ || words != 25 || y2_boot_word(p + 96) != 22) return 0;
			valid = (y2_boot_word(p + 20) & 3) == 2 &&
				((y2_boot_word(p + 68) >> 28) & 7) == 0;
		}
		p += words * 4; bytes -= words * 4;
	}
	return 0;
}

static inline unsigned y2_cpu_selector(unsigned long hz)
{
	switch (hz) {
	case 598000000: case 747500000: case 1040000000: return 72;
	case 1196000000: return 80;
	case 1300000000: return 88;
	default: return 0;
	}
}

struct y2_dvfs_io {
	void *context;
	int (*voltage_get)(void *);
	int (*voltage_set)(void *, unsigned);
	int (*clock_set)(void *, unsigned long);
	unsigned long (*clock_get)(void *);
	void (*fault)(void *);
};

/* Errors never reduce voltage underneath an unknown or restored old clock. */
static inline int y2_dvfs_transition(const struct y2_dvfs_io *io,
				   unsigned long hz, unsigned long maximum)
{
	unsigned target = y2_cpu_selector(hz), old_target;
	int old, ret;
	if (!target || hz > maximum) return -EINVAL;
	old_target = y2_cpu_selector(io->clock_get(io->context));
	if (!old_target) return -EOPNOTSUPP;
	old = io->voltage_get(io->context);
	if (old < 0) return old;
	if ((old != 72 && old != 80 && old != 88) || (unsigned)old < old_target)
		return -ERANGE;
	if (target > (unsigned)old) {
		ret = io->voltage_set(io->context, target);
		if (ret) goto fault;
	}
	ret = io->clock_set(io->context, hz);
	if (ret) goto fault;
	if (io->clock_get(io->context) != hz) { ret = -EIO; goto fault; }
	if (target < (unsigned)old) {
		ret = io->voltage_set(io->context, target);
		if (ret) {
			/* A failed write is not proof that voltage remained high.
			 * Restore the previous source-backed selector if readback is
			 * below the new OPP or unavailable. Never use unknown readback
			 * as a reason to allow a high clock. */
			int observed = io->voltage_get(io->context);
			if (observed < (int)target) {
				io->voltage_set(io->context, old);
				observed = io->voltage_get(io->context);
				if (observed < (int)target) {
					/* Lowest known clock is the containment path if the
					 * PMIC itself cannot restore a supported selector. */
					io->clock_set(io->context, 598000000);
					io->fault(io->context);
					return -ERANGE;
				}
			}
			io->fault(io->context);
			return 0;
		}
	}
	return 0;
fault:
	io->fault(io->context);
	return ret;
}
#endif
