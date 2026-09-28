# Y2 CPU Final Fix02 physical qualification — prepared, not run

**Status: NOT_RUN.** This document prepares the single owner-controlled
acceptance run of the [Fix02 candidate](Y2-CPU-FINAL-FIX02.md). Nothing below
is a hardware result. Every row stays NOT_TESTED until a receipt from the
installed Fix02 identity replaces it. The [Fix01 report](Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md)
remains the authoritative physical baseline until then.

## Owner action

1. Verify `SHA256SUMS` in `out/y2linux-cpu-final-fix02-candidate/`.
2. Install only BOOTIMG and ANDROID/Y2ROOT with
   `MT6582_preserve_data_scatter.txt` through the existing preserving workflow.
3. Confirm the existing USB and Wi-Fi SSH aliases reach the Y2, then run:

```sh
python3 tools/development/qualify-cpu-fix02.py --run --host y2 --wifi-host OWNER_WIFI_ALIAS
```

Add `--allow-warm-reboot` to let it issue one ordinary `reboot` that proves
SRAM retention before any suspend request. Without it, retention stays
unproven and staged tests rely on the awake self-test and the watchdog backstop.

## What the harness does

Awake checks first, each with its own receipt, no owner input:

| Step | Pass criterion |
| --- | --- |
| Identity | installed build/Reborn/kernel/rootfs equal the package manifest |
| Timers | events ready, highres, NO_HZ, `arch_sys_counter`, 1-ms sleep median < 2 ms on every CPU |
| QoS | real wheel input yields Interactive; scan lease; screen-off releases Interactive |
| MMC runtime gating | both hosts `gated=1` after 8 s idle, suspends increase, 1 MiB fsync write/read-back SHA match on `/data` (and SD if mounted) resumes the host, no resume error, ext4 error counters unchanged, CCF summary retained |
| SLIDLE/coordinator | silent 240-s screen-off window: `parked_mask=0xe`, online `0`, SLIDLE entries > 0; display wake restores `0-3`; per-process CPU attribution for the window |
| PWRAP/DVFS | `pwrap_readiness` operands recorded; every admitted OPP reaches its frequency with 1.15/1.15/1.15/1.20/1.25 V readback; conservative-only admission is recorded as such |
| USB loaded stress | ten 1-MiB upload+download SHA-verified rounds over USB; independent Wi-Fi capture before and after (status, fault snapshot, carrier/UDC/runtime PM, IRQs, dmesg, callback ring) |
| SRAM journal | awake self-test `result=pass`; optional warm reboot shows `previous_stage=SELFTEST_B selftest_scratch=retained` |

Only if every awake check passes: charger state is classified. Refusal is
tested only when the charger engine is active (then the owner is asked to
unplug USB once). Staged `freezer`, `devices`, `platform`, `processors`, `core`
each arm the 30-s RGU backstop; a new boot is never counted as resume and makes
the harness collect the previous boot's journal, callback ring and open
callback. Then a full RTC suspend (+30 s alarm) and a Power wake (one
deliberate press, 120-s RTC backstop), each with same boot ID, unchanged taint
and the serviced wake IRQ, followed by restoration captures and three more USB
rounds.

## Result table (to be filled from receipts)

| Area | Status |
| --- | --- |
| Timer/QoS/playback regressions | NOT_TESTED |
| MSDC runtime clocks gate safely | NOT_TESTED |
| SLIDLE entries/residency > 0 | NOT_TESTED |
| Coordinator parks 3→2→1, restores on demand, no oscillation | NOT_TESTED |
| PWRAP readiness understood on device | NOT_TESTED |
| 1196/1300 MHz admitted with voltage readback | NOT_TESTED |
| Voltage fault fallback | NOT_TESTED physically (host fault injection only) |
| Repeated loaded USB transfers / attributed fault | NOT_TESTED |
| SRAM journal awake self-test / warm retention | NOT_TESTED |
| Charger refusal only when charging active | NOT_TESTED |
| Staged freezer/devices/platform/processors/core | NOT_TESTED |
| Full RTC suspend same boot | NOT_TESTED |
| Power wake same boot | NOT_TESTED |
| Post-resume CPU/timer/storage/USB/radio/display/audio/Reborn | NOT_TESTED |
| DORMANT prerequisites (`dormant_preflight`) | NOT_TESTED; C3 stays default off |

## Stop rules

The harness stops before any suspend if an awake check fails, and stops the
suspend sequence at the first failure. It never flashes, never overrides a
blocker, thermal limit, OPP guard or charger refusal, and never treats a new
boot as resume. Owner input is limited to: USB unplug only if charging is
active, one Power press for the Power-wake test, and one Power press only if an
RTC wake does not return automatically.
