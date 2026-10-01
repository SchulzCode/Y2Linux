# Current Y2Linux platform state

Updated 2026-09-29 from the **CPU Final Fix02 physical qualification**. This
documentation update made no source repair, build or flash.

**Working development baseline; FAIL for normal CPU-platform acceptance.**
The awake CPU platform is substantially qualified: timers, QoS, MMC runtime
gating, automatic parking and 1196/1300-MHz voltage DVFS pass. Remaining gates
are full-suspend resume completion, the SLIDLE bus-DCM predicate and loaded
USB upload. The [Fix02 physical report](validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md)
is authoritative for actual behavior, ahead of software receipts.

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
