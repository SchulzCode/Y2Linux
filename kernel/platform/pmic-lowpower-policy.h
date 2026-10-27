/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PMIC_LOWPOWER_POLICY_H
#define Y2_PMIC_LOWPOWER_POLICY_H
/* Vendor MT6582 BSP PMIC_INIT_SETTING_V1 (pmic_mt6323.c, chip >= E2,
 * "20130604_0.85v") sets bit 0 (xxx_LP_SEL) of these LDO control registers:
 * VA 0x404, VIO28 0x500, VUSB 0x502, VM 0x552, VIO18 0x556. VTCXO (0x402) is
 * already set by the connectivity subset. LP_SEL selects the LDO low-power mode
 * only while the SoC releases SRCLKEN, so an awake system sees no change.
 * POWER-FINAL-13 completes the stock E2 list (the Y2 FM kernel's own init,
 * 0xc04b81f8): VMC 0x504, VMCH 0x506 and VEMC_3V3 0x508 (eMMC supply; stock
 * removes it entirely in suspend, this keeps it on in low-power mode only).
 * No enable, voltage or clock-control bit is touched here; the VPROC sleep
 * voltage is a separate, suspend-only owner (vproc-sleep-policy.h). */
#define Y2_PMIC_LP_BIT 0x0001U
/* #28: the same stock init sets VIBR_THER_SHDN_EN (DIGLDO_CON39 bit 2, golden
 * row vibr_ther_shen_en). Set once with LP_SEL and never cleared (policy.h);
 * the lp_sel switch does not revert it. */
#define Y2_PMIC_VIBR_REG 0x542U
#define Y2_PMIC_VIBR_THR 0x0004U
#define Y2_PMIC_CID_REG 0x100U
#define Y2_PMIC_CID_E2 0x2023U
struct y2_pmic_lp_rail {
	unsigned reg;
	const char *name;
};
static const struct y2_pmic_lp_rail y2_pmic_lp_rails[] = {
	{0x404, "VA"}, {0x500, "VIO28"}, {0x502, "VUSB"}, {0x552, "VM"}, {0x556, "VIO18"},
	{0x504, "VMC"}, {0x506, "VMCH"}, {0x508, "VEMC3V3"},
};
#define Y2_PMIC_LP_RAILS (sizeof(y2_pmic_lp_rails) / sizeof(y2_pmic_lp_rails[0]))
/* The vendor E2 branch is the verified source for this register map. */
static inline int y2_pmic_lp_chip_ok(unsigned cid)
{
	return cid >= Y2_PMIC_CID_E2;
}
/* Accept exactly "0" or "1" (optional trailing newline). */
static inline int y2_pmic_lp_parse(const char *text, unsigned long length)
{
	if (length && text[length - 1] == '\n')
		length--;
	if (length != 1 || (text[0] != '0' && text[0] != '1'))
		return -1;
	return text[0] == '1';
}
#endif
