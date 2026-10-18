# Y2Linux 1.0 Beta 8.4 (Baseline 90)

Status: **PUBLISHED as GitHub release `baseline-90`** (2026-10-18). Release `1.0.0-beta.8.4`, Reborn
`1.0.0-beta.8.4` (identity only), kernel `6.18.56-y2linux-1.0.0-beta.8.4`, update sequence 6700, Linux `c339919`,
Reborn `000876f`. Built with `tools/production/release.py`: container suite **775 OK**, host suite 802 OK; all seal
checks PASS, no MediaTek firmware in any image. **Not yet run on a player:** the owner installs it over the air.
Milestone audit BETA-8.4 in [roadmap-gap-audit](../planning/roadmap-gap-audit.md).

| Image | SHA-256 |
| --- | --- |
| BOOTIMG | `71ad38769b9aeb91b25936c0e3708625f94fd71f3fb5c54d2598737581ad298e` |
| Y2ROOT | `d2436b2ce9e1cbfcffdf124060395ff14d7701edd46e61d5ee9fa25b131b5606` |
| RECOVERY | `b59d0acda12604e5751e45de21dca0a840cc1b6ec614b6f7f855e3c999c5c6fd` |
| Y2DATA (no SSH key) | `fec46f0f919ea3bd53df2f9072b0c73405ed6fb48ad08bee6d4e8dc0b1fcfb7f` |

## The dark display after a restart

The owner's over-the-air update to Beta 8.3 restarted from the recovery into a dark display; Reborn never drew a
frame and the health check rolled the update back ([Beta 8.3 report](Y2-LINUX-1.0-BETA-8.3.md)). Four restarts from
the recovery that day, logs in `out/dsi-lag-2026-10-18/` (local):

| Boot | `holding SMI common/larb0` at mm-clocks probe (0.36 s) | Handover snapshot | Display |
| --- | --- | --- | --- |
| 6500 install, 6501 install, rollback (8.2) | yes | larb0 `stat=0 greq=0` | lit |
| 6600 install (8.3) | **no** | `SMI gates 1 left closed by a restart`, larb0 `stat=1 greq=1` after 10 ms | dark |
| rollback to 6500 (8.3's recovery) | yes | larb0 idle | lit |

On the dark boot the SMI common clock gate was closed before Linux touched anything (before Lima's probe at
0.39 s), and LK's display request was stuck in larb0. The DSI register reads lagging one register from the first
modeset, which three DSI power cycles did not clear, are the consequence. This is the case found in
[POWER-FINAL-40](Y2-PLATFORM-V1-POWER-FINAL-40.md): a watchdog restart keeps `MMSYS_CG_CON0`, so a gate Linux closed
at shutdown (DRM disables the display and drops its SMI references) reaches LK, and LK's first fetch through
larb0 never completes. [42](Y2-PLATFORM-V1-POWER-FINAL-42.md) reopened the gates as the last step of a restart;
[43](Y2-PLATFORM-V1-POWER-FINAL-43.md) removed that because its own test restarts were dark for a second reason
(Lima closing the gate during the next boot), which 43's hold fixes. Since then nothing covered the first case.

| Part | Change (Linux `7ed664b`) |
| --- | --- |
| Restart handler | Back: priority 255, after every driver has shut down and before the watchdog handler (128); writes `MMSYS_CG_CLR0` = 3 (SMI common, larb0) when the DIS domain is powered. A powered-down domain comes back with its gates open anyway. |
| Hold | Unchanged (43): mm-clocks holds SMI common/larb0 from probe until the display mutex owns them. |
| Evidence | When the gate is found closed at probe, the boot logs `SMI common gated before Linux touched it: CG_CON0=… larb0 stat=… greq=…`. |
| Test | `tests.test_display_domain`: the handler writes only `MMSYS_CG_CLR0` = 3 and only with DIS powered; registered after the hold. |

**Not established:** why only some restarts leave the gate closed; the handler reopens it on every restart either way.

**The first OTA to 8.4:** the restart *into* the recovery is still done by the running 8.2/8.3 kernel, so the
install screen can stay dark once more; the restart *out of* the recovery into 8.4 is done by 8.4's recovery
kernel, which has the handler.

## Known gaps

- Nothing of 8.4 has run on a player yet. The check after the owner's OTA: `holding SMI common/larb0` on the 8.4
  boot and no `SMI gates … left closed`; later restarts the same.
- Unchanged: the sleep drain with the panel powered down is not measured yet (#42); the GC9503V panel path awaits a
  GC9503V owner (#45). One host-suite run failed a timing-sensitive Bluetooth test under load
  (`test_reconnect_signal_cache_intents_and_bounded_recovery`); it passed alone three times and in the full rerun.
