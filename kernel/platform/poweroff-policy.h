/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_POWEROFF_POLICY_H
#define Y2_POWEROFF_POLICY_H
/* Power off with a charger attached (F46). Stock mt_power_off (FM kernel
 * 0xc04a1100) restarts while pmic_chrdet_status() is set instead of staying
 * off. The plain power-down would leave CHRDET high with no new plug-in edge,
 * so the PMIC stays off and nothing charges until the cable is re-plugged.
 *
 * The restart is stock arch_reset "charger" (0xc049622c → wd_sw_reset(0)):
 * wdt_arch_reset (0xc0541d10) kicks the counter, clears EN, IRQ, AUTO_RESTART
 * and DUAL (bic 0x59), keeps the external PMIC reset and leaves AUTO_RESTART
 * clear. The preloader then reports BR_WDT (3), not BR_WDT_BY_PASS_PWK (4) as
 * our ordinary restart does, and LK's is_force_boot (0x81e166f4) no longer
 * forces a normal boot: kernel_power_off_charging_detection (0x81e16778) picks
 * KERNEL_POWER_OFF_CHARGING_BOOT (8) while CHRDET is set, or powers off. */
#define Y2_RGU_MODE 0x00
#define Y2_RGU_RESTART 0x08
#define Y2_RGU_SWRST 0x14
#define Y2_RGU_MODE_KEY 0x22000000U
#define Y2_RGU_MODE_EXRST 0x04U
#define Y2_RGU_MODE_CLEAR 0x59U /* EN | IRQ | AUTO_RESTART | DUAL */
#define Y2_RGU_RESTART_KEY 0x1971U
#define Y2_RGU_SWRST_KEY 0x1209U
#define Y2_CHR_CON0_CHRDET 0x20U /* MT6323 CHR_CON0 (0x000) bit 5 */

static inline int y2_poweroff_to_charger(unsigned con0)
{
	return !!(con0 & Y2_CHR_CON0_CHRDET);
}

static inline unsigned y2_rgu_charger_mode(unsigned mode)
{
	return (mode & ~Y2_RGU_MODE_CLEAR) | Y2_RGU_MODE_KEY | Y2_RGU_MODE_EXRST;
}
#endif
