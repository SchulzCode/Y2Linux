/* SPDX-License-Identifier: GPL-2.0-only */
/* Conversion/OPP definitions derived from Copyright (C) 2011-2014 MediaTek Inc. */
#ifndef Y2_POWER_MATH_H
#define Y2_POWER_MATH_H
/* MT6582 BSP d53dd75c3ff7: mtk_ts_{cpu,pmic}.c, mt_cpufreq.c and
 * charging_hw_pmic.c. Keep vendor integer rounding; never use default fuses.
 * Provenance, units and limitations: docs/knowledge/m4-power-platform.md. */
struct y2_pmic_cal { int intercept, divisor; };
static inline int y2_pmic_calibrate(unsigned cid, unsigned a, unsigned b,
				  struct y2_pmic_cal *cal)
{
	int slope, id, vts, vbe;
	if (!(a & 2) || (cid != 0x1023 && cid != 0x2023 && cid != 0x3023))
		return -1;
	vts = ((b & 31) << 8) | (a >> 8);
	if (cid == 0x1023) { id = (b >> 12) & 1; slope = (b >> 6) & 63; }
	else { id = (b >> 14) & 1; slope = (((b >> 11) & 7) << 3) | ((b >> 6) & 7); }
	if (!id) slope = 0;
	cal->divisor = -(171 + ((b & 32) ? -slope : slope));
	vbe = -((vts + 9102) * 1800 / 32768) * 1000;
	cal->intercept = vbe * 100 / cal->divisor + ((a >> 2) & 63) * 500;
	return 0;
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
	default: return 0;
	}
}
static inline int y2_charge_ua(unsigned selector)
{
	static const int ua[16] = {1600000,1500000,1400000,1300000,1200000,1100000,
		1000000,900000,800000,700000,650000,550000,450000,300000,200000,70000};
	return ua[selector & 15];
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
