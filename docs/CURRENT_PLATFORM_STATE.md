# Current Y2Linux platform state

Updated 2026-09-28 from the 2026-09-27 **CPU Final Fix01 physical qualification**.
Source and documentation are published on `main` in both repositories. Publication
does not qualify hardware or promote a release. This documentation update made
no device connection, source repair, build or flash.

**Working development baseline; FAIL for normal CPU-platform acceptance.**
Timers and real workload hints now work. Remaining gates concern MMC runtime
clock ownership/idle, PWRAP readiness, early PM diagnostics/recovery and loaded
USB recovery. The [physical report](validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md)
is authoritative for actual behavior, ahead of software receipts.

**Software candidate, 2026-09-28:** one CPU Final Fix02 preserving candidate
(Linux `1a1a6693`, Reborn `77cf83e0`, BOOTIMG `a621e270…`, Y2ROOT `5010d839…`)
is built and software-validated but **not flashed or physically qualified**;
see the [Fix02 receipt](validation/Y2-CPU-FINAL-FIX02.md). The Fix01 physical
result below remains the latest observed hardware state.

## Latest physically observed identity

| Field | Exact identity, observed 2026-09-27 |
| --- | --- |
| Linux compiled source | `0de6e951b438bf0f2d701e23e476d2b2405ba3c6` |
| Reborn compiled source | `36db1869c6bab1ad2d00b7d8e7807ea6ef5b3803` |
| Kernel | `6.18.0-y2linux-cpu-final-fix01` |
| Rootfs | `2025.02.18-platform-v1.7` |
| Release / build | `1.0.0-cpu-final-fix01-candidate.1` / `Y2LINUX-CPU-FINAL-FIX01` |
| Last recovery boot | `81e422f3-43cd-4ad6-877f-692eb1191b36` |

Later documentation HEADs are not compiled source identities. The report retains
all three boot IDs and immediate recovery captures. No fresh device observation
is implied by the date of a documentation commit.

## Physical coverage

| Area | Status | Observed scope |
| --- | --- | --- |
| GPT6/GPT4/PPI29 | PASS | advancing 13-MHz reference, sole GPT4 broadcast, local architectural events on CPU0–3 |
| Highres/NO_HZ | PASS | actual timer_list state and stopped ticks; requested 1-ms sleeps about 1.09 ms, formerly about 10 ms |
| Conservative CPU policy | PASS | 598/747.5/1040 MHz, schedutil, repeated hotplug and bounded hash-integrity load |
| Workload QoS | PASS | all eight real app/worker classes, renew/expiry, screen-off interaction release and thermal priority |
| C1 WFI | PASS | entries/residency advance |
| C2 SLIDLE | FAIL | zero entries; radios-off `0x3000` = MSDC0/eMMC + MSDC1/SD |
| Coordinator | PARTIAL | no parking during 120/160-s periods; sustained eligibility unproven; owned automatic restore NOT_TESTED |
| High OPP / voltage DVFS | PARTIAL | bin0/software VOSEL/1.15-V feedback recognized; 1196/1300 admission FAIL at pwrap_readiness; voltage changes NOT_TESTED |
| C3 / CIRQ deadline handoff | NOT_TESTED | disabled, clock/domain and recovery prerequisites unmet |
| Suspend | FAIL staged attempt | devices request lost recovery; remaining stages and full RTC/Power/same-boot restoration NOT_TESTED |
| Persistent PM diagnostics | PARTIAL | durable kernel_suspend survives; invalid SRAM does not identify exact kernel boundary |
| Awake RTC | PASS | alarm and PMIC IRQ while awake; not deep wake |
| Wired playback | PASS, bounded | S16 44.1 and 24/96 -> 48 output; zero XRUN/decode/filter errors, no analog/endurance claim |
| Wi-Fi / CONSYS | PASS, bounded | four small Wi-Fi hash roundtrips; one isolated CONSYS retry restores firmware/calibration/WLAN/HCI |
| USB | PARTIAL | first hash roundtrip passes; next transfer loses connection with UI alive; owner restart |
| Recovered filesystems/runtime | PASS, bounded | rw root/data/SD, ext4 errors 0, taint 0, CPU/UI/audio/controllers respond; new boot is not resume |

Owner Power once and two owner restarts are recorded; no spontaneous reboot is
established. Clean charging-refusal unwind is NOT_TESTED: USB-online was paired
with BAT0 Not charging/hold. Final health retains a supplicant_unavailable
discrepancy despite association, route and verified Wi-Fi TCP traffic.
There is no demonstrated overall idle-energy improvement.

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

**CPU Final Fix02 is a plan, not an implemented candidate.** Prove early PM
journal safety/readback/retention; expose and reconcile genuine PWRAP readiness;
separate MMC system-sleep retention from runtime gating and reassess sustained-idle
coordination; diagnose/correct loaded USB recovery with independent evidence.
Preserve working timers/QoS/conservative CPU/audio and the single CONSYS retry.
Do not bypass guards or restart CPU architecture. The
[roadmap](planning/platform-v1-roadmap.md) and
[standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule)
govern implementation and acceptance; this page does not authorize new scope.

## Candidate, fallback and evidence

Latest candidate: `out/y2linux-cpu-final-fix01-candidate/`, already owner-flashed
before qualification. Preserve-data payload is BOOTIMG/Y2ROOT only. Exact
known-good fallback is **Hardware02**, kernel `6.18.0-y2linux-hardware-02` /
root `2025.02.18-platform-v1.4`. The [candidate index](knowledge/candidate-index.md)
holds full hashes and identity; [reconstruction](build/platform-v1-reconstruction.md)
uses the built pair rather than later docs HEADs.

Private evidence: `out/cpu-final-fix01-physical-qualification/20260927T205514Z/`,
364 sealed files / 55 command receipts. Raw secrets, firmware and calibration
are not published. [Documentation index](README.md), [catalog](DOCUMENTATION_CATALOG.md)
and [knowledge rules](KNOWLEDGE_BASE.md) identify current contracts versus history.
Older candidate/evidence reports preserve their original scope and failures.
