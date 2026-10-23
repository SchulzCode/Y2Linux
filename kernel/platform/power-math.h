/* SPDX-License-Identifier: GPL-2.0-only */
/* Conversion/OPP definitions derived from Copyright (C) 2011-2014 MediaTek Inc. */
#ifndef Y2_POWER_MATH_H
#define Y2_POWER_MATH_H
#include "charge-table.h"
/* MT6582 BSP d53dd75c3ff7: mtk_ts_{cpu,pmic}.c, mt_cpufreq.c and
 * charging_hw_pmic.c. Keep vendor integer rounding; default fuses only where
 * the vendor uses them (PMIC die without its calibration fuse, below).
 * Provenance, units and limitations: docs/knowledge/m4-power-platform.md. */
struct y2_pmic_cal { int intercept, divisor; };
/* Without the calibration-enable fuse, pmic_cali_prepare() uses its nominal
 * VTS 3698 at 25 C, slope 0. Nominal can be about 10 C off (our own E2 fuses
 * read 8 C low through it), so it reads 10 C high: derating and the 150 C
 * stops act early, never late. */
#define Y2_PMIC_NOMINAL_VTS 3698
#define Y2_PMIC_NOMINAL_DEGC_HALF 50
#define Y2_PMIC_NOMINAL_MARGIN_MC 10000
/* 0: own fuses; 1: nominal (calibration fuse clear); -1: unknown PMIC. */
static inline int y2_pmic_calibrate(unsigned cid, unsigned a, unsigned b,
				  struct y2_pmic_cal *cal)
{
	int slope, id, vts, vbe, degc_half, margin = 0, nominal = !(a & 2);
	if (cid != 0x1023 && cid != 0x2023 && cid != 0x3023)
		return -1;
	vts = ((b & 31) << 8) | (a >> 8);
	degc_half = (a >> 2) & 63;
	if (cid == 0x1023) { id = (b >> 12) & 1; slope = (b >> 6) & 63; }
	else { id = (b >> 14) & 1; slope = (((b >> 11) & 7) << 3) | ((b >> 6) & 7); }
	if (!id) slope = 0;
	if (nominal) {
		vts = Y2_PMIC_NOMINAL_VTS; degc_half = Y2_PMIC_NOMINAL_DEGC_HALF;
		slope = 0; margin = Y2_PMIC_NOMINAL_MARGIN_MC;
	}
	cal->divisor = -(171 + ((b & 32) ? -slope : slope));
	vbe = -((vts + 9102) * 1800 / 32768) * 1000;
	cal->intercept = vbe * 100 / cal->divisor + degc_half * 500 + margin;
	return nominal;
}
static inline int y2_pmic_mc(const struct y2_pmic_cal *cal, unsigned raw)
{
	int mv = raw * 1800 / 32768;
	return cal->intercept + 100000 * mv / cal->divisor;
}
static inline unsigned y2_battery_uv(unsigned raw)
{
	return ((unsigned long long)raw * 7200000) >> 15;
}
struct y2_soc_cal { int gain, offset, room[2], degc, divisor; };
static inline int y2_soc_calibrate(unsigned a, unsigned b, struct y2_soc_cal *c)
{
	int slope = (a >> 26) & 63;
	unsigned vts[] = { (a >> 17) & 511, (a >> 8) & 511 };
	int i;
	if (!(a & 1) || (a == 0xffffffffU && b == 0xffffffffU)) return -1;
	/* Reject the observed byte-read truncation, never convert it to Celsius. */
	if (!(a & 0x03ffff00U) || !(b & 0xfffff000U)) return -1;
	if (!(b & (1 << 9))) slope = 0;
	c->gain = 10000 + (((int)(b >> 22) - 512) * 10000) / 4096;
	c->offset = (int)((b >> 12) & 1023) - 512;
	c->degc = (a >> 1) & 63;
	c->divisor = 165 + ((a & 128) ? -slope : slope);
	for (i = 0; i < 2; i++)
		c->room[i] = (((int)vts[i] + 3350 - c->offset) * 10000 / 4096) * 10000 / c->gain;
	return 0;
}
static inline int y2_soc_mc(const struct y2_soc_cal *c, unsigned raw, unsigned sensor)
{
	int delta = (((int)raw - c->offset) * 10000 / 4096) * 10000 / c->gain - c->room[sensor];
	delta = delta * 15 / 18;
	return (c->degc * 10 / 2 - delta * 100 / c->divisor) * 100;
}
static inline unsigned y2_cpu_pcw(unsigned long hz)
{
	switch (hz) {
	case 598000000: return 0x810b8000;
	case 747500000: return 0x810e6000;
	case 1040000000: return 0x800a0000;
	case 1196000000: return 0x800b8000;
	case 1300000000: return 0x800c8000;
	default: return 0;
	}
}
/* POWER-FINAL-28: the battery voltage the MT6323 latched at power-on, before
 * any boot load (rg_adc_out_wakeup_pchr 0x724[14:0], bit 15 ready). Stock
 * get_hw_ocv 0xc04cae90 (oam_init command 9): raw * 7200 / 32768 + 8 mV
 * (offset at 0xc0a01d28). 0 when not ready or implausible. */
static inline int y2_hw_ocv_uv(unsigned reg)
{
	int mv;
	if (!(reg & 0x8000)) return 0;
	mv = (int)(((reg & 0x7fff) * 7200U) >> 15) + 8;
	return mv < 3000 || mv > 4400 ? 0 : mv * 1000;
}
/* ISENSE-BATSNS drop to charger-path current. The BSP uses CUST_R_SENSE 200 mOhm
 * for the internal MT6323 linear charger (68 mOhm only with an external
 * FAN5405/BQ24158), and the Y2 drop is 98.3 mV at the 450 mA and 139.6 mV at the
 * 650 mA selector, flat across 3.70..4.17 V, about 0.21 ohm. The stock Y2 binary
 * compiles 68, so this is an ESTIMATE of the charger path only (not net battery
 * current, not USB input current) and stays unvalidated without an external
 * meter. Zero when the drop does not exceed the zero-current offset. */
#define Y2_ISENSE_MOHM 200
static inline int y2_isense_current_ua(int drop_uv, int offset_uv)
{
	int net = drop_uv - offset_uv;
	return net > 0 ? (int)((long long)net * 1000 / Y2_ISENSE_MOHM) : 0;
}
static inline int y2_charge_ua(unsigned selector)
{
	return y2_cs_ua(selector);
}
static inline int y2_charge_uv(unsigned selector)
{
	static const int uv[32] = {4200000,4212500,4225000,4237500,4250000,4262500,
		4275000,4300000,4325000,4350000,4375000,4400000,4425000,4162500,
		4175000,2200000,4050000,4100000,4125000,3775000,3800000,3850000,
		3900000,4000000,4050000,4100000,4125000,4137500,4150000,4162500,
		4175000,4187500};
	return uv[selector & 31];
}
#endif
