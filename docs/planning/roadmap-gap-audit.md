# Y2Linux roadmap and gap audit

Original audit date: **2026-09-09**. Documentation/planning only. No implementation, build,
BOOTIMG creation, device access or flash. Repository baseline `00b4afa`; GitHub
issues #1–#27 and M0/M1 inspected before adding five deferred epics #28–#32.
Y2PlayerNative has no open application issues and its checked-in content remains
blueprint/planning material; no native platform readiness is inferred from that.

## M2 activation audit — 2026-09-09

Baseline `ebeed72`, clean `main`; current GitHub #1–#32, open M0 and closed M1
reviewed against the local blueprint and retained hardware result. The owner now
explicitly authorizes autonomous M2 research/implementation/builds and normal
Git/GitHub operations, with a stop at each hardware-testable BOOTIMG and before
any physical-device modification. This supersedes the earlier planning-session
restrictions and queued research order; it does not establish any hardware fact.

**M2 is ACTIVE, entry PARTIAL; exit BLOCKED on unimplemented/unverified core
interfaces.** M1 stays complete. All coverage rows below were reconsidered:
their capability classifications and evidence limits remain unchanged because
no newer physical result exists. M2's phase status changes from PLANNED to ACTIVE;
#22–#27 remain open, and #27's controller gate is still BLOCKED. The working
owner-reported version is 6.18.0-y2-m1; exact flashed SHA-256, MemTotal, repetition
and timing are not supplied. The retained time32 candidate identity is recorded
in [the M1 result](../knowledge/m1-runtime-hardware-result.md), not promoted to
an identified hardware trial.

Execution priority: (1) #27 USB contract and only #23's needed VUSB/PWRAP/clock
prerequisite; (2) one logging implementation/trial if that contract permits it;
(3) RAM adequacy and #22's bounded expansion decision; (4) #24 input; (5) #25
standard display handoff; (6) #26 read-only removable media and recoverable
stage-appropriate rootfs; (7) identified repeatability/stability and M2 exit audit.
RAM expansion, UART wiring, full SMP and full power-framework implementation are
not prerequisites for USB research. Reorder subsequent work only on evidence.
The smallest current blocker is the FM-normal-handoff USB PHY/clock/VUSB/DMA
contract; inspect those exact functions before selecting a runtime change.

D08, CPU0, stopped AP watchdog and guarded framebuffer remain the default.
Stock preloader/LK, partition tables and calibration remain protected. Existing
SPFT/FM recovery evidence is retained without another provenance/recovery audit;
M0 backup/installed-lineage and inherited-DMA uncertainties remain residual risks,
not production waivers. New subsystem ownership must be justified separately.
No M3 audio or Y2PlayerNative implementation starts before real M2 exit evidence.
No optional feature has been waived merely to reach today's target.

### Current execution checkpoint

[M2-PWRAP-01](../build/m2-pwrap-01-result.md) is offline validated, 1,093,632 bytes,
SHA-256 `c51102c861d65d39f5af2425ed73e9c1ba21d8adaae1b016f450a58630946ac0`.
The owner has now supplied [two physical photographs](../knowledge/m2-pwrap-hardware-result.md):
PWRAP RC=0/VALID=3, CID=0x2023, VUSB=0xc000, BEAT 18→50 and MemTotal 22208 kB.
The cached PWRAP/VUSB result is confirmed on this boot; exact flashed hash,
host timing and restoration remain unreported. Timer -61 is a source-identified
IRQ-name parser mismatch. #23/#27 and M2 remain open.

Scope review at `c6709cc`: the next localized candidate corrects IRQ parsing and
reads four exact USB clock handoff registers; no clock/PHY/USB controller/PMIC
write or DMA ownership change is added. [Contract](../knowledge/m2-usb-clock-probe.md).
All coverage rows were reconsidered against these photographs: boot stability,
memory and power remain PARTIAL (now with measured MemTotal and one successful
PWRAP read path); USB remains BLOCKED on its lower-level ownership contract.
Other input/display/storage/rootfs/thermal/battery gaps and later-phase gates are
unchanged. Preserve D08, CPU0, screen/watchdog and established recovery policy.

[M2-USBCLK-01](../build/m2-usbclk-01-result.md) is now offline validated and
awaits the owner test: 1,093,632 bytes, SHA-256
`602080eb199f3a4be30082ee114dfeb97ef414e10d8ad4acf5d239f2096cd6ec`.
One clean build and 19 targeted validation methods pass; no device action occurred.

### First prerequisite scope review

The focused [PWRAP/VUSB probe contract](../knowledge/m2-pwrap-probe.md) now resolves
one implementable #23 step using FM/LK and pinned upstream/vendor source. This
adds bounded AP PMIC read transactions to a candidate, with no PMIC register
writes or USB ownership. Power remains PARTIAL and USB remains BLOCKED pending
physical state evidence and the remaining PHY/clock/DMA contract. D08 and all
other coverage classifications remain unchanged. Basic battery/charger reporting
also remains an M2 exit gap from the blueprint; VUSB status cannot satisfy it.

## Blueprint assessment and authority

The [Y2Linux/Y2PlayerNative blueprint](https://github.com/SchulzCode/Y2PlayerNative/blob/main/YY2PlayerNativeBlueprint.md)
still covers the project's direction adequately: upstream 6.18, standard Linux
interfaces, native Rust/C application, native audio before central player work,
power/connectivity/product gates and rolling-wave planning. This audit reviewed
the local blueprint, the actual Y2Linux code/config/DT and retained build reports,
current issues, and the stock evidence indexed in
[evidence-index.md](../knowledge/evidence-index.md). Local blueprint snapshot SHA-256: `f9850e35ed790008cbb125001cf166848c7dd1c11c580d41299ecbe311add95e`.
No repeated provenance audit
or new driver investigation was performed. Source applicability is not hardware
success; Android behavior is a reference, not the native implementation.

Important reconciliations:

- Blueprint example task IDs are illustrative. Existing Y2E/Y2B IDs and results
  remain authoritative; do not create duplicate tasks with those sample IDs.
- The owner superseded the original role split with direct M1 implementation.
  That authorization is not a standing permission to execute a planning epic.
- The owner accepted specific first-boot risks under D11. M1 core success does
  not retroactively certify all M0 backups/lineage/DMA questions. M0 remains open;
  proven same-device SPFT recovery must not be repeatedly rediscovered.
- **M1 core is achieved**, as explicitly reported by the owner. Blueprint M1
  wording about reproducible boots/stability is broader than the retained report:
  boot identity/repetition/timing/stress evidence remains PARTIAL and is tracked
  under Y2H-300. Do not reopen the achieved core or claim release qualification.
- New recurring milestone audits and the fast-iteration validation policy below
  supplement the blueprint. They are not permission for a full build on a
  documentation-only day.

## Classification key

One primary status describes the scoped area, not every fact in it:

- **CONFIRMED:** the stated narrow result is directly evidenced; scope matters.
- **PARTIAL:** useful evidence or execution exists, but the platform capability
  or its qualification is incomplete.
- **PLANNED:** a bounded next task exists; it has not been implemented/validated.
- **UNKNOWN:** the required contract/capability is not established; an epic is a
  reminder, not enough evidence to upgrade this to a resolved plan.
- **BLOCKED:** a specific missing prerequisite prevents the stated next capability.

Hardware-fact labels in older knowledge documents (including INFERRED/DISPROVEN)
remain unchanged. An epic does not turn unknown hardware into confirmed support.

## Coverage matrix

| Area | Status | Actual evidence / remaining gap | Durable tracking |
| --- | --- | --- | --- |
| Physical Linux + initramfs + native PID1 | **CONFIRMED** | Owner reports 6.18.0-y2-m1, PID1, mounts, increasing BEAT/uptime and working sleep. [Result](../knowledge/m1-runtime-hardware-result.md). | Closed M1 / #20–21; core stays achieved. |
| Boot/kernel stability and maintenance | **PARTIAL** | Reproducible offline artifacts and a working run exist; exact flashed hash for latest report, repeated cold/warm boots, duration, timing accuracy, stress and maintained 6.18.y selection are not qualified. | Y2H-300 #28; maintenance/release policy #32. |
| RAM and reserved/DMA ownership | **PARTIAL** | D08 24.5 MiB described map is visible; photographed MemTotal 22208 kB; working-set adequacy, expanded banks, heap/high carveouts and full DMA containment remain open. [Map](../knowledge/initial-ram-map.md). | #22; #28. No automatic expansion. |
| SMP | **UNKNOWN** | CPU0 works; physical extra cores do not establish release/coherency/interrupt/PM semantics. [Audit](../knowledge/linux-6.18-support.md). | #28; optional for first wired player. |
| Clocks, resets, pinctrl/GPIO/IRQ and I2C | **BLOCKED** | Minimal fixed timer/UART clocks work sufficiently for M1. Common peripheral providers/mux/reset/rail contracts are not established; USB/storage/audio cannot simply inherit sibling compatibles. | #23 and #28, now explicit shared foundation. |
| Watchdog and controlled reset | **PARTIAL** | Reviewed early AP_RGU stop and displayed stopped state; no production driver takeover, pet/timeout strategy, deliberate reset or restart qualification. | #28 with #30. Preserve current experiment behavior. |
| On-device diagnostic channel | **CONFIRMED** | Working inherited RGB565 text and owner-observed heartbeat; exact other fields/duration are not invented. | #20–21; [result](../knowledge/m1-runtime-hardware-result.md). |
| Developer host logs / USB | **BLOCKED** | CDC ACM/PID1 relay specified, but controller/PHY/clocks/VUSB/FIFO/PIO handoff are unresolved. [Research](../knowledge/usb-logging.md). Stock ADB is not Linux USB support. | #27, #23, #28. |
| Physical UART / early crash capture | **UNKNOWN** | UART0 candidate exists; no pad/level/wire capture. Ramoops retention/reader is unproved. USB cannot log hangs before its initialization. | Existing #16; broader crash/debug policy #32. |
| Display/controller/panel | **PARTIAL** | Inherited 480×360 RGB565 works; panel candidate and LK evidence exist. No native DRM/panel initialization, standard-interface takeover or display-PM proof. | #25; #28. |
| Backlight | **PLANNED** | Existing illumination persists; independent brightness/control/limits and rail/PWM ownership unverified. Narrow ownership research is queued. | #25/#23; #28/#30. |
| Wheel/select input | **PLANNED** | Stock input nodes/key behavior exist; GPIO/MCU/transport and evdev mapping require the scoped proof. No native input driver. | #24; #28. |
| Other buttons/power-key/touch/wake | **UNKNOWN** | Stock touch-named nodes do not prove physical touchscreen capability. Full key coverage, long-press and wake routes not established. | #28 and #30; no fabricated touchscreen requirement. |
| Removable SD | **PLANNED** | Metadata distinguishes card from internal eMMC; #26 scopes controller/read-only proof. Native driver/clock/pins/DMA contract missing. | #26; #28. |
| Internal eMMC | **BLOCKED** | Capacity/ordinary partition offsets reconciled; no native controller support contract, safe write/flush path or filesystem integrity qualification. | #28; recovery/rootfs policy #32. |
| Development rootfs and filesystem/data layout | **PLANNED** | Current image is diagnostic initramfs only. Block access, filesystem, writable-data boundaries and recoverable rootfs deployment were absent beyond broad blueprint intent. | Added #28/#32; no distribution/layout chosen. |
| PMIC/battery/charger telemetry | **PARTIAL** | Photographed native PWRAP CID 0x2023/VUSB 0xc000 reads now work; duplicate Android battery fields still conflict. No trustworthy units/calibration, native gauge/charger path or charge policy. | #23; added #30. |
| Thermal sensors/protection | **UNKNOWN** | Sensor mapping, calibration, trips/cooling and safe operating limits unverified. Required before sustained workload qualification. | #30; basic reporting dependency for #28. |
| cpufreq/voltage/OPP and cpuidle | **UNKNOWN** | Minimal config does not establish frequency/voltage transitions or idle states. Shared clocks/rails precede optimization. | #30 with #28. |
| Runtime PM/suspend/resume/wake | **UNKNOWN** | No native suspend/wake, rail retention, storage resume or screen-off-audio proof. | #30; integrates #28/#29/#31 incrementally. |
| MT6582 AFE/ASoC/I2S/DMA | **BLOCKED** | Later stock references suggest second-I2S/DL1. No established native CPU-DAI/DMA/clock/machine route. [Audio](../knowledge/audio-path.md). | Added #29; shared resource contracts #28/#30. |
| CS43131 / headphone / amplifier / speaker routing | **PARTIAL** | Historical cs43131_dac at 1-0030 and aw87559_pa at 1-0058; stock /proc/asound/cards empty. Binding names are not native PCM, electrical routing or upstream applicability proof. Reset/mute/jack/ACCDET/analog levels remain open. | #29 with #23/#30. |
| Wi-Fi | **PARTIAL** | Stock firmware/module names present, not loaded-blob or native-driver evidence. Transport/revision/calibration/regulatory/power contract missing. | Added #31. |
| Bluetooth | **PARTIAL** | WMT/STP and ttyMT2 clues; no standard HCI transport/firmware initialization or native BlueZ/A2DP proof. | #31; audio #29 and power #30. |
| Firmware and per-device calibration | **PARTIAL** | Names and some hashed artifacts exist; exact selected radio blobs/order/compatibility, loading permissions, redistribution provenance and unique-data retention unresolved. | #31 and #32; private raw data stays private. |
| FM where physically supported | **UNKNOWN** | Stock chip-ID/tune leads do not establish useful RF reception, antenna population or this board revision's native route. | Explicit conditional branch #31 feeding blueprint M16, not a universal promise. |
| Recovery and safe acquisition | **PARTIAL** | Owner-proven SPFT/FM history plus documented BOOTIMG-only Android restoration. Exact current image/per-device backups, consistency and independent retention remain incomplete. [Recovery](../knowledge/recovery.md), [actual trial](../knowledge/first-experiment-result.md). | Open M0; #32 owns continuing production/recovery coverage. |
| Updates / rollback / production security | **PLANNED** | Blueprint names signatures/rollback; no reviewed storage/key/threat/interrupt-safe updater design. Unchanged LK does not automatically verify custom payloads. | Added #32; M18–20 platform handoff, not implementation now. |
| Production rootfs / services / non-root app contract | **PLANNED** | No rootfs/udev/service stack yet. ARMv7 ABI, filesystem permissions, startup/restart policy, footprint and privileged-service boundary need design. | #32 (M6); app implementation belongs to native repo M7+. |
| Time/RTC, entropy, identity and diagnostic privacy | **UNKNOWN** | Wall-clock persistence/source, entropy readiness and secret-redacted export policy not yet qualified. No physical RTC assumed. Relevant to TLS, updates and later history. | #31/#32; added explicit cross-cutting coverage. |

## Gaps found and minimum additions

The blueprint already names the major phases; the repository had only M0/M1
GitHub milestones and narrow #22–#27 follow-ups. Audio, product PM, connectivity
and production lifecycle had no durable epic owner. Common clock/pinctrl/I2C,
watchdog/reset, eMMC/rootfs, firmware selection/redistribution/calibration and
conditional FM could otherwise fall between the narrow issues.

Five **deferred planning epics** now retain these gaps. No detailed children or
new GitHub milestone shells are needed today:

| Blueprint phase | Tracking epic | Current phase status / dependency |
| --- | --- | --- |
| M0 Evidence & Recovery | Existing open milestone 1 | **PARTIAL**; owner-accepted experimental exceptions are documented, not a full pass. |
| M1 First Boot | Existing closed milestone 2 | **CONFIRMED core**; broader stability qualification moves into #28. |
| M2 Core Hardware | [Y2H-300 #28](https://github.com/SchulzCode/Y2Linux/issues/28) | **ACTIVE, entry PARTIAL**, under the activation decision above; owns #22–#27 dependencies. |
| M3 Native Audio | [Y2A-300 #29](https://github.com/SchulzCode/Y2Linux/issues/29) | **BLOCKED** on needed core resources; major Gate D before central player/audio work. |
| M4 Power Viability | [Y2P-400 #30](https://github.com/SchulzCode/Y2Linux/issues/30) | **PLANNED**; basic safety begins alongside M2, full suspend/playback integrates later. |
| M5 Connectivity | [Y2N-500 #31](https://github.com/SchulzCode/Y2Linux/issues/31) | **PLANNED**; radio transport/firmware dependencies, conditional FM foundation. |
| M6 Production Y2Linux | [Y2R-600 #32](https://github.com/SchulzCode/Y2Linux/issues/32) | **PLANNED**; storage/recovery/PM gates and later M18–20 platform release obligations. |
| M7–M20 application/product phases | Existing blueprint, no native backlog added | **PLANNED** at blueprint level only. Native implementation is gated; do not manufacture detailed tasks now. |

M4 numbering is not permission to defer basic thermal/power safety until after
audio. M5 radio-specific power measurements depend on radio operation, so these
phases have shared gates rather than a rigid all-or-nothing serial chain. M6
rootfs contracts can mature with M2 without prematurely choosing production OTA.
The original next-five order is retained as history. The activation decision above
prioritizes USB logging #27 and its smallest shared prerequisite; it does not
remove the controller evidence gate.

## When Y2PlayerNative can become the serious workstream

The blueprint's Gate C and especially Gate D remain the immediate platform
threshold. Require an evidenced vertical slice:

1. Repeatable, identified Linux boot with sufficient measured memory, recoverable
   artifacts and usable logs; real timer/syscall behavior and basic thermal/power
   safety, not only one screen report.
2. Stable Linux input and a supported display interface (reviewed framebuffer
   fallback is acceptable initially), readable media storage and a recoverable
   userspace layout with writable app data/permissions defined.
3. Real ALSA/ASoC PCM playback through the intended Y2 headphone path, with known
   clock/rate/format/volume/routing behavior and repeated playback evidence.
4. An ARMv7 Rust/C build/runtime and platform API contract using Linux interfaces,
   with boot supervision and non-root application access scoped. This is later
   design work, not proof supplied by the kernel-only toolchain.

Host-only application models/tests could precede that slice, but should not
become the central device workstream now. Full SMP, maximal RAM, Wi-Fi, Bluetooth,
FM, advanced suspend and OTA are **not all prerequisites for initial wired-player
work**. They remain necessary where applicable for later product promises and
Android retirement under Gates E–I; no feature parity or production readiness is
claimed by M1 completion.

## Standing milestone-boundary rule

**At every major milestone boundary, repeat this gap audit and update the roadmap
from real hardware evidence before proceeding.** Apply it before phase activation,
exit/closure or a material change in hardware/memory/production scope. Root
`AGENTS.md` links this rule so it remains visible in future sessions.

The boundary review must:

1. Compare current blueprint/owner decisions, repository changes, all active
   issues/epics, retained validation and actual hardware results. Record date,
   revision, tested artifact identity when known, evidence origin and limitations.
2. Refresh every coverage row using this classification vocabulary. Separate
   working narrow behavior, untested driver availability and planned work. Preserve
   older failures/history with explicit superseding notes; do not invent missing
   timings, hashes, firmware selection or hardware success.
3. Reconcile dependency/phase gates, accepted risks, unresolved backup/calibration
   requirements and the next smallest blocker. An experiment's accepted risk is
   not a permanent production waiver. Honor all later explicit owner decisions.
4. Link each material gap to an existing issue/epic; add only a missing high-level
   bookmark when necessary. Decompose only the next evidence boundary. Record
   whether the next phase is ready, partially ready or blocked and why.
5. Update this audit, relevant knowledge pages, roadmap and GitHub milestone/epic
   status before proceeding. Hardware authorization remains separate.

An audit is a planning/evidence review, not an automatic test run. Future small
localized code changes use one clean build, affected tests, D08/layout/BOOTIMG
checks and final size/hash. Architecture/memory/packaging changes, new hardware
subsystems, milestone qualification and release candidates use the appropriate
full verification boundary. Do not rehash the unchanged entire source tree or
redo ROM/SPFT provenance without relevant cause. A documentation-only request
never triggers a build or hardware operation to fill a missing audit datum.
