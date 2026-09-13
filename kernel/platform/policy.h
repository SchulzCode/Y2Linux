/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PLATFORM_POLICY_H
#define Y2_PLATFORM_POLICY_H
/* MT6582 BSP sdm_pll_vco_calc_op. Power-of-two arithmetic avoids a 64-bit
 * divide runtime in ARM kernel code; overflow/disabled PLL means unknown. */
static inline unsigned y2_pll_decode(unsigned con0, unsigned con1, unsigned arm)
{
	static const unsigned shift[] = {0, 1, 2, 2};
	unsigned long long rate;
	if (!(con0 & 1))
		return 0;
	rate = 26000000ULL * (con1 & 0x1fffff);
	rate *= 1 + ((con0 >> 19) & 1);
	rate >>= 14 + shift[(con0 >> 4) & 3];
	if (arm)
		rate >>= (con1 >> 24) & 7;
	return rate > 0xffffffffULL ? 0 : (unsigned)rate;
}
/* MT6323 register definitions: upstream include/linux/mfd/mt6323/registers.h.
 * No SET/CLR aliases: regmap RMW retains every bit outside this mask. */
static inline unsigned y2_pmic_write_mask(unsigned reg)
{
	switch (reg) {
	case 0x000: /* Charger owner may only clear CHR_EN/CSDAC_EN (value guard below). */
		return 0x0018;
	case 0x758: /* AUXADC_CON11: voltage buffer, no calibration bits. */
		return 0x0010;
	case 0x76e: /* AUXADC_CON22: BATSNS, ISENSE, BATON1 and PMIC die requests. */
		return 0x00e8;
	case 0x8000: /* RTC BBPU key, permitted only for upstream poweroff. */
		return 0xffff;
	case 0x8004: return 0x000d; /* RTC alarm/one-shot/low-power IRQ enable. */
	case 0x8008: return 0x0010; /* RTC alarm DOW mask. */
	case 0x800a: case 0x800c: case 0x8018: case 0x801a: return 0x003f;
	case 0x800e: case 0x8010: case 0x801c: case 0x801e: return 0x001f;
	case 0x8012: case 0x8020: return 0x0007;
	case 0x8014: case 0x8022: return 0x000f;
	case 0x8016: case 0x8024: return 0x007f;
	case 0x803c: return 0x0001; /* RTC_WRTGR. */
	case 0x160:
	case 0x166:
	case 0x172:
	case 0x174:
		return 0xffff;
	case 0x16c:
		return 0x10;
	case 0x330:
	case 0x338:
	case 0x340:
	case 0x348:
		return 0x1f00;
	case 0x50c: /* MT6323 DIGLDO_CON8: VGP2 enable only. */
		return 0x8000;
	case 0x532: /* DIGLDO_CON29: VGP2 selector only. */
		return 0x00e0;
	case 0x356:
		return 0x000f;
	default:
		return 0;
	}
}
static inline int y2_pmic_value_allowed(unsigned reg, unsigned value)
{
	if (reg == 0 && (value & 0x18)) return 0;
	if (reg == 0x8000 && value != 0x4300) return 0;
	return y2_pmic_write_mask(reg) != 0;
}
/* Command filter precedes any DMA setup. SD CMD6 only changes volatile bus
 * mode; eMMC CMD6 can write persistent EXT_CSD and is deliberately refused. */
static inline int y2_mmc_read_allowed(unsigned op, unsigned embedded, unsigned write, unsigned sbc)
{
	if (write || sbc)
		return 0;
	switch (op) {
	case 0:
	case 1:
	case 2:
	case 3:
	case 7:
	case 8:
	case 9:
	case 10:
	case 12:
	case 13:
	case 16:
	case 17:
	case 18:
	case 41:
	case 51:
	case 55:
		return 1;
	case 6:
		return !embedded;
	default:
		return 0;
	}
}
#endif
