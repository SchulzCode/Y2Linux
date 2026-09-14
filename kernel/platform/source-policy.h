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

/* Retained Y2 ceilings: configured SDP 450, CDP 650, DCP/nonstandard 1100,
 * detected Apple 800 mA. This production policy limits dedicated charging
 * sources to 650 mA and treats nonstandard signatures conservatively.
 * USB's negotiated allowance applies only to SDP; DCP/CDP do not need an OS
 * gadget. A negative allocation means no data controller has been attached.
 * Once SDP enumeration starts, reset/suspend's zero allowance stops charging.
 */
static inline unsigned y2_source_charge_ua(enum y2_source_type type, int allocated_ua)
{
	if (type == Y2_SOURCE_CDP || type == Y2_SOURCE_DCP || type == Y2_SOURCE_APPLE)
		return 650000;
	if (type == Y2_SOURCE_SDP && allocated_ua >= 0) {
		if (allocated_ua >= 500000) return 450000;
		if (allocated_ua >= 100000) return 70000;
		return 0;
	}
	return 70000;
}

static inline int y2_charge_selector(unsigned ua)
{
	switch (ua) {
	case 70000: return 15;
	case 450000: return 12;
	case 650000: return 10;
	default: return -EINVAL;
	}
}
#endif
