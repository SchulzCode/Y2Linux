# Y2Linux 1.0 Beta 8.40 (Baseline 154)

Status: **PUBLISHED as GitHub release `baseline-154`** (2026-10-27), not installed on a player before publishing
(owner: "do the fix and push as 8.40"). Release `1.0.0-beta.8.40`, Reborn `1.0.0-beta.8.40`, kernel
`6.18.56-y2linux-1.0.0-beta.8.40`, update sequence 10500, root `2026.08-platform-v1.196`, Linux `1b09963`, Reborn
`77c54e2`. Built with `tools/production/release.py` (107 s, resumed workspace, update based on `baseline-153`):
container suite **885 OK**, host suite 912 OK; all seal checks PASS, no MediaTek firmware in any image. Milestone
audit BETA-8.40 in [roadmap-gap-audit](../planning/roadmap-gap-audit.md).

| Image | SHA-256 |
| --- | --- |
| BOOTIMG | `f5c6992d9fd9b38a6ee1578d77415ad85b0295f48ee7429f8fc395a9fac470c9` |
| Y2ROOT | `313305ef9dfcee77a98b80603283737ed5434b224885311a765df55bae135e0a` |
| RECOVERY | `15baf379bed46d094ecb695ccd417965c46acfa3c21eb49f0833d539d15dd2da` |
| Y2DATA (no SSH key) | `1464783324f7644909ab887de0f4d660ffa03607ec65fb36a3ba6d7b001a53b2` |

## What changed since Beta 8.39

| Change | Detail | Evidence |
| --- | --- | --- |
| Power off on a charger opens the charging screen (ledger F46, install 02f46e9f) | Before, Power Off with a cable plugged in switched the player fully off and it never charged until the cable was plugged in again: the MT6323 power-down left the charger detected with no new plug-in to wake it. Now, as on stock, a charger at power-off restarts the player into the bootloader's charging mode, which shows the Y2Linux charging screen; unplugging there switches off, a Power hold starts the player. Without a charger, power off is unchanged | report 149a9d81 (off at 40 % while charging, 40 % again 39 min later); fleet power journals (74 → 74 % after 14 h, 71 → 71 % after 12 h, 57 → 57 % after 9 h off on a charger); REA below; `tests/test_poweroff_charger.py` |

### How the restart reaches the charging screen

- Stock `mt_power_off` (FM kernel 0xc04a1100) loops on `pmic_chrdet_status()` and calls `arch_reset`; the stock
  `charger` restart (0xc049622c → `wd_sw_reset(0)` → `wdt_arch_reset` 0xc0541d10) clears EN, IRQ, AUTO_RESTART
  and DUAL in WDT_MODE (bic 0x59), keeps the external PMIC reset and resets the SoC.
- The stock preloader reports such a reset as "SW reset without bypass power key flag", i.e. boot reason 3. Our
  ordinary restart keeps AUTO_RESTART set (patch 0037), which gives reason 4.
- Stock LK `is_force_boot` (0x81e166f4) forces a normal boot only for an alarm, META, a held Power key, reason 4 or
  reason 6. Otherwise `kernel_power_off_charging_detection` (0x81e16778) picks mode 8 while a charger is present,
  or powers off. Mode 8 is Y2Linux's offline charging (`y2-offline-charge`).
- Kernel: `y2_charger_power_off` (`kernel/platform/mt6323-charger.c`) is a high-priority power-off handler. It
  runs after every driver's shutdown, so charging is already inhibited. It reads CHR_CON0 bit 5 and, with a charger,
  writes the stock reset (`kernel/platform/poweroff-policy.h`). Otherwise it does nothing and the BBPU power-off
  follows. The charger node references the RGU (`innioasis,rgu = <&rgu>`). It maps the RGU without claiming it,
  since the watchdog driver owns it.

No platform userspace, Reborn (identity only), memory map, partition or packaging change.

## Open

- Device check: Power Off on a charger, expect the charging screen and the next start's
  `Y2BOOT: loader metadata valid=1 mode=8 reason=3`; unplug on that screen must still switch off.
- The reporter (02f46e9f) also sees a Rockbox boot picture before Y2Linux. Y2Linux never writes LOGO or UBOOT, and
  that player's LK is not the stock v3.2.0 one (prefix SHA-256 `bd001af2…`). Advice: flash the stock `logo.bin` to
  LOGO only.
