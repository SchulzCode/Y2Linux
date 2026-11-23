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
| 1 | Current meter per state | WAITING: no meter; each phase states what a meter would confirm |
| 2 | Energy accounting (time per state × estimated current, lease holders) | see below |
| 3 | Sleep backoff: 30 min → 1 h → 2 h → 4 h untouched; 30 min when battery not Normal / ≤ 20 %; wake ends after the safety sample (≤ 2 s) | DONE in source (`e72afe65`), device check owed |
| 4 | Batch deferred work into shared windows | see below |
| 5 | Battery Saver | see below |
| 6a | Timer slack for background services | see below |
| 6b | Freezer | needs `CONFIG_CGROUPS`/`CONFIG_CGROUP_FREEZER`; SIGSTOP fallback considered |
| 7 | Bluetooth BTIF/APDMA clock gating (stock STP PSM) | high risk; behind a switch |
| 8 | Smaller cleanups (DAC power-down after a stream, decoder block size, native EQ) | see below |

Each phase: `GATE OK` before commit, a test build on the owner's player before anything is published.

## Device checks owed (all phases)

- 3: one long unplugged sleep: wakes at 30/30/30/30/60/60/120/120/240 min (`suspend-last.json`, power journal
  `suspend_maintenance_wake`), `spm` state `maintenance_seconds`, low-battery shutdown still in time (short test
  override or a run from a low charge).
