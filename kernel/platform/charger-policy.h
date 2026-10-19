/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CHARGER_POLICY_H
#define Y2_CHARGER_POLICY_H
#include "charge-table.h"
/* MT6323 field semantics: MediaTek MT6582 BSP, Copyright (C) 2011-2014
 * MediaTek Inc. Sequence reconciled with the retained Y2 stock kernel.
 * See docs/knowledge/m4-charging.md for exact binary addresses and limits.
 * This is the production sequencer, also exercised by fault-injection tests.
 */
#define Y2_CHARGE_UV 4175000
#define Y2_CHARGE_SAFETY_UV 4200000
/* Software termination stops at the first loaded sample at 4.175 V, so the
 * pack relaxes to about 4.09-4.10 V (measured on the Y2: every 70 s hold/restart
 * loop for a day, never Full, with the stock 4.11 V recharge rule). A hold that
 * ends the active cycle is therefore Full when it rests at or above
 * Y2_FULL_REST_UV for the confirmation window; below it the charge is not
 * finished and charging resumes. After Full the maintenance cycle restarts
 * only below Y2_RECHARGE_UV, never at the stock 4.11 V. */
#define Y2_FULL_REST_UV 4070000
#define Y2_RECHARGE_UV 4000000
#define Y2_TOPOFF_UV 4050000
/* POWER-FINAL-28 (ledger F31): stock tops off at hardware CV until the
 * charging current is at most 150 mA for six 10-s samples (BAT_TopOffModeAction
 * 0xc03a2208, batt_cust_data 0xc09d2d68), computed as (ISENSE - BATSNS +
 * offset) * 1000 / 68 (battery_meter_get_charging_current): a 10.2-mV net
 * sense drop, whatever the true sense resistance. Y2 keeps its 4.175 V target
 * and 4.2 V guard: a 4.175 V sample at a higher current only steps the cycle
 * down (finish stage, see 29 below); at 70 mA the cycle completes on 4.175 V,
 * on the stock taper rule or on a plateau. Stock recharges below 4.110 V; Y2 does
 * so too, but never within 30 mV of the voltage the last Full rested at (the
 * 70-s restart loop seen when the rest was below 4.11 V). */
#define Y2_FINISH_UA 70000U
#define Y2_TAPER_DROP_UV 10200
/* POWER-FINAL-29: 28 on the device (archive 126) stepped 450 -> 70 mA and the
 * pack fell back to 4.09 V: an awake Y2 on USB draws about what 70 mA supplies
 * (ISENSE est. 90 mA, screen off). The finish now steps 450/650 -> 200 -> 70 mA
 * at successive 4.175 V samples, and at 70 mA a voltage that has not risen
 * 5 mV in 10 min completes the cycle (the pack is as full as an awake Y2 gets
 * it); Full still needs the >= 4.07 V rest. As stock BAT_TopOffModeAction, the
 * 3-h top-off budget ends a finishing cycle as complete, not as a fault. */
#define Y2_FINISH_STEP_UA 200000U
#define Y2_PLATEAU_RISE_UV 5000
#define Y2_PLATEAU_SECONDS 600
#define Y2_STOCK_RECHARGE_UV 4110000
#define Y2_RECHARGE_MARGIN_UV 30000
/* Actual Y2 LK uses 3.2 V for ordinary boot, and modes 8/9 for charging
 * below that threshold. Stock Linux leaves precharge above 3.4 V. The
 * inherited PMIC UVLO/ULC and pack-presence protections remain mandatory;
 * a software 3.4-V entry veto would prevent this recovery path. */
#define Y2_BOOT_MIN_UV 3200000
#define Y2_PRECHARGE_UV 3400000
/* The configured 500 mA host profile maps to the MT6323's 450 mA selector. Keep
 * that bounded profile during deep-discharge recovery (charge-limits.h). */
#define Y2_DEEP_DISCHARGE_MAX_UA 450000U
#define Y2_PRECHARGE_MAX_SECONDS 10800U /* conservative 3h vs stock 24h total */
#define Y2_CHARGE_MAX_SECONDS 86400U
#define Y2_CV_MAX_SECONDS 10800U
#define Y2_CHARGE_ENGINES 0x18U

struct y2_charge_io {
	void *context;
	int (*read)(void *, unsigned, unsigned *);
	int (*update)(void *, unsigned, unsigned, unsigned, int);
	void (*mismatch)(void *, unsigned, unsigned, unsigned, unsigned, int);
};

static inline int y2_charge_expect(const struct y2_charge_io *io,
				   unsigned reg, unsigned mask, unsigned expected)
{
	unsigned value = 0;
	int ret = io->read(io->context, reg, &value);
	if (!ret && (value & mask) != expected) ret = -EIO;
	if (ret && io->mismatch)
		io->mismatch(io->context, reg, mask, expected, value, ret);
	return ret;
}

/* An inherited value the sequence will not adopt. Recorded like a failed
 * check (BAT0 charging_state check_*, expected 0 = "one of several"), so a
 * report names the register instead of a bare -EIO (#47). */
static inline int y2_charge_refuse(const struct y2_charge_io *io,
				   unsigned reg, unsigned mask, unsigned value)
{
	if (io->mismatch)
		io->mismatch(io->context, reg, mask, 0, value, -EIO);
	return -EIO;
}

/* An inherited CV the sequence lowers to 4.175 V (selector 30): from stock's
 * 4.2 V (0, also the PMIC reset value), 4.1875 V (31, found on a player with
 * the same LK and pack, #47) or 4.175 V (14, 30). A lower one was chosen on
 * purpose and is never raised. */
static inline int y2_cv_adoptable(unsigned cv)
{
	switch (cv & 0x1f) {
	case 0: case 14: case 30: case 31: return 1;
	default: return 0;
	}
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
	/* Separate emergency OVP from normal CV. The actual Y2 kernel's
	 * charging_hw_init selects 1 (4.3V), with detection still enabled. */
	if (!ret && ov != 0x0003) ret = y2_charge_refuse(io, 0x00c, 0xffff, ov);
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
	if (!y2_cs_selector_permitted(selector)) return -EINVAL;
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
	unsigned input, cv, ov;
	int ret;
	if (!y2_cs_selector_permitted(selector)) return -EINVAL;
	ret = y2_charge_stop(io);
	/* Do not raise an unexpectedly lower inherited protection/CV setting. */
	if (!ret) ret = io->read(io->context, 0x002, &input);
	if (!ret && (input & 0xf0) != 0xf0 && (input & 0xf0) != 0xb0)
		ret = y2_charge_refuse(io, 0x002, 0x00f0, input);
	if (!ret) ret = io->read(io->context, 0x006, &cv);
	if (!ret && !y2_cv_adoptable(cv)) ret = y2_charge_refuse(io, 0x006, 0x001f, cv);
	if (!ret) ret = io->read(io->context, 0x00c, &ov);
	/* Only adopt the observed boot setting or the exact stock setting.
	 * Never clear an asserted detector, disable OVP or accept a higher trim. */
	if (!ret && ov != 1 && ov != 3) ret = y2_charge_refuse(io, 0x00c, 0xffff, ov);
	/* Change only reconciled fields, with engines off. Hardware protection
	 * is enabled before arming the charger; OV/UVLO/emergency trims survive. */
	if (!ret) ret = y2_charge_set(io, 0x002, 0x00f0, 0x00b0);
	if (!ret) ret = y2_charge_set(io, 0x03c, 0x0020, 0x0020);
	if (!ret) ret = y2_charge_set(io, 0x00c, 0x000e, 0x0002);
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
	unsigned long long confirm_at, taper_at;
	unsigned confirmations, taper_samples;
	int topoff, hold, full, timed_out, completing, confirm_low;
	/* 28: inputs set by the caller each run (0 = unknown, 26 behaviour) and
	 * the finish stage / last Full rest voltage they drive. */
	unsigned charge_ua;
	int net_drop_uv, full_rest_uv;
	/* 29: finish = current cap of the finish stage in uA (0: none). */
	int finish, plateau_uv, topoff_done;
	unsigned long long plateau_at;
};

static inline int y2_recharge_uv(const struct y2_charge_cycle *c)
{
	int uv = Y2_STOCK_RECHARGE_UV;
	if (!c->full_rest_uv) return Y2_RECHARGE_UV;
	if (c->full_rest_uv - Y2_RECHARGE_MARGIN_UV < uv)
		uv = c->full_rest_uv - Y2_RECHARGE_MARGIN_UV;
	return uv < Y2_RECHARGE_UV ? Y2_RECHARGE_UV : uv;
}

static inline void y2_charge_account(struct y2_charge_cycle *c,
				    unsigned long long elapsed, int uv)
{
	c->total += elapsed;
	if (uv <= Y2_PRECHARGE_UV) c->precharge += elapsed;
	if (uv > Y2_TOPOFF_UV) c->topoff = 1;
	if (c->topoff) c->cv += elapsed;
	if (c->total >= Y2_CHARGE_MAX_SECONDS || c->precharge >= Y2_PRECHARGE_MAX_SECONDS)
		c->timed_out = 1;
	if (c->cv >= Y2_CV_MAX_SECONDS) {
		if (c->finish) c->topoff_done = 1;
		else c->timed_out = 1;
	}
}

static inline int y2_charge_termination(struct y2_charge_cycle *c,
				      unsigned long long now, int uv, int active)
{
	int low = uv < y2_recharge_uv(c), resume;
	/* Voltage-limited termination: stop at the target on the FIRST sample.
	 * POWER-02 waited for CV confirmations while still feeding the battery,
	 * then hit the independent 4.2V fault. Never wait above the target or
	 * raise that safety guard to accommodate ADC/comparator disagreement.
	 * Full denotes completion of this conservative cycle, not measured SOC
	 * or a taper-current claim. CV status remains observable separately. */
	if (!c->hold) {
		int finishing = c->finish && active && c->charge_ua && c->charge_ua <= Y2_FINISH_UA;
		if (c->topoff_done && c->finish) {
			/* Stock: the 3-h top-off budget completes the cycle. */
		} else if (uv < Y2_CHARGE_UV) {
			if (!finishing) {
				c->taper_samples = 0;
				return 0;
			}
			if (!c->plateau_uv || uv >= c->plateau_uv + Y2_PLATEAU_RISE_UV) {
				c->plateau_uv = uv;
				c->plateau_at = now;
			}
			if (now < c->plateau_at + Y2_PLATEAU_SECONDS) {
				/* Stock taper rule, only in the 70 mA finish stage. */
				if (c->net_drop_uv > Y2_TAPER_DROP_UV) {
					c->taper_samples = 0;
					return 0;
				}
				if (c->taper_samples && now < c->taper_at + 10) return 0;
				c->taper_at = now;
				if (++c->taper_samples < 6) return 0;
			}
		} else if (active && c->charge_ua > Y2_FINISH_UA) {
			/* A target sample above 70 mA steps the finish stage down. */
			c->finish = c->charge_ua > Y2_FINISH_STEP_UA ? Y2_FINISH_STEP_UA : Y2_FINISH_UA;
			c->taper_samples = 0;
			c->plateau_uv = 0;
			return 1;
		}
		c->taper_samples = 0;
		c->plateau_uv = 0;
		c->hold = 1;
		c->full = 0;
		c->completing = active;
		c->confirm_low = 0;
		c->confirmations = 0;
		c->confirm_at = now;
		return 1;
	}
	/* Resume when a completing hold rests below the full level, or when any
	 * other hold falls below the maintenance recharge level. */
	resume = c->completing ? uv < Y2_FULL_REST_UV : low;
	if (resume != c->confirm_low) {
		c->confirm_low = resume;
		c->confirm_at = now;
		c->confirmations = 0;
	}
	if ((!resume && (!c->completing || c->full)) || uv >= Y2_CHARGE_SAFETY_UV) {
		c->confirmations = 0;
		c->confirm_at = now;
		return 0;
	}
	if (now < c->confirm_at + 10) return 0;
	c->confirm_at = now;
	if (++c->confirmations < 6) return 0;
	c->confirmations = 0;
	/* An actually completed cycle may start a fresh maintenance cycle.
	 * Mere pauses/holds cannot repeatedly evade the charging time budget. */
	if (resume && c->full) { c->total = 0; c->precharge = 0; }
	c->hold = !resume;
	c->full = !resume;
	c->completing = 0;
	if (c->full) c->full_rest_uv = uv;
	if (resume) { c->cv = 0; c->topoff = 0; c->finish = 0; c->topoff_done = 0; }
	return 1;
}
#endif
