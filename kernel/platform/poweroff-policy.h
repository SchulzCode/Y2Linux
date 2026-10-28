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

/* F39: a power-off that stalls before reboot(2) is ended by the SoC watchdog
 * that userspace arms. With AUTO_RESTART clear its reset reaches LK as reason
 * 3, like the charger reset above: charging mode on a charger, else off. Every
 * other bit (EN, the timeout mode) stays as the watchdog driver set it; a
 * later reboot sets AUTO_RESTART again (patch 0037). */
#define Y2_RGU_MODE_AUTO_RESTART 0x10U

static inline unsigned y2_rgu_poweroff_mode(unsigned mode, int powers_off)
{
	mode &= 0xffffU;
	mode = powers_off ? mode & ~Y2_RGU_MODE_AUTO_RESTART : mode | Y2_RGU_MODE_AUTO_RESTART;
	return mode | Y2_RGU_MODE_KEY;
}

/* F50: the BBPU power-down returned (on battery too: 7e52eb4a, 46be199c).
 * Stock mt_power_off never returns, it waits with the power-down asserted
 * and restarts into charging mode while CHRDET is set. Power held keeps the
 * PMIC on and makes LK boot normally, so wait for its release (debounced
 * PWRKEY_DEB set = released, as mtk-pmic-keys reads it), then take the
 * charger reset: LK charges or powers off. Never longer than the bound. */
#define Y2_PMIC_CHRSTATUS 0x142U
#define Y2_CHRSTATUS_PWRKEY_DEB 0x02U
#define Y2_RTC_BBPU 0x8000U /* MT6323 RTC base + RTC_BBPU */
#define Y2_RTC_WRTGR 0x803cU
#define Y2_RTC_BBPU_POWER_DOWN 0x4300U /* RTC_BBPU_KEY, as mt6323-poweroff */
#define Y2_POWEROFF_RELEASED_SECONDS 2U
#define Y2_POWEROFF_LAST_SECONDS 15U

static inline int y2_poweroff_reset_now(unsigned second, unsigned con0, unsigned chrstatus,
					unsigned *released)
{
	if (y2_poweroff_to_charger(con0)) return 1;
	*released = (chrstatus & Y2_CHRSTATUS_PWRKEY_DEB) ? *released + 1 : 0;
	return *released >= Y2_POWEROFF_RELEASED_SECONDS || second >= Y2_POWEROFF_LAST_SECONDS;
}
#endif
