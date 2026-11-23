# Power plan 2026-11: measure, back off, batch, save

Owner request 2026-11-24 after the 9.3.39 drain night ("can you just do all the steps in total?").
No current meter is available yet; everything that needs one is built so that a meter only adds numbers.
Entry audit: [roadmap-gap-audit.md](roadmap-gap-audit.md#power-plan-2026-11-entry-audit--2026-11-24).

## Evidence that started it

- Owner's player, 9.3.39, boot `70c6fea1`, unplugged 2026-11-22 23:44 → 11-23 22:49 UTC: 47/47 clean
  `SPM_CPU_SHUTDOWN_INFRA_PDN` sleeps, 47 maintenance wakes (~2.3 s awake each), 100 % → 79 %. Voltage-estimated
  ≈10 mA. **Owner: older drain numbers may be wrong and are not compared.**
- Awake (USB, dark, not playing): `slow_reject_clock` 376 089, of which BTIF + APDMA (Bluetooth enabled) 376 058;
  `dormant_entries` 5 in 25 h; `deep_audio_entries` 0. Ledger F37.
- Kernel has timer slack (`/proc/<pid>/timerslack_ns`); no cgroups (so no freezer) and no uclamp.

## Phases and state

| # | Phase | State |
| --- | --- | --- |
| 1 | Current meter per state | WAITING: no meter. Phase 2 reads a `power-profile.json` the day one exists |
| 2 | Energy accounting | DONE in source (`998f319c`, summary line `0364b2a8`): `energy.json` seconds per state (asleep, charger, screen_on, dark_playing, dark_idle), radio and lease seconds, gauge mAh per state, profile mAh when a profile exists; in error reports |
| 3 | Sleep backoff | DONE in source (`e72afe65`): 30 min ×4, 1 h ×2, 2 h ×2, then 4 h; 30 min when the battery is not Normal, ≤ 20 % or unknown; a wake ends after the safety sample (≤ 2 s) |
| 4 | Batch deferred work | AUDITED, no change: the service daemon already runs every periodic job (space policy, time, update check, error reports, health) in one maintenance pass (30 s lit, 300 s dark), none wakes the player from sleep; Reborn's only short poll is the Last.fm sign-in |
| 5 | Battery Saver | DONE in source (Reborn `1c34592`): Display › Battery Saver, Off / At 15 % (default) / On: ≤ 30 % brightness, ≤ 15 s screen timeout, no animation |
| 6a | Timer slack | DONE in source (`5e67db62`): 50 ms for the power, service, connectivity and SSH daemons and their children |
| 6b | Freezer | DROPPED on evidence: the candidates (umtprd, dropbear, wpa_supplicant, wpa_cli, crond, syslogd) wake < 2 times/s together while idle (umtprd and the SSH listener 0/s); not worth `CONFIG_CGROUPS` against the kernel Image cap |
| 7 | Bluetooth BTIF/APDMA clock gating (stock STP PSM) | IN PROGRESS: donor `psm_core.c` / BTIF HAL analysis |
| 8 | Cleanups | Decoder blocks: already 4096 frames since 9.3.37. DAC: powers down 120 s after a stream as designed (the long active time seen was the owner's wake). Reborn dark idle wakes ≈10/s over seven threads (1–2/s each): left, watchdog heartbeats. Native EQ chain instead of FFmpeg filters: NOT done, it would change the sound of every EQ/AutoEq profile and needs listening tests |
Each phase: `GATE OK` before commit, a test build on the owner's player before anything is published.

## Device checks owed (all phases)

- 3: one long unplugged sleep: wakes at 30/30/30/30/60/60/120/120/240 min (`suspend-last.json`, power journal
  `suspend_maintenance_wake`), `spm` state `maintenance_seconds`, low-battery shutdown still in time (short test
  override or a run from a low charge).
