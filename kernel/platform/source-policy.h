/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SOURCE_POLICY_H
#define Y2_SOURCE_POLICY_H
/* MT6323 BC1.1 sequence: actual Y2 charging_get_charger_type 0xc04cb2a8,
 * reconciled with MediaTek BSP d53dd75c3ff7 pmic_chr_type_det.c.
 * Call only with both charger engines stopped and the USB PHY exclusively
 * acquired, after the data controller has disconnected. */
enum y2_source_type {
	Y2_SOURCE_UNKNOWN, Y2_SOURCE_SDP, Y2_SOURCE_CDP,
	Y2_SOURCE_NONSTANDARD, Y2_SOURCE_DCP, Y2_SOURCE_APPLE,
};
struct y2_bc11_io {
	struct y2_charge_io pmic;
	/* Sleep, then verify fresh CHRDET. Removal must abort every stage. */
	int (*wait_source)(void *, unsigned ms);
};

static inline int y2_bc11_clear(const struct y2_bc11_io *io)
{
	int ret = y2_charge_set(&io->pmic, 0x026, 0x01ff, 0);
	int second = y2_charge_set(&io->pmic, 0x024, 0x000c, 0);
	return ret ? ret : second;
}

static inline int y2_bc11_step(const struct y2_bc11_io *io,
			      unsigned con24, unsigned con26, int *high)
{
	unsigned value;
	/* Clear the preceding comparator/pull configuration before changing
	 * VSRC, as the stock per-stage cleanup does. Keep BIAS on between steps. */
	int ret = y2_charge_set(&io->pmic, 0x026, 0x00ff, 0);
	if (!ret) ret = y2_charge_set(&io->pmic, 0x024, 0x000c, con24);
	if (!ret) ret = y2_charge_set(&io->pmic, 0x026, 0x01ff, con26 | 0x0100);
	if (!ret) ret = io->wait_source(io->pmic.context, 80);
	if (!ret) ret = io->pmic.read(io->pmic.context, 0x024, &value);
	if (!ret) *high = !!(value & 0x0080);
	return ret;
}

static inline int y2_bc11_detect(const struct y2_bc11_io *io, enum y2_source_type *type)
{
	int ret, cleanup, high;
	*type = Y2_SOURCE_UNKNOWN;
	ret = io->wait_source(io->pmic.context, 300);
	if (!ret) ret = y2_bc11_clear(io);
	if (!ret) ret = y2_charge_set(&io->pmic, 0x026, 0x0100, 0x0100);
	if (!ret) ret = y2_charge_set(&io->pmic, 0x024, 0x0003, 0x0003);
	if (!ret) ret = io->wait_source(io->pmic.context, 50);
	/* DCD: IPU=2, IPD=1, VREF=1, CMP=2. */
	if (!ret) ret = y2_bc11_step(io, 0, 0x99, &high);
	if (!ret && high) {
		/* A1: distinguish the stock Apple signature from nonstandard. */
		ret = y2_bc11_step(io, 0, 0x14, &high);
		if (!ret) *type = high ? Y2_SOURCE_APPLE : Y2_SOURCE_NONSTANDARD;
	} else if (!ret) {
		/* A2: VSRC=2, IPD=1, VREF=0, CMP=1. */
		ret = y2_bc11_step(io, 0x08, 0x14, &high);
		if (!ret && !high) *type = Y2_SOURCE_SDP;
		else if (!ret) {
			/* B2: IPU=2, VREF=1, CMP=1. */
			ret = y2_bc11_step(io, 0, 0x85, &high);
			if (!ret) *type = high ? Y2_SOURCE_DCP : Y2_SOURCE_CDP;
		}
	}
	cleanup = y2_bc11_clear(io);
	if (!ret) ret = cleanup;
	if (ret) *type = Y2_SOURCE_UNKNOWN;
	return ret;
}

/* Stock Y2 `select_charging_curret` (0xc03a1d88) request per detected BSP
 * charger type. Detection (0xc04cb2a8) only ever stores types 1 STANDARD_HOST,
 * 2 CHARGING_HOST, 3 NONSTANDARD, 4 STANDARD_CHARGER and 5 APPLE_2_1A:
 *   SDP 500 (the PMIC floors it to 450), CDP 650, DCP 1100, nonstandard 1100,
 *   Apple 800, unknown 70 mA.
 * This is the vendor *request*. What may actually be programmed is
 * y2_source_class_ceiling_ua() and the battery/thermal limits in charge-limits.h.
 * A negative allocation means no data controller has been attached (70 mA).
 * POWER-FINAL-16 (Defect C, archive 109): like stock, an SDP requests 500 mA
 * whatever the gadget negotiated. A powered-off PC keeps VBUS on a suspended
 * bus; the USB 2.0 suspend allowance (2 mA, rounded to 0) left the Y2 neither
 * charging nor sleeping for 6.3 h (75 % -> 32 %). The stock firmware never
 * consulted the negotiated budget (`select_charging_curret` 0xc03a1d88).
 */
static inline unsigned y2_source_stock_request_ua(enum y2_source_type type, int allocated_ua)
{
	switch (type) {
	case Y2_SOURCE_CDP: return 650000;
	case Y2_SOURCE_DCP:
	case Y2_SOURCE_NONSTANDARD: return 1100000;
	case Y2_SOURCE_APPLE: return 800000;
	case Y2_SOURCE_SDP:
		return allocated_ua < 0 ? 70000 : 500000;
	default: return 70000;
	}
}

/* Highest current this Linux policy lets a source class draw. Equal to the
 * stock request except nonstandard: BC1.2 defines no capability for a source
 * whose data lines are neither shorted (DCP) nor pulled (SDP/CDP) and Linux has
 * no validated VBUS-collapse check. F38 (cb904ffb, e7604e73): at 70 mA such a
 * charger (dumb adapters, charge-only cables) carried less than the awake load
 * and the player drained while "Charging". It gets what any USB 2.0 port gives
 * an SDP here (450 mA), not the stock 1100 mA, until one has been qualified. */
static inline unsigned y2_source_class_ceiling_ua(enum y2_source_type type)
{
	switch (type) {
	case Y2_SOURCE_NONSTANDARD: return 450000;
	case Y2_SOURCE_UNKNOWN: return 70000;
	case Y2_SOURCE_SDP: return 500000;
	case Y2_SOURCE_CDP: return 650000;
	case Y2_SOURCE_DCP: return 1100000;
	case Y2_SOURCE_APPLE: return 800000;
	}
	return 70000;
}
#endif
