/* SPDX-License-Identifier: GPL-2.0-only */
/* Sequence derived from Copyright (C) 2011-2014 MediaTek Inc.
 * Exact FM Y2 spm_mtcmos_ctrl_cpu{1,2,3}; CPU0/cluster are PCM-owned.
 * The caller serializes all SPM operations and owns the MMIO window. */
#ifndef Y2_SPM_POLICY_H
#define Y2_SPM_POLICY_H
/* Stock FC1/FC2/FC3 descend in PWR_STATUS; CPU3 physically clears bit 9. */
#define Y2_SPM_SECONDARY_CPU_MASK 0x00000e00U
static inline unsigned y2_spm_cpu_status_mask(unsigned cpu)
{
	switch (cpu) {
	case 1: return 0x800;
	case 2: return 0x400;
	case 3: return 0x200;
	default: return 0;
	}
}
struct y2_spm_io {
	void *context;
	unsigned (*read)(void *, unsigned);
	void (*write)(void *, unsigned, unsigned);
	void (*delay)(unsigned);
};
static inline void y2_spm_update(const struct y2_spm_io *io, unsigned reg,
				 unsigned mask, unsigned value)
{
	io->write(io->context, reg, (io->read(io->context, reg) & ~mask) | value);
}
static inline int y2_spm_poll(const struct y2_spm_io *io, unsigned reg,
			      unsigned mask, unsigned value)
{
	unsigned n;
	for (n = 0; n < 10000; n++) {
		if ((io->read(io->context, reg) & mask) == value) return 0;
		io->delay(1);
	}
	return -ETIMEDOUT;
}
static inline int y2_spm_cpu_power(const struct y2_spm_io *io, unsigned cpu, int on)
{
	unsigned power, sram, bit;
	int ret;
	if (cpu < 1 || cpu > 3) return -EINVAL;
	power = 0x214 + cpu * 4; sram = 0x25c + cpu * 8;
	bit = y2_spm_cpu_status_mask(cpu);
	io->write(io->context, 0, 0x0b160001);
	if (!on) {
		/* Never isolate a running CPU. Its architectural die hook must
		 * have flushed L1 and left SMP coherency before this WFI status. */
		ret = y2_spm_poll(io, 0x720, 1U << (15 + cpu), 1U << (15 + cpu));
		if (ret) return ret;
		y2_spm_update(io, power, 0x20, 0x20); /* SRAM_CKISO */
		y2_spm_update(io, power, 0x40, 0); /* SRAM_ISOINT_B */
		y2_spm_update(io, sram, 1, 1);
		ret = y2_spm_poll(io, sram, 0x100, 0x100);
		if (ret) return ret;
		y2_spm_update(io, power, 2, 2); /* isolation */
		y2_spm_update(io, power, 0x11, 0x10); /* reset, clock disabled */
		y2_spm_update(io, power, 4, 0);
		y2_spm_update(io, power, 8, 0);
		ret = y2_spm_poll(io, 0x60c, bit, 0);
		if (!ret) ret = y2_spm_poll(io, 0x610, bit, 0);
		return ret;
	}
	y2_spm_update(io, power, 4, 4);
	io->delay(1); /* stock __const_udelay(0x1a36e), ARM HZ=100: 1 us */
	y2_spm_update(io, power, 8, 8);
	ret = y2_spm_poll(io, 0x60c, bit, bit);
	if (!ret) ret = y2_spm_poll(io, 0x610, bit, bit);
	if (ret) return ret;
	y2_spm_update(io, power, 0x10, 0);
	y2_spm_update(io, sram, 1, 0);
	ret = y2_spm_poll(io, sram, 0x100, 0);
	if (ret) return ret;
	y2_spm_update(io, power, 0x40, 0x40);
	y2_spm_update(io, power, 0x20, 0);
	y2_spm_update(io, power, 2, 0);
	y2_spm_update(io, power, 1, 1); /* reset release only after both rails/SRAM */
	return 0;
}
#endif
