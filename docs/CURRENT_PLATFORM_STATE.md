# Current Y2Linux platform state

Updated 2026-10-03 from the sealed Baseline02 receipts and retained UART01
hardware evidence. This synchronization changes documentation only; it does
not build, flash, push or run a new physical qualification.

## Current candidate and installed evidence

| Field | Sealed Baseline02 |
| --- | --- |
| Build | `Y2LINUX-BASELINE-02` |
| Candidate | `out/y2linux-baseline-02-candidate/` |
| Kernel | `6.18.0-y2linux-baseline-02` |
| Rootfs / release | `2025.02.18-platform-v1.21` / `1.0.0-baseline-candidate.2` |
| Compiled Linux | `8584ccd85f052f351fe51650b2348c83ca894ed4` |
| Compiled Reborn | `b71b468860233faa0a42b8448ec5777fa952b8e3` |
| ABI / feature contract / layout / data | 1 / 2 / 1 / 1 |
| Software / package | PASS |
| New-image hardware / automatic cold boot | PHYSICAL_NOT_RUN |

The latest hardware receipt is **UART01**, kernel
`6.18.0-y2linux-cpu-c3-uart-01`, Linux `e9e8d63f9c94232c2b6627881e0967583e202dac`,
Reborn `b71b468860233faa0a42b8448ec5777fa952b8e3`, root
`2025.02.18-platform-v1.20`, boot
`fd955840-35d9-47db-83e0-ff47d6bb2d2b`, taint 0.
C1, C2 and C3 are physically working on that image. Baseline02 retains that
kernel architecture; its new image and automatic cold-boot activation remain
**PHYSICAL_NOT_RUN** until owner installation and the guarded checks.

See the [sealed receipt](validation/Y2-BASELINE-02.md),
[exact candidate and fallback identities](knowledge/candidate-index.md) and
[authoritative hardware report](validation/Y2-CPU-C3-UART-PHYSICAL.md).
Compiled source identity comes from the manifest; a later documentation HEAD
or published branch tip does not identify the installed kernel/rootfs.

## CPU hardware results carried forward

| Behavior | Exact UART01 bounded result |
| --- | --- |
| C1 WFI | All four CPUs enabled; entry/residency advance; rejected 0; 400 bounded wakes |
| C2 SLIDLE | +6,971 entries; 50.654676 s / 60.054881 s = 84.3473%; clock restore failures 0 |
| C3 DORMANT | 21 guarded reset-and-return trials, then +3,624 entries; 39.204662 s / 60.088344 s = 65.2450% |
| Final C3 accounting | 3,647 entries = resumes = successes = UART ACKs; timer/context/CIRQ/clock restore failures 0 |
| Core parking | Natural CPU0-only topology; owned mask 0xe; both secondary power status copies 0 |
| Hotplug | 18 supported transitions; park 3→2→1, restore 1→2→3 |
| Timers | GPT6, GPT4 sole broadcast, 13 MHz CNTFRQ, PPI29 on all cores, highres and NO_HZ pass |
| DVFS | 598 / 747.5 / 1040 / 1196 / 1300 MHz guarded readback before and after C3 passes |
| Storage | eMMC and inserted SD: eight fsynced checksum rounds each after C2 and after C3; ext4 errors 0 |
| Wake and I/O | Workload/screen demand restore, real FFmpeg/ALSA playback, bounded USB transfer and independent Wi-Fi observer pass |

UART1's retained clock is handled by the exact MT6582 UART sleep request/ACK
contract and Linux UART ownership. It is no longer an unresolved C3 blocker.
No forced UART peripheral gate or weakened admission guard was used.

## Baseline02 boot policy

Baseline02 enables qualified **CPU0 C3 automatically after boot** through
`S05y2-cpu-idle`. The kernel initially registers C3 disabled with budget 0.
The once-per-boot service checks identity, taint and static SPM/CIRQ/timer/context
foundations, sets budget -1, enables CPU0 state2 and reads both controls back.
Failures roll back; CPU1–3 C3 remains disabled. Dynamic topology, frequency,
screen/workload/radio/USB, clock/domain, future-deadline, UART ACK and restore
fault guards remain intact. The service does not force parking or gate UART1.

`y2-platform cpu-idle-policy stop` disables C3 for the current boot and prevents
another start from overriding that choice. Recovery boot options
`y2.deep_idle=off` and `y2.cpu_safe=1` retain the safe fallback. Runtime C3 is
separate from full system suspend.

## Software validation and release limits

Fresh kernel/config/DT/module/ABI, Buildroot ARM and Reborn ARM checks pass.
The integrated suite has **391 cases: 386 passed and 5 dependency skips**;
27 native filesystem/GIO and 3 ALSA checks plus installed ARM checks cover the
missing minimal-environment dependencies. All **125 focused source tests** and
**238 Reborn tests**, formatting/lint, Cortex-A7 QEMU, installed ARM applications,
ELF/dependencies and preserving-package checks pass. These are retained build
receipts, not tests rerun by this documentation update.

Full system suspend with same-boot Power/RTC wake remains open. This pass does
not qualify additional CIRQ edge injection, long endurance, electrical battery
savings, native 24-bit/high-rate wired output, optional Bluetooth peer/codec
behavior, USB host/VBUS, a calibrated battery gauge or public binary distribution.
All five guarded active OPPs remain available; a C3 entry cap of 747.5 MHz does
not lower the active CPU ceiling.

Source/license collection passed with recorded collector limitations. The
package remains owner-local; successful collection is not redistribution
permission or a byte-identical rebuild demonstration. The
[community feature audit](release/Y2-COMMUNITY-BETA-FEATURE-AUDIT.md) retains
233 features and the separate unresolved release gates.

## Next owner boundary

Verify the sealed package and use `MT6582_preserve_data_scatter.txt` in
Download Only mode, selecting **BOOTIMG and ANDROID/Y2ROOT only**. Preserve
USRDATA/Y2DATA and every protected partition. After owner installation, the
packaged SSH harness verifies exact identity and real cold-boot activation,
quarantines C3 for foundation tests, runs one guarded C3 trial plus 20 cycles,
and checks normal runtime, wake and I/O regressions. No installation is inferred
from the user's earlier confirmation for UART01.


## Historical snapshots

The following text retains earlier dated decisions and results. Its “current”,
“next” and candidate labels belong to those sessions; the Baseline02 summary
above takes precedence. Historical failures and seal-time NOT_RUN receipts
remain evidence for their exact images.

---

# Current Y2Linux platform state

## Baseline02 sealed precedence — 2026-10-03

New latest-source preserving candidate is sealed at `out/y2linux-baseline-02-candidate/`:
Linux`8584ccd85f052f351fe51650b2348c83ca894ed4`, Reborn`b71b468860233faa0a42b8448ec5777fa952b8e3`,
kernel6.18.0-y2linux-baseline-02/rootv1.21. Fresh build,391 production cases,
source/native/Reborn/ARM/QEMU/ELF/config/DT/ABI/package checks pass. Owner-selected
automatic qualified CPU0 C3 boot policy is included; all existing guards remain.
Only BOOTIMG/Y2ROOT; preserve Y2DATA and protected partitions. New-image hardware/
cold-boot qualification NOT_RUN. Installed UART01 still proves C1/C2/C3 working,
3647 real C3 returns, same boot/taint0 and zero restore failures. Earlier summaries
below are historical. See [sealed baseline record](validation/Y2-BASELINE-02.md).
No flash or push; owner installation is the next boundary.

## Baseline02 preparation precedence — 2026-10-03

Installed UART01 is physically qualified for C1/C2/C3 on Linuxe9e8d63/Rebornb71b4688,
with3647 real C3 returns and clean restores, same boot/taint0. Earlier summaries
below are historical. Owner requests a latest integrated Baseline02 preserving
candidate and selects automatic qualified C3 boot policy. Source tests/live ARM
control transaction pass; fresh build/seal and new-image physical checks are
separate boundaries. See [Baseline02](validation/Y2-BASELINE-02.md) and
[UART01 hardware](validation/Y2-CPU-C3-UART-PHYSICAL.md).

## Current audit precedence — 2026-10-02

The [community beta master audit](release/Y2-COMMUNITY-BETA-FEATURE-AUDIT.md) now reconciles source, builds and hardware across both repositories; the [RC1 plan](release/Y2-COMMUNITY-BETA-PLAN.md) defines the shortest proposed beta path. At its17:49:54 CEST cutoff: Linux `dcbd7d1` candidate4 source/build activity, Reborn `b92d312`; latest sealed/reported installed candidate3, with narrow SD characterization in a concurrently edited draft; latest broad physical report remains Fix02. The exact baseline and conflicting older “unflashed” text are documented there. Earlier summaries below are historical and must not override that baseline. No hardware acceptance is created by this documentation update.

Updated 2026-09-29 from the **CPU Final Fix02 physical qualification**. This
documentation update made no source repair, build or flash.

**Working development baseline; FAIL for normal CPU-platform acceptance.**
The awake CPU platform is substantially qualified: timers, QoS, MMC runtime
gating, automatic parking and 1196/1300-MHz voltage DVFS pass. Remaining gates
are full-suspend resume completion, the SLIDLE bus-DCM predicate and loaded
USB upload. The [Fix02 physical report](validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md)
is authoritative for actual behavior, ahead of software receipts.

**Storage ceiling candidate 2, 2026-10-01 (the candidate to flash):**
`out/y2linux-storage-ceiling-02-candidate/`, Linux `4db6d6de`, kernel
`6.18.0-y2linux-storage-ceiling-02`, BOOTIMG `5c17f744…`, Y2ROOT `dc611011…`.
Candidate 1 booted: eMMC ran HS200 cleanly (~133 MB/s read), but no SD card
initialised because the PMIC write gate refused the SD rail writes.
Candidate 2 admits exactly those fields. SD at the ceiling is not yet
observed on hardware.

**Storage ceiling candidate 1, 2026-10-01 (superseded):** `out/y2linux-storage-ceiling-candidate/` is the Product
UI v2 candidate plus the storage hardware ceiling pass. eMMC goes from 50 MHz
HS52 to **HS200** (8-bit, 200 MHz, fixed 1.8 V VIO18 IO, CMD21 tuning). SD goes
from 50 MHz SD HS at 3.3 V to **UHS-I up to SDR104** (4-bit, 200 MHz, MT6323
VMC 3.3→1.8 V switch, CMD19 tuning). Both have automatic fallback ladders and
readback verification. Linux `540ae029`, Reborn `b92d312c`, kernel
`6.18.0-y2linux-storage-ceiling`, root `2025.02.18-platform-v1.11`, BOOTIMG
`4332d4f2…`, Y2ROOT `72eab34a…`. Software-validated; **not flashed or
physically qualified; no mode above 50 MHz has run on a Y2 yet**. See the
[storage ceiling record](validation/Y2-STORAGE-CEILING.md).

**Product candidate, 2026-10-01 (superseded by the storage ceiling candidate):**
`out/y2linux-reborn-product-ui-v2-candidate/` is the Fix03 platform (same
kernel) plus Reborn Product UI v2, a Reborn-mark splash and a platform
backlight-off step before power-down. Linux `5cfe04cb`, Reborn `bd8436dd`,
root `2025.02.18-platform-v1.10`, BOOTIMG `8a974cb5…`, Y2ROOT `a5c2914e…`.
Software-validated; **not flashed or physically qualified**. See the
[Product UI v2 receipt](validation/Y2-REBORN-PRODUCT-UI-V2.md).

**Software candidate, 2026-10-01:** one CPU Final Fix03 preserving candidate
(Linux `76a5d41d`, Reborn `77cf83e0`, BOOTIMG `c55b2640…`, Y2ROOT `04cafd4b…`)
is built and software-validated, but **not flashed or physically qualified**.
It reconciles the SLIDLE bus-DCM baseline and makes the USB storm guard
progress-aware. It fixes wake escalation, SD identity and the DVFS label, and
instruments the resume window so the next RTC attempt names the stalled call.
See the [Fix03 receipt](validation/Y2-CPU-FINAL-FIX03.md). The Fix02 result
below remains the latest observed hardware state.

## Latest physically observed identity

| Field | Exact identity, observed 2026-09-29 |
| --- | --- |
| Linux compiled source | `1a1a6693dcd82f62daecd4c1491ef512823a86c5` |
| Reborn compiled source | `77cf83e09a18f82a867040e72d35b6e70b26fa85` |
| Kernel | `6.18.0-y2linux-cpu-final-fix02` |
| Rootfs | `2025.02.18-platform-v1.8` |
| Release / build | `1.0.0-cpu-final-fix02-candidate.1` / `Y2LINUX-CPU-FINAL-FIX02` |
| Final boot | `6bb70fad-29a7-4a8e-a1a1-b69884434127` |

Later documentation HEADs are not compiled source identities. The report retains
all four boot IDs and the retained-journal capture. No fresh device observation
is implied by the date of a documentation commit.

## Physical coverage

| Area | Status | Observed scope |
| --- | --- | --- |
| GPT6/GPT4/PPI29, highres/NO_HZ | PASS | 13-MHz CNTFRQ all CPUs, sole GPT4 broadcast, PPI29 on CPU0–3, ~1.09-ms 1-ms sleeps |
| Hotplug, schedutil, workload QoS, thermal override | PASS | eight classes, real input/playback/scan/transfer/maintenance producers, screen-off interaction release |
| C1 WFI | PASS | entries/residency advance, rejected 0 |
| MMC runtime clock gating | PASS | eMMC/SD gated at rest, CCF counts 0, 16/16 hash rounds, no errors or mismatches |
| C2 SLIDLE | FAIL | clock mask 0 with radios off, but TOPCKGEN+4 reads `0x0`, not the required `0x0f` |
| Automatic parking / restore | PASS | 3→2→1 at 5-s pacing, display restore 0.63 s; wake counted as pressure restore |
| High OPP / voltage DVFS | PASS | PWRAP `0x7f` ready; 1196 @ 1.20 V, 1300 @ 1.25 V readback; sampled ordering; integrity 0 errors |
| C3 / CIRQ deadline handoff | NOT_TESTED | preflight unmet: topology, frequency, domains, clocks, bus |
| SRAM journal | PASS | awake self-test; warm-reboot retention; valid records after backstop reset |
| Charger refusal / staged PM | PASS | active-charging `-EBUSY` clean unwind; freezer/devices/platform/processors/core same boot |
| Full RTC suspend | FAIL | SPM return and resume vector proven; reset by 30-s backstop after device resume, before PM exit |
| Power wake / RTC cycles / restoration | NOT_TESTED | no same-boot full resume |
| USB loaded transfer | FAIL | 0/10; EP1 RX DMA channel 5 IRQ storm, twice, UI alive, Wi-Fi-observed |
| Wi-Fi / CONSYS | PASS, bounded | 4 × 1-MiB SHA roundtrips; radio restore after quiesce |
| SD media lifecycle | FAIL (non-CPU) | kernfs inode/ctime inventory remounts `/media/sd` after reclaim |

Screen-off idle, parked: 2.71 % of four cores (Fix01 9.74 %), 171 IRQ/s (391),
355 context switches/s (504). This is a CPU idle improvement, not a measured
battery improvement. Owner actions: USB unplug and reconnect only.

## Enabled software and unfinished qualification

- S16 stereo 44.1/48 output is enabled. Native S24/S32/preserved 24-bit and
  88.2/96-kHz output remain gated; decoding high-rate sources is separate.
- Optional AAC/aptX/aptX-HD/LDAC encoders are included in the private experiment
  build. Normal runtime disables their endpoints; SBC is baseline and Auto
  retains qualification/distribution gates. Fix01 peer audio is NOT_TESTED.
- Schema-2 low-voltage policy is enabled and provisional: critical/warning/
  recovery 3.4/3.5/3.6 V. Estimated SOC is not measured current, coulomb count
  or calibrated capacity. Pack thermal and discharge/reserve/full-cycle charging
  acceptance remain open. See the [power contract](architecture/platform-power-v1.md).
- Automatic deep sleep, C3 promotion, AP watchdog recovery, USB host/VBUS/UAC
  and NCM activation remain unqualified/gated. Screen-off is not system sleep.
- Root-only OTA/recovery/maintenance software exists; production trust,
  distribution/provisioning, recovery/endurance and byte-identical image
  reproducibility remain separate gates. Automatic BOOTIMG OTA is excluded.

The [capability ledger](validation/Y2-HARDWARE-FINAL-CAPABILITIES.md) separates
latest observations from historical build flags. Runtime status determines
actual admission/faults. [Hardware gates](knowledge/platform-v1-hardware-gates.md)
and [Reborn state](../../Y2Reborn/docs/CURRENT_REBORN_STATE.md) retain other limits.

## Next smallest correction boundary

**CPU Final Fix03 is an implemented, unflashed candidate.** The owner's single
Fix03 run (prepared in [Fix03 physical qualification](validation/Y2-CPU-FINAL-FIX03-PHYSICAL-QUALIFICATION.md))
checks the regression gate, SLIDLE with radios off, USB 10 × 1 MiB, then
RTC → Power → 5 cycles. If resume still stalls, the timed ring names the one
call to correct next. The Fix02 EP1 "IRQ storm" was legitimate throughput
tripping the guard, not a re-arm defect. Preserve all Fix02 physical passes. Do not
bypass guards or restart CPU architecture. The
[roadmap](planning/platform-v1-roadmap.md) and
[standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule)
govern implementation and acceptance; this page does not authorize new scope.

## Candidate, fallback and evidence

Latest candidate: `out/y2linux-cpu-final-fix02-candidate/`, already owner-flashed
before qualification. Preserve-data payload is BOOTIMG/Y2ROOT only. Exact
known-good fallback is **Hardware02**, kernel `6.18.0-y2linux-hardware-02` /
root `2025.02.18-platform-v1.4`. The [candidate index](knowledge/candidate-index.md)
holds full hashes and identity; [reconstruction](build/platform-v1-reconstruction.md)
uses the built pair rather than later docs HEADs.

Private evidence: `out/cpu-final-fix02-physical-qualification/20260929T152328Z/`,
460 sealed files (Fix01: `out/cpu-final-fix01-physical-qualification/20260927T205514Z/`). Raw secrets, firmware and calibration
are not published. [Documentation index](README.md), [catalog](DOCUMENTATION_CATALOG.md)
and [knowledge rules](KNOWLEDGE_BASE.md) identify current contracts versus history.
Older candidate/evidence reports preserve their original scope and failures.
