/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CHARGER_POLICY_H
#define Y2_CHARGER_POLICY_H
/* MT6323 field semantics: MediaTek MT6582 BSP, Copyright (C) 2011-2014
 * MediaTek Inc. Sequence reconciled with the retained Y2 stock kernel.
 * See docs/knowledge/m4-charging.md for exact binary addresses and limits.
 * This is the production sequencer, also exercised by fault-injection tests.
 */
#define Y2_CHARGE_UV 4175000
#define Y2_RECHARGE_UV 4110000
#define Y2_TOPOFF_UV 4050000
/* Actual Y2 LK uses 3.2 V for ordinary boot, and modes 8/9 for charging
 * below that threshold. Stock Linux leaves precharge above 3.4 V. The
 * inherited PMIC UVLO/ULC and pack-presence protections remain mandatory;
 * a software 3.4-V entry veto would prevent this recovery path. */
#define Y2_BOOT_MIN_UV 3200000
#define Y2_PRECHARGE_UV 3400000
#define Y2_PRECHARGE_MAX_SECONDS 10800U /* conservative 3h vs stock 24h total */
#define Y2_CHARGE_MAX_SECONDS 86400U
#define Y2_CV_MAX_SECONDS 10800U
#define Y2_CHARGE_ENGINES 0x18U

struct y2_charge_io {
	void *context;
	int (*read)(void *, unsigned, unsigned *);
	int (*update)(void *, unsigned, unsigned, unsigned, int);
};

static inline int y2_charge_expect(const struct y2_charge_io *io,
				   unsigned reg, unsigned mask, unsigned expected)
{
	unsigned value;
	int ret = io->read(io->context, reg, &value);
	return ret ? ret : (value & mask) == expected ? 0 : -EIO;
}

static inline int y2_charge_set(const struct y2_charge_io *io,
				unsigned reg, unsigned mask, unsigned value)
{
	int ret = io->update(io->context, reg, mask, value, 0);
	return ret ? ret : y2_charge_expect(io, reg, mask, value);
}

static inline int y2_charge_stop(const struct y2_charge_io *io)
{
	int ret, first = 0, attempt;
	/* Never remove the watchdog's last defence until BOTH engines are
	 * confirmed off. A transient failed write gets one bounded retry. */
	for (attempt = 0; attempt < 2; attempt++) {
		ret = io->update(io->context, 0x000, Y2_CHARGE_ENGINES, 0, 1);
		if (!ret) ret = y2_charge_expect(io, 0x000, Y2_CHARGE_ENGINES, 0);
		if (!ret) break;
		if (!first) first = ret;
	}
	if (ret) return ret;
	ret = y2_charge_set(io, 0x01a, 0x001f, 0); /* watchdog off, TD=4s */
	if (!first) first = ret;
	ret = y2_charge_set(io, 0x01e, 0x0003, 0); /* interrupt/flag write off */
	if (!first) first = ret;
	ret = y2_charge_set(io, 0x02e, 0x0040, 0); /* HWCV after engine-off */
	return first ? first : ret;
}

static inline int y2_charge_pet(const struct y2_charge_io *io)
{
	int ret;
	/* Command strobes MUST reach hardware on every pet, even if a read
	 * returns the previous value. regmap_update_bits alone can elide them. */
	ret = io->update(io->context, 0x01a, 0x011f, 0x0110, 1);
	if (!ret) ret = io->update(io->context, 0x01e, 0x0003, 0x0003, 1);
	if (!ret) ret = y2_charge_expect(io, 0x01a, 0x001f, 0x0010);
	if (!ret) ret = y2_charge_expect(io, 0x01e, 0x0005, 0x0001);
	return ret;
}

static inline int y2_charge_protections(const struct y2_charge_io *io)
{
	unsigned ov;
	int ret;
	/* Require the observed board configuration; never clear a protection
	 * or an unknown/test bit in order to pass these checks. */
	ret = y2_charge_expect(io, 0x000, 0x0085, 0x0001); /* HV enabled, no HV fault/automode */
	if (!ret) ret = y2_charge_expect(io, 0x002, 0x00f0, 0x00b0); /* stock 7V input OVP */
	if (!ret) ret = io->read(io->context, 0x00c, &ov);
	/* Retain the inherited OVP selector 0, or stock selector 1 (4.3V).
	 * Selector 0's exact voltage remains unknown: never change it upward. */
	if (!ret && (ov != 0x0001 && ov != 0x0003)) ret = -EIO;
	if (!ret) ret = y2_charge_expect(io, 0x00e, 0xffff, 0x0005); /* BATON/TDET, present */
	if (!ret) ret = y2_charge_expect(io, 0x018, 0xffff, 0); /* charger test/reset */
	if (!ret) ret = y2_charge_expect(io, 0x020, 0xffff, 0x0005); /* normal ADC route/USBDL/UVLO */
	if (!ret) ret = y2_charge_expect(io, 0x03c, 0xffff, 0x0020); /* stock autonomous thermal shutdown */
	if (!ret) ret = y2_charge_expect(io, 0x044, 0x0703, 0); /* no PMIC thermal alarm/test */
	return ret;
}

static inline int y2_charge_regulation(const struct y2_charge_io *io, unsigned selector)
{
	int ret = y2_charge_expect(io, 0x006, 0x001f, 30);
	if (selector != 15 && selector != 12 && selector != 10) return -EINVAL;
	if (!ret) ret = y2_charge_expect(io, 0x008, 0x000f, selector);
	if (!ret) ret = y2_charge_expect(io, 0x004, 0x000e, 0x000e);
	if (!ret) ret = y2_charge_expect(io, 0x028, 0x0077, 0x0021);
	if (!ret) ret = y2_charge_expect(io, 0x02a, 0x0077, 0x0014);
	if (!ret) ret = y2_charge_expect(io, 0x02c, 0x003f, 1);
	if (!ret) ret = y2_charge_expect(io, 0x02e, 0x00c7, 0x00c4);
	return ret;
}

static inline int y2_charge_prepare(const struct y2_charge_io *io, unsigned selector)
{
	unsigned input, cv;
	int ret;
	if (selector != 15 && selector != 12 && selector != 10) return -EINVAL;
	ret = y2_charge_stop(io);
	/* Do not raise an unexpectedly lower inherited protection/CV setting. */
	if (!ret) ret = io->read(io->context, 0x002, &input);
	if (!ret && (input & 0xf0) != 0xf0 && (input & 0xf0) != 0xb0) ret = -EIO;
	if (!ret) ret = io->read(io->context, 0x006, &cv);
	if (!ret && (cv & 0x1f) != 30 && (cv & 0x1f) != 0) ret = -EIO;
	/* Change only reconciled fields, with engines off. Hardware protection
	 * is enabled before arming the charger; OV/UVLO/emergency trims survive. */
	if (!ret) ret = y2_charge_set(io, 0x002, 0x00f0, 0x00b0);
	if (!ret) ret = y2_charge_set(io, 0x03c, 0x0020, 0x0020);
	if (!ret) ret = y2_charge_protections(io);
	if (!ret) ret = y2_charge_set(io, 0x008, 0x000f, selector);
	if (!ret) ret = y2_charge_set(io, 0x006, 0x001f, 30);
	if (!ret) ret = y2_charge_set(io, 0x028, 0x0077, 0x0021);
	if (!ret) ret = y2_charge_set(io, 0x02a, 0x0077, 0x0014);
	if (!ret) ret = y2_charge_set(io, 0x02c, 0x003f, 1);
	if (!ret) ret = y2_charge_set(io, 0x02e, 0x00c4, 0x00c4);
	if (!ret) ret = y2_charge_set(io, 0x004, 0x000a, 0x000a);
	if (!ret) ret = y2_charge_regulation(io, selector);
	if (!ret) ret = y2_charge_pet(io);
	return ret;
}

static inline int y2_charge_start(const struct y2_charge_io *io, unsigned selector)
{
	int ret = y2_charge_protections(io);
	if (!ret) ret = y2_charge_regulation(io, selector);
	if (!ret) ret = y2_charge_expect(io, 0x01a, 0x001f, 0x0010);
	if (!ret) ret = y2_charge_expect(io, 0x01e, 0x0005, 0x0001);
	if (!ret) ret = y2_charge_expect(io, 0x000, 0x00bd, 0x0021);
	if (!ret) ret = y2_charge_set(io, 0x000, 0x0008, 0x0008);
	if (!ret) ret = y2_charge_set(io, 0x000, 0x0010, 0x0010);
	if (!ret) ret = y2_charge_expect(io, 0x000, 0x00bd, 0x0039);
	return ret;
}

/* Timing is elapsed boottime, never a count of workqueue invocations. USB
 * budget changes, manual inhibit and transient pauses cannot reset budgets. */
struct y2_charge_cycle {
	unsigned long long total, cv, precharge;
	unsigned long long confirm_at;
	unsigned confirmations;
	int topoff, hold, full, timed_out;
};

static inline void y2_charge_account(struct y2_charge_cycle *c,
				    unsigned long long elapsed, int uv)
{
	c->total += elapsed;
	if (uv <= Y2_PRECHARGE_UV) c->precharge += elapsed;
	if (uv > Y2_TOPOFF_UV) c->topoff = 1;
	if (c->topoff) c->cv += elapsed;
	if (c->total >= Y2_CHARGE_MAX_SECONDS || c->cv >= Y2_CV_MAX_SECONDS ||
	    c->precharge >= Y2_PRECHARGE_MAX_SECONDS)
		c->timed_out = 1;
}

static inline int y2_charge_termination(struct y2_charge_cycle *c,
				      unsigned long long now, int uv, int cv_detected)
{
	int qualifies = c->hold ? uv < Y2_RECHARGE_UV :
		uv >= Y2_CHARGE_UV && cv_detected;
	if (!qualifies) {
		c->confirmations = 0;
		c->confirm_at = now;
		return 0;
	}
	if (now < c->confirm_at + 10) return 0;
	c->confirm_at = now;
	if (++c->confirmations < 6) return 0;
	c->confirmations = 0;
	c->hold = !c->hold;
	c->full = c->hold; /* only a completed confirmation sequence earns Full */
	if (!c->hold) { c->cv = 0; c->topoff = 0; }
	return 1;
}
#endif
