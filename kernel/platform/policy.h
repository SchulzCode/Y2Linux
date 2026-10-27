/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PLATFORM_POLICY_H
#define Y2_PLATFORM_POLICY_H
#include "charge-table.h"
#include "vproc-sleep-policy.h"
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
	case 0x000: /* Production charger engines; OVP/automode remain immutable. */
		return 0x0018;
	case 0x002: return 0x00f0; /* Only the stock 7V input OVP selector. */
	case 0x004: return 0x000a; /* Current sense + CV comparators. */
	case 0x006: return 0x001f; /* Only 4.175V CV (value guard below). */
	case 0x008: return 0x000f; /* CS_VTH: only table steps up to the stock 1100 mA maximum (y2_cs_selector_permitted). */
	case 0x00c: return 0x000f; /* Stock 4.3V battery OVP, switched on; status immutable. */
	case 0x01a: return 0x011f; /* Charger watchdog: 4s, enable, service. */
	case 0x01e: return 0x0003; /* Charger watchdog interrupt/ack, no OUT write. */
	case 0x024: return 0x000f; /* BC1.1 BB/RST/VSRC, comparator read-only. */
	case 0x026: return 0x01ff; /* BC1.1 bias/comparator/current sources. */
	case 0x028: case 0x02a: return 0x0077; /* Stock CSDAC step parameters. */
	case 0x02c: return 0x003f; /* Stock low-current debounce. */
	case 0x02e: return 0x00c4; /* CSDAC mode, HWCV, ULC; never disable ULC. */
	case 0x03c: return 0x0020; /* Enable stock thermal shutdown, never disable. */
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
	/* Vendor PMIC_INIT_SETTING_V1 LP_SEL (bit 0 only) of the always-on LDOs: VA,
	 * VIO28, VUSB, VM, VIO18. Enable, selector and mode bits stay unwritable;
	 * VEMC_3V3, VMC and VMCH LP_SEL are deliberately not admitted.
	 * Owner-authorized 2026-10-08 (POWER-FINAL-05). */
	case 0x404: case 0x500: case 0x502: case 0x552: case 0x556:
		return 0x0001;
	/* POWER-FINAL-13: the stock E2 PMIC_INIT_SETTING_V1 also sets LP_SEL on the
	 * storage LDOs: VEMC_3V3 0x508 (LP_SEL only, its enable stays unwritable);
	 * VMC 0x504 and VMCH 0x506 below add bit 0 to their SD enable bits. */
	case 0x508: return 0x0001;
	/* POWER-FINAL-13 stock VPROC sleep fields (vproc-sleep-policy.h), set only
	 * inside platform prepare_late..wake and restored exactly afterwards. The
	 * buck enables (0x21a/0x240) and software VOSEL 0x21e stay unwritable; VSYS
	 * mode fields follow below (27), its selectors never. */
	case 0x212: return 0x0030; /* QI_VPROC_VSLEEP */
	case 0x216: return 0x0002; /* VPROC_VOSEL_CTRL only */
	case 0x21c: return 0x80ff; /* VPROC_SFCHG_FRATE/FEN/REN */
	case 0x220: return 0x007f; /* VPROC_VOSEL_ON: 72/80/88 only */
	case 0x222: return 0x007f; /* VPROC_VOSEL_SLEEP: 0.85..1.25 V */
	case 0x230: return 0x0103; /* VPROC_TRANSTD, VPROC_VSLEEP_EN */
	case 0x50c: /* MT6323 DIGLDO_CON8: VGP2 enable only. */
		return 0x8000;
	case 0x532: /* DIGLDO_CON29: VGP2 selector only. */
		return 0x00e0;
	/* #28 vibration motor (regulator-haptic on VIBR), the stock vibrator's
	 * only fields: DIGLDO_CON39 enable + the stock thermal shutdown (set once
	 * by pmic-lowpower.c, never cleared), DIGLDO_CON40 selector 1.8..2.8 V.
	 * Keep both DT bounds admitted: at registration the regulator core moves
	 * an out-of-range VIBR to 1.8 or 2.8 V, and a refusal there would fail
	 * the whole mt6323-regulator probe (SD, audio and radio rails with it). */
	case 0x542: return 0x8004;
	case 0x544: return 0x00e0;
	/* SD rails as stock msdc_sd_power/msdc_sd_power_switch drive them
	 * (docs/validation/Y2-STORAGE-CEILING.md). The VEMC_3V3 (root eMMC)
	 * enable stays unwritable; only its LP_SEL bit is admitted above. */
	case 0x504: return 0x1001; /* DIGLDO_CON3: VMC (SD IO) enable + LP_SEL. */
	case 0x506: return 0x4001; /* DIGLDO_CON5: VMCH (SD card) enable + LP_SEL. */
	case 0x52a: return 0x0010; /* DIGLDO_CON24: VMC 1.8/3.3 V selector. */
	case 0x52c: return 0x0080; /* DIGLDO_CON26: VMCH selector, 3.3 V only. */
	case 0x356:
		return 0x000f;
	/* Stock connectivity clock/rail fields. No charger, VPROC, RTC spare or
	 * calibration trim write is added for M5. Regulator selectors stay 3.3V. */
	case 0x102: return 0x8841; /* + 18 rg_clksq_en_aud (pmic-sleep-policy.h) */
	/* POWER-FINAL-18 stock suspend fields (pmic-sleep-policy.h), written only
	 * inside the SPM enter callback and restored exactly before any resume. */
	case 0x108: return 0x1440; /* drv_2m, rtc_mclk, efuse clock PDN */
	case 0x10e: return 0x0020; /* auxadc_ctl clock PDN */
	case 0x70c: return 0x0010; /* rg_adacck_en */
	case 0x776: case 0x778: return 0x8000; /* rg_vref18_enb(_md) */
	/* POWER-FINAL-27 VSYS buck sleep state (pmic-sleep-policy.h group 8); the
	 * VSYS enable and every VSYS voltage selector (0x244..0x24a) stay closed. */
	case 0x238: return 0x0030; /* QI_VSYS_VSLEEP */
	case 0x23c: return 0x0002; /* VSYS_VOSEL_CTRL only */
	case 0x242: return 0xff7f; /* VSYS_SFCHG_FRATE/RRATE/REN */
	case 0x256: return 0x0133; /* VSYS_TRANSTD/VOSEL_TRANS_EN/VSLEEP_EN */
	case 0x120: return 0x0030;
	case 0x148: return 0x000a;
	case 0x402: return 0x0801;
	case 0x416: return 0x00ac;
	case 0x418: return 0x5000;
	case 0x41c: return 0x5000;
	case 0x512: return 0x4002;
	default:
		return 0;
	}
}
static inline int y2_pmic_value_allowed(unsigned reg, unsigned value)
{
	if (reg == 0 && (value & 0x18) && ((value & 0x85) != 1)) return 0;
	if (reg == 0x002 && (value & 0xf0) != 0xb0) return 0;
	if (reg == 0x004 && (value & 0xa) != 0xa) return 0;
	if (reg == 0x006 && (value & 0x1f) != 30) return 0;
	if (reg == 0x008 && !y2_cs_selector_permitted(value & 0xf)) return 0;
	if (reg == 0x00c && (value & 0x000f) != 3) return 0;
	if (reg == 0x01a && (value & 0xf)) return 0;
	if (reg == 0x028 && (value & 0x77) != 0x21) return 0;
	if (reg == 0x02a && (value & 0x77) != 0x14) return 0;
	if (reg == 0x02c && (value & 0x3f) != 1) return 0;
	if (reg == 0x02e && !(value & 0x80)) return 0;
	if (reg == 0x03c && !(value & 0x20)) return 0;
	if (reg == 0x542 && !(value & 0x4)) return 0; /* VIBR thermal shutdown stays on */
	/* VIBR 1.8/2.0/2.8 V (selectors 3..5); stock runs it at 2.8 V. */
	if (reg == 0x544 && (((value >> 5) & 7) < 3 || ((value >> 5) & 7) > 5)) return 0;
	if (reg == 0x8000 && value != 0x4300) return 0;
	if (reg == 0x416 && (value & 0xc)) return 0;
	if (reg == 0x52c && !(value & 0x80)) return 0; /* VMCH never 3.0 V */
	if (!y2_vproc_value_allowed(reg, value)) return 0;
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
