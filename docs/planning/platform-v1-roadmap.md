# Platform v1 software completion roadmap

## CPU idle owner-flash boundary — 2026-10-03 11:07 UTC

Repeated standing gap audit uses final current-boot receipts21–23 and read-only
GitHub receipt24, alongside the complete CPU evidence and the fresh committed
build pair. Installed baseline remains Linux131ee462/Reborn7f9df397, boot
cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2, taint0. C1 is WORKING on all four cores.
Natural CPU0 parking plus C2 adds7182 entries/50.763741s in60.404076s; controlled
30-second enabled/disabled/re-enabled windows add25.023/0/25.016s SLIDLE,
respectively. eMMC/inserted SD ext4 errors remain0. Owner screen/radio/DVFS/USB
policy is restored; the temporary owned Wi-Fi observer is stopped.

| Coverage/dependency | Current classification | Next boundary |
| --- | --- | --- |
| C1/timer foundation, #28 | WORKING on installed baseline; frozen algorithm | Candidate regression through single automated SSH harness |
| C2/parking/MMC, #28/#33 | WORKING after legitimate quiet/pressure hold; storage integrity/restore pass | Preserve guards; qualify unchanged paths with new clock owners |
| C3/clock/context/CIRQ, #28/#31/#34 | SOFTWARE_READY_NEEDS_NEW_FLASH; no real entry observed | Owner installs one preserving candidate; one guarded entry, then20 only after success |
| Active DVFS/hotplug, #28/#29 | WORKING in bounded baseline tests, all five OPPs/three cycles | Candidate pre/post-dormant voltage/topology/thermal regression |
| Display/USB owner dependencies, #34/#27 | SOURCE_IMPLEMENTED and SOFTWARE_VALIDATED; baseline USB0 leak observed | Real screen-off clock release and USB runtime restore after owner flash |
| Full-system suspend, audio/radio/GPU/storage ceiling/endurance | Prior narrow evidence and unresolved gates retained | No unrelated closure or promotion from CPU idle evidence |

Fresh clean paired source 3dfb5f5/b71b4688 produces kernel6.18.0-y2linux-cpu-idle-01
and root2025.02.18-platform-v1.18. Kernel/config/DT/modules/ABI, fresh Buildroot/
Reborn ARM, production regression364 cases (five locked-host prerequisites
covered by25 packaging-host cases), Reborn238 host cases/clippy/fmt, installed
ARM/QEMU,383 ELF files/1403 dependency edges and release inventory/legal-info
complete. Software evidence is not current-candidate hardware acceptance.
Preserving-package/root/composition/checksum validation and final hash seal pass;
owner-flash handoff is ready. No reuse-userspace.

Active issues#16/#27/#28/#29/#31/#32/#33/#34 remain OPEN; no external status is
changed. CPU work maps to existing#28, owner dependencies to#27/#31/#33/#34.
The next phase is software-ready, physically unqualified until owner flash.
Recovery/fallback uses the exact Hardware02 BOOTIMG/Y2ROOT pair. Memory, ROM/
recovery provenance, partition layout, factory/calibration protection and Y2DATA
preservation remain unchanged. No hardware limitation is inferred from an
untested prerequisite. No milestone is closed, no default-on C3 patch, no flash
or push. See [implementation](../validation/Y2-CPU-IDLE-COMPLETION.md) and
[physical record](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).



## CPU idle completion admission — 2026-10-03
### CPU idle dependency audit — 2026-10-03 08:05 UTC

Current-boot receipts04–11 now physically close C1 and C2 on the installed
baseline: four-core WFI counters/residency and timer continuity pass; natural
parking reaches owner0xe/CPU0; C2 adds7182 entries and50.763741 seconds residency
in60.404075502 seconds (84.0%), with restore failures0. Eight uncached, fsynced
SHA256 cycles per eMMC/SD medium pass in each radio-off run, ext4 counts0.
Three hotplug cycles and all five guarded OPPs pass. C3 remains unentered.
Live screen-off still owns OVL/RDMA/COLOR/DSI and32-kHz mutex references;
inherited AFE clocks remain enabled with zero references. This new evidence
supersedes the admission's pending C1/C2 observations, not unrelated gates.

Exact d53dd75c/3be93a68 MT6582 `dpidle_condition_mask` requires DISP0 0x7ffff
and DISP1 0xf clock quiescence, rather than unconditional DISP-domain power-off.
CPU completion therefore includes the existing Reborn DRM master's screen-sleep
lifecycle, balanced shared display/audio clock ownership, and read-only checks
for safely retiring inactive loader clocks. Active or unknown engines remain
blockers. This dependency is within the owner's explicit CPU clock/wake scope;
it authorizes no new display feature, arbitrary power-domain shutdown, DMA reset,
MMIO userspace access, memory layout, partition, flash or push. Existing GPU
and radio ownership and active performance ceiling remain required regressions.



Owner explicitly authorizes an autonomous CPU/idle implementation and physical
qualification pass on installed `y2linux-baseline-candidate`, with one fresh
`y2linux-cpu-idle-completion-candidate` preserving BOOTIMG/Y2ROOT package.
Only the owner flashes; no push, protected-partition or Y2DATA payload is admitted.

Read-only SSH verifies Linux `131ee4621cd583c955182f994adac5a594fb823f`,
Reborn `7f9df397ab3809d52e2f1073ca93a305246e0c3a`, kernel
`6.18.0-y2linux-baseline-01`, root `2025.02.18-platform-v1.17`, release
`1.0.0-baseline-candidate.1`, boot `cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2`,
taint0, matching the sealed package. This supersedes installed Candidate4
identity only; retained Fix02 passes/failures remain historical evidence.
Private receipts: `out/cpu-idle-completion-physical/20261003T074026Z/`.

Initial current coverage: GPT6/GPT4/PPI29/CNTFRQ13MHz/highres/NO_HZ admitted;
all five guarded OPPs admitted; CPU0-only coordinator-owned topology observed.
C1 registered/enabled with accumulated residency, fresh continuity qualification
pending. C2 registered/enabled but entries0, live APDMA/BTIF blockers while Wi-Fi
runs; bus rejection0 and MMC runtime gating intact. C3 registered/default-off,
entries0; current frequency, domains and clocks reject its read-only preflight.
No new idle-state pass is claimed. Existing storage tuning remains Candidate4.

GitHub active issues refreshed read-only: #16/#27/#28/#29/#31/#32/#33/#34 OPEN;
CPU/timers/idle map to #28 and storage/radio/GPU interactions to #33/#31/#34.
Their older descriptions do not override current source or hardware receipts.
No external issue or milestone is closed or changed. Every unrelated hardware,
audio/distribution/endurance gate retains its existing classification.

Scope: characterize C1 then freeze its proven architecture; decode/fix actual C2
clock/device ownership; resolve every source-backed C3 topology/deadline/CIRQ/PCM/
context prerequisite, without masking blockers or undocumented register writes.
Bounded existing runtime controls and workload/storage/network regression tests
are authorized, with safe abort and state restoration. First C3 entry requires
all real prerequisites; no repeated hammering of a failed deep path. Preserve
GPT6/GPT4/PPI29, schedutil/QoS/thermal, hotplug, all guarded OPPs, exact memory/
partition layout and Hardware02 fallback. Full system-suspend acceptance remains
a separate gate. Coherent source fixes and fresh software/package qualification
precede owner flash and current-candidate physical acceptance.

Track implementation and physical results in
[CPU idle completion](../validation/Y2-CPU-IDLE-COMPLETION.md) and
[physical qualification](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).

### USB clock dependency audit — 2026-10-03

Current-boot receipt12 uses supported USB role `none` over the verified Wi-Fi
observer. MUSB changes active ->suspended and returns active on restoring
`device`; same boot, no source/partition changes. PERI USB0 bit10 nevertheless
remains active in the exact C3 blocker mask. Source confirms the glue adopts
that gate without a CCF consumer. The MT6582 d53 USB PHY's usb_enable_clock
explicitly owns MT_CG_PERI_USB0 through enable_clock/disable_clock.

This CPU completion boundary now admits a balanced USB0 CCF consumer and
MUSB runtime save/gate/enable/restore hooks, preserving attached sessions,
DMA/IRQ refusal, PHY sequencing and ordinary role-based detach/reconnect.
No blocker bit is removed. Only the existing controller is modeled; no host,
VBUS, PHY-retune, power-domain, memory, partition or flash scope is added.
Fresh USB regressions and same-boot role recovery are required. The next C3
harness may intentionally release USB through the existing role interface
while its durable device-side job runs, then restore transport. Radios and
USB may both legitimately prevent C3 until their owners have quiesced.

### CPU idle source/build boundary audit — 2026-10-03

Fresh same-boot receipts04/10/11/12/16/17 now establish four-core C1, useful C2,
physical secondary hotplug, all five OPPs, the USB0 ownership leak, actual44.1-kHz
playback with PlaybackNormal demand/all-core restoration and screen-off/on
userspace transitions. Taint0; mounted eMMC/SD errors0; C3 has not entered.
The source pass preserves the working timer/C1/C2/MMC/DVFS/parking algorithms
and repairs the exact C3 clock/context/deadline/CIRQ/PCM prerequisites. Targeted
native fault tests and fresh Reborn host/clippy checks pass; full fresh software
and package qualification is next. This crosses a software build boundary only:
no new hardware/memory/partition scope, milestone closure, flash or push.
All unrelated full-suspend/storage-ceiling/audio/distribution gates remain open.
One coherent CPU idle preserving candidate and its automated20-cycle harness
are the next deliverables. CPU hardware completion remains pending owner flash.


**Current scope, 2026-10-01:** See [platform state](../CURRENT_PLATFORM_STATE.md)
and the [Fix02 physical result](../validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md).
The owner-flashed Fix02 qualifies the awake CPU platform, but fails acceptance
on full-suspend resume completion, SLIDLE and loaded USB upload. One Fix03
correction candidate is built for the owner's next run; it is not a release.
This roadmap keeps earlier Platform v1/Hardware Final/CPU Final boundary
decisions as history. Their "ready" and "next" language belongs to those
earlier packages.

## CPU Final Fix03 — single correction candidate, 2026-10-01

The [implementation boundary](roadmap-gap-audit.md#cpu-final-fix03-implementation-boundary--2026-10-01)
implements exactly the Fix02 handoff batch. It adds timed resume-window marks
up to `pm_suspend` exit, with the decoded RGU reset cause. It reconciles the
stock bus-DCM baseline for SLIDLE and deep idle, makes the USB storm guard
progress-aware (the Fix02 storm was legitimate throughput), and fixes three
policy defects: wake escalation, SD identity and the DVFS label. The output is
one preserving `out/y2linux-cpu-final-fix03-candidate/` BOOTIMG/Y2ROOT package
with the exact Hardware02 fallback; no flash or push. Next: the owner's single
Fix03 run, then one targeted correction for whichever resume call the timed
ring names.

## CPU Final Fix02 — single correction candidate, 2026-09-28

[Implementation boundary](roadmap-gap-audit.md#cpu-final-fix02-implementation-boundary--2026-09-28)
implements the Fix02 plan without restarting the CPU architecture: MSDC runtime
clock ownership, sustained-idle parking and removal of the recurring
screen-off background work, reconciled PWRAP readiness for 1196/1300 MHz,
awake-proven retained PM diagnostics with a warm-reset backstop, charger-state
discrimination and attributed USB transport faults. One preserving
`out/y2linux-cpu-final-fix02-candidate/` BOOTIMG/Y2ROOT package with the exact
Hardware02 fallback; no flash/push. Next: the owner's single Fix02 run.

## CPU Final Fix01 physical sweep — 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-fix01-physical-admission--2026-09-27)
records exact already-flashed runtime identity through USB SSH, taint0. Owner
authorizes one observation/stress sweep using the existing harness, automatic
independent continuation and bounded C3/suspend only after prerequisites.
No implementation source edits, builds, flashes or pushes. The observation sweep
is now **COMPLETE at safe physical-test scope, FAILED for CPU Final Fix01
acceptance**. See the [physical report](../validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md)
and [closing audit](roadmap-gap-audit.md#cpu-final-fix01-physical-closing-boundary--2026-09-27).
GPT6/GPT4/PPI29/highres/NO_HZ and all real QoS producers pass. Conservative
OPPs/hotplug/WFI/playback/thermal authority remain functional. SLIDLE stays blocked
by retained MMC runtime clocks; automatic parking is not observed. High OPPs now
pass selector-mode recognition but fail PWRAP readiness. First devices-stage
request loses recovery and SRAM does not identify the kernel boundary; later
USB stress loses connectivity while UI remains usable. Two owner restarts occur,
not demonstrated spontaneous resets. C3/full RTC/Power wake remain NOT_TESTED.
No whole-platform promotion or external epic closure. Preserve exact fallback.

Next smallest boundary is one coherent **Fix02 plan**: prove early retained PM
diagnostic safety, resolve actual PWRAP readiness, correct MMC runtime ownership
without changing card rails/storage safety, reassess sustained-idle coordination,
and fix the captured loaded USB recovery defect. The plan is not a new build,
flash, memory-layout change or architecture workstream authorization.


## CPU Final Fix 01 — focused implementation, 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-fix-01-implementation-admission--2026-09-27)
activates the owner's single correction batch using completed physical qualification.
Fix GPT6/GPT4/PPI29 admission, complete Reborn QoS writes, decoded clock ownership
and useful SLIDLE eligibility, actual Y2 PMIC selector mode, full suspend/wake and
persistent stages. Preserve conservative paths and bounded CONSYS retry. Build one
fresh `out/y2linux-cpu-final-fix01-candidate/` preserving BOOTIMG/Y2ROOT package with
exact fallback; no flash/push. No hardware acceptance or external epic closure.

Implementation now reaches the single-candidate build boundary: timer ownership,
Reborn/worker leases, clock lifecycle/coordinator, both legitimate PMIC banks,
persistent suspend stages and clean charger unwind have focused passing tests.
Proceed with fresh broad software qualification; next physical run alone may
accept highres/NO_HZ, actual SLIDLE/high OPP entry and same-boot RTC/Power wake.


Fresh software qualification is complete for runtime Linux0de6e95/Reborn36db186:
215 locked passes plus12 host native tests,192 Rust passes/fmt/strict Clippy,
ARM/QEMU,382 ELF files/1396 dependency edges and105 pinned source packages.
Seal one preserving BOOTIMG/Y2ROOT package with the exact Hardware02 fallback;
physical acceptance remains pending the one SSH qualification sequence.


COMPLETE at software-candidate scope: one coherent CPU Final Fix01, BOOTIMG82b38fd3
and Y2ROOT26aa01ce, preserving validation/exact fallback PASS. See the
[implementation/validation receipt](../validation/Y2-CPU-FINAL-FIX01.md).
Close this correction pass before owner-controlled flashing. Highres/NO_HZ,
SLIDLE/high OPP entry, full RTC/Power wake and SRAM retention remain physical
checks; no epic, automatic sleep or C3 qualification is promoted.



## CPU Final physical qualification — 2026-09-27

[Closing boundary audit](roadmap-gap-audit.md#cpu-final-physical-qualification-boundary--2026-09-27)
and [physical report](../validation/Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md) record
COMPLETE observation sweep, FAILED CPU Final acceptance. Exact flashed pair
ff586df/afcf9ff matches; lower OPPs/schedutil/hotplug/WFI/kernel QoS, native silent
playback and USB/WLAN data work. New GPT6/GPT4/PPI29 admission fails, highres/NO_HZ
are inactive, Reborn device hint writes fail, high-bin voltage admission is gated,
SLIDLE remains blocked and dormant/deadline/CIRQ entry unqualified.

Freezer and battery devices/platform/processors/core pass. Quiescent MUSB resume
avoids storm; isolated CONSYS retry succeeds after a captured mandatory-STP
failure. First full RTC suspend never returns; loss of USB/Wi-Fi recovery ends
risky testing, owner restart restores clean filesystems and baseline runtime.
No same-boot Power/RTC full wake or full device-restoration pass. Keep existing
suspend qualification/C3-default-off controls, retain fallbacks, address the one
prioritized fix batch. No source fixes/build/flash/push or external epic closure.

## CPU Final candidate boundary — 2026-09-27

[Boundary audit](roadmap-gap-audit.md#cpu-final-candidate-boundary--2026-09-27)
records completed CPU Final runtime source Linux `ff586df` / Reborn `afcf9ff`.
Fresh kernel/config/modules, Buildroot/ARM, DT/BOOTIMG/memory, 205 locked regressions
plus 12 native dependency tests, 189 Reborn tests/fmt/strict Clippy, ARM/QEMU,
ELF/dependency and pinned source inventory pass. The DT validation-only correction
`3f86af2` enforces CIRQ/MCU context resources and changes no compiled runtime code.
COMPLETE at software-candidate scope: `out/y2linux-cpu-final-candidate/`. The local
package/data/exact-Hardware02-fallback checks pass; no new hardware qualification.
C1/C2/C3, stock-bin automatic DVFS, QoS, timer/context/CIRQ, suspend/device restore,
diagnostics and recovery options are implemented. C3 remains experimental/default
off, CPU0 with other cores physically off; the exact source report records all
preflight/default limits. Retained hardware receipts and qualifications remain
separate, with no new physical campaign, flash or push.

## CPU Final — implementation pass, 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-implementation-admission--2026-09-27)
uses retained real Hardware02 receipts. Owner authorizes complete CPU/timer/DVFS/
cpuidle/dormant/CIRQ/GPT/suspend software, Reborn semantic hints, bounded fallback
and one fresh `out/y2linux-cpu-final-candidate/` BOOTIMG/Y2ROOT package. Preserve
Hardware Final work and Y2DATA; no flash/push. Implement and source-test independent
paths before full build/package validation. Physical qualification stays independent;
no long qualification campaign or hardware evidence acquisition is part of this pass.

## Hardware Final — single campaign, 2026-09-27

The [activation audit](roadmap-gap-audit.md#hardware-final-campaign-activation--2026-09-27)
confirms installed Hardware 02 (Linux `76bc822` / Reborn `95747e0`) through real
USB SSH evidence. This owner direction supersedes Hardware 03/04/05 and older
per-batch build/flash sequencing. No final capability is inferred from availability.

1. Complete Hardware 02 physical census and broad bounded measurements.
2. Investigate CPU/power/battery, storage/USB, radio/audio and recovery in parallel;
   implement all independently justified corrections on `hardware-final`.
3. Collect required physical actions while independent work continues. Preserve
   truthful electrical/calibration blockers and bounded fallbacks.
4. Audit accumulated changes, run complete current-source host/ARM/image checks,
   and create one `y2linux-hardware-final-candidate` preserving Y2DATA.
5. Stop for one owner installation/qualification cycle; destructive root OTA
   application requires explicit approval. Long endurance and unperformed physical
   cases remain explicit pending evidence, never assumed passes.

[Campaign](../validation/Y2-HARDWARE-FINAL.md),
[capability ledger](../validation/Y2-HARDWARE-FINAL-CAPABILITIES.md), and
[qualification](../validation/Y2-HARDWARE-FINAL-QUALIFICATION.md) own the new scope.
Existing epics remain open. No intermediate image is justified at entry.

## Active hardware capability ceiling campaign — 2026-09-26

Current boundary: [Hardware 02 candidate handoff](roadmap-gap-audit.md#hardware-02-candidate-handoff--2026-09-26).
[Concrete package](../validation/PLATFORM-V1-HARDWARE-02.md) is built and release
validated from Linux `76bc822` / Reborn `95747e0`: source-backed SDR steps,
Inventra DMA, GPT6/PPI29 local timer, tested Reborn sink repair, bounded failure
handling and diagnostics. It is ready for one preserving owner flash with the
exact installed Physical 01 pair as fallback. No new mode is physically accepted.

Recovered suspend evidence proves freezer/devices/platform/processors return,
including CPU3/2/1; USB overflow and radio timeout prevent full restore. A fresh
Power Menu reboot restored pinned SSH and clean storage/taint. RTC standard
UTC write/read/ticking now pass; automatic-write policy, retention and alarm
remain separate gates. Ordinary DNS works while one transient Wi-Fi observer
timeout remains. Post-flash regression and measured 25→50 MHz qualification
precede any capability promotion. Older notes retain their historical boundaries.

The owner's platform-wide ceiling request supersedes incremental feature
bring-up. [Fresh entry audit](roadmap-gap-audit.md#hardware-capability-ceiling-entry--2026-09-26)
binds the work to installed Physical 01 (Linux `198fa7c`, Reborn `feb530f`,
candidate.3, boot `4991ba2c-0571-4649-9f7c-9e0318abb952`). Exact versions match;
IOS reads eight-bit eMMC and four-bit SD at 13 MHz/legacy, taint and ext4 error
counters are zero. The initial regression gate passes, including checked Wi-Fi
DNS/TCP, owner-confirmed wired audio and clean replacement-card workloads.
Physical 01 is the working baseline; no final ceiling or freeze is accepted. Earlier current-target descriptions below are historical.

Continuation receipts `17`–`49` separate storage/query performance, identify
stock CPU table 0 and physically prove GPT6/PPI29. The bounded probe restored
controls and normal reboot returned taint 0. Reborn's PCM metadata repair passes
host regression and temporary ARM SBC playback; owner confirms play/pause.
Original binaries/session were restored. Receipt 49 supersedes the incomplete
SSH-stream interpretation: CPU3/2/1 restart works, while USB and radio restore
fail. The recovered unit is healthy after a confirmed fresh reboot; core/SPM
entry remains unproven. Advance future suspend stages only after local and host
recovery checks.

1. Retain the verified Physical-01 regression baseline; complete matched library,
   RAM/storage transfer, lifecycle and endurance measurements.
2. Investigate all hardware ceilings against retained stock/vendor and current
   physical evidence before selecting changes; preserve conservative fallbacks.
3. Hardware Batch 2: supported eMMC/SD timing, USB transfer/DMA, working local
   timer and observation. Hardware Batch 3: core/idle/suspend/RTC dependencies.
4. Coherent power, radio and audio batches follow their measured prerequisites:
   meter/pack evidence; stable SBC; real DL1 bit transport before higher rates.
5. Build/validate each coherent batch, then one owner flash and broad regression.
   Root OTA/rollback and endurance follow stable storage/power/core. Freeze only
   after the final report distinguishes qualified ceilings from evidence gates.

The owner confirmed menu, clean wired/AirPods tones and a known-good replacement
SD card; Reborn AirPods play/pause works by owner report. No external USB meter
is available presently.
The [ceiling report](../validation/PLATFORM-V1-HARDWARE-CEILING.md) tracks these
limits. Independent investigation continues. Existing issue owners
remain; historical closed #30 does not mean physical power qualification.

## Active physical bring-up campaign — 2026-09-26

The owner's complete campaign supersedes the earlier passive qualification
scope. The [entry audit](roadmap-gap-audit.md#complete-physical-bring-up-campaign-entry--2026-09-26)
reconciles current Telemetry 01 hardware evidence with every coverage area.
Order: broad bounded census → root-cause clusters → coherent fixes → host/ARM/
candidate validation → necessary owner installation → broad regression; repeat.
Independent tests continue after ordinary failures. Only the named safety and
recovery conditions stop all hardware work. No planning row authorizes guessed
SPM, storage clocks, charger limits, VBUS or audio formats.

Current physical target: Linux userspace `814c2f3`, kernel/base `d04b95a`, Reborn
`1556083`, Telemetry 01; newer repository heads and original candidate are distinct.
All historical failures and limited passes below remain evidence, with old
authorization restrictions superseded only by this new explicit owner scope.
The [bring-up report](../validation/PLATFORM-V1-PHYSICAL-BRINGUP.md) and
[master issue table](../validation/PLATFORM-V1-PHYSICAL-ISSUES.md) track progress.
Existing issues stay open; no platform/endurance acceptance is declared.

The [Fix Batch 1 admission audit](roadmap-gap-audit.md#physical-fix-batch-1-admission--2026-09-26)
refreshes all coverage after the broad real-device census. Implement the measured
Wi-Fi checksum, legacy storage-width, benchmark/PSS, library query and Bluetooth
pair/connect fixes as one candidate, then regress the whole baseline. Stock Y2
evidence supports eight/four data lines at the existing storage clock; higher
clocks, timer/SPM activation, charger changes, USB VBUS and wider audio remain
gated. The removed inconsistent FAT card stays isolated. Physical SBC pairing
diagnosis is deferred at the owner's request and the Pairable window is closed;
no pass is inferred from ACL Connected alone.

The [packaging boundary audit](roadmap-gap-audit.md#physical-fix-batch-1-packaging-boundary--2026-09-26)
refreshes all areas with receipts through 56. Linux `198fa7c` / Reborn `feb530f`
compile as one candidate; verify final host/ARM/package receipts and the exact
running-image fallback before owner handoff. The new UI distinguishes a link,
saved bond and audio readiness. Nine decoder suites also pass at the existing
frequency caps, with original policy restored; this does not qualify sustained
playback or justify governor changes. No milestone is closed.

2026-09-23. Owner-authorized software implementation, local commits only; no
physical-device access or flashing. This roadmap does not close hardware epics.
Entry revisions: Linux `5f6b4468fb43605ca1da679420823afa72cea73f`, Reborn
`d9ba0549e6b34eff029bd13f7c33a491fee09e66`.

Software freeze: built pair `d04b95a` / `6c8aa12`, following the
[final boundary audit](roadmap-gap-audit.md#platform-v1-software-freeze-boundary--2026-09-23-utc).
All implemented target contracts below now have final current-source ARM/image
receipts. Host orchestration/inventory tools remain host tools. No row has current
candidate physical or endurance qualification. Optional hardware gates remain
explicit and do not prevent this bounded software candidate.

Read the [entry audit](roadmap-gap-audit.md#platform-v1-completion-entry--2026-09-23)
and [completion ledger](../validation/PLATFORM-V1-COMPLETION.md). Evidence levels
are independent: IMPLEMENTED, HOST_TESTED, ARM_BUILT, IMAGE_VALIDATED,
PHYSICALLY_QUALIFIED, ENDURANCE_QUALIFIED. Workflow labels never imply a higher
evidence level.

| Order | Workstream / tracking | Software boundary | Hardware boundary |
| --- | --- | --- | --- |
| 1 | Telemetry, health, capability/readiness, boot history (#16/#28/#32) | Implemented/host tested; phase-1 and phase-3 ARM/QEMU passed: bounded JSON observation, honest missing data, retained boot stages | Collection overhead, current devices, reset-cause retention |
| 2 | Space, SD, scratch/SQLite/library benchmarks (#28/#29/#33) | Implemented/host tested: stable mount identities, budgets, scratch/library/resource and syscall-fault tools; phase-2 ARM/QEMU including installed 1k benchmark passed | SD loss, I/O tails, electrical durability, 1k/10k/20k target performance |
| 3 | Shutdown, low battery, clock/entropy, Wi-Fi (#28/#30/#31) | Implemented/host tested: independent bounded shutdown, disabled thresholds, DHCP/DNS and clock/entropy; phase-3 ARM/QEMU passed | Thresholds/reserve, charging, RTC retention, DHCP/DNS/reconnect |
| 4 | Bluetooth baseline, AVRCP, codec policy (#31) | Implemented/host tested: observed PCM, bounded reconnect, AVRCP, explicit qualified Auto; fresh ARM/production/QEMU passed | SBC peers/coexistence; optional codecs need distribution and peer evidence |
| 5 | USB transfer/reliability/host feasibility (#27/#28/#32) | Implemented/host tested: authenticated USB-only SFTP, reserve and verified publication; ARM/protocol faults passed | Reconnect/PC sleep, transfer performance; host VBUS/role wiring |
| 6 | Signed staged root updates/recovery, reset/backup (#32/#33) | Final ARM/image validated: signed staging/key policy/offline backup-write-readback/restore, reset/export, ARM fault cases and installed production signature/payload verification | Interruption/recovery; automatic BOOTIMG writes excluded |
| 7 | CPU/idle/suspend and high-resolution audio (#28/#29/#34) | Implemented/host tested queries, precision fixtures and default suspend refusal; wider hardware paths explicitly gated | No speculative SPM/voltage/packing; exact clock/sample/resume proof |
| 8 | Release/security/endurance/candidate (#32/#33) | DONE_SOFTWARE / HOST_VALIDATED / ARM_VALIDATED / IMAGE_VALIDATED: exact paired candidate, source/hash/license inventory, passive endurance tools and owner sessions | PHYSICAL_GATE: owner sessions A–G and qualified operating envelope |

Stock preloader/LK, BOOTIMG rescue, root/data split, protected partition policy,
reserved memory, driver/CONSYS ownership, ALSA/DRM/evdev, FFmpeg, SQLite's single
writer, BlueZ/BlueALSA and bounded workers remain authoritative. No new hardware
or memory scope is authorized by this document. Advance each boundary only after
updating the audit using retained physical evidence; do not repeat unchanged
ROM/recovery provenance. Existing #30 acceptance is preserved, with later adverse
suspend evidence and unqualified charger changes explicitly retained.

## Reborn UI v1 application work — 2026-09-24

The owner now authorizes Reborn UI implementation against the frozen Platform v1
contract before physical platform acceptance. The [activation audit](roadmap-gap-audit.md#reborn-ui-v1-application-activation--2026-09-24)
retains every physical gate and issue status above. Work order: design/input and
navigation; native music/queue screens; service-backed connectivity/system/advanced
observations; focus/error/visual tests; fresh host/ARM/package validation; preserving
UI candidate for owner review. No platform architecture or hardware expansion.
Candidate creation and closure require another evidence audit. No flash.

The [software candidate handoff audit](roadmap-gap-audit.md#reborn-ui-v1-software-candidate-handoff--2026-09-24)
records the final native previews, current-source host/ARM checks and preserving
root package in `out/y2linux-reborn-ui-v1-candidate/`. Application implementation
is complete at software-candidate scope. Platform v1 remains physically unqualified;
the next boundary is owner Session A and the UI qualification checklist, then the
relevant platform sessions. No installation or hardware scope is activated here.

The [bounded graphics memory review](roadmap-gap-audit.md#reborn-ui-v1-bounded-graphics-memory-review--2026-09-24)
permits a shared 4 MiB application font atlas and bounded collection-art worker,
cache and texture. Target RSS/PSS, navigation latency and audio coexistence remain
owner qualification items. Platform reservations and hardware gates are unchanged.

The [UI candidate admission audit](roadmap-gap-audit.md#reborn-ui-v1-candidate-build-admission--2026-09-24)
authorizes fresh ARM application compilation and preserving root packaging after
host checks. Final image and runtime receipts remain required. Platform v1
compiled identities remain distinct from the application/package review heads.

## Physical qualification entry — 2026-09-25

The owner identifies `out/y2linux-reborn-ui-v1-candidate/` as the installed and
intended qualification package. Authenticated versions metadata matches Linux
`d04b95aaff713edf943042d97a4c6134ca19fc24` / Reborn
`155608393f6acfc6657f2f2e23cd07d0533479c6`; local package integrity passes.
The [entry audit](roadmap-gap-audit.md#platform-v1-physical-qualification-entry--2026-09-25)
retains all hardware gates and the original plain-candidate identity mismatch.
The [stop audit](roadmap-gap-audit.md#platform-v1-physical-qualification-stop--2026-09-25)
records **Session A FAIL; B/C/D NOT_TESTED**. Health CLI dispatch crashes and
status observation blocks on wakeup_count; retained kernel warnings trigger the
owner's stop rule. Narrow root/data identity, clean counters, first-frame/main
menu and USB SSH observations are retained in the
[physical qualification report](../validation/PLATFORM-V1-PHYSICAL-QUALIFICATION.md).
Next boundary: separately authorized corrective telemetry work and warning
review, followed by Session A with the exact resulting installed identity.
No production code/build/policy changes, flash, long unattended run or Session
E/F/G operation is authorized by this entry; no capability is promoted.

## Telemetry repair — 2026-09-25

The owner authorizes the [focused repair](roadmap-gap-audit.md#platform-v1-telemetry-repair-entry--2026-09-25):
health CLI dispatch, bounded wakeup-counter observation, targeted regressions,
warning review and a preserving root-only candidate. Flashing remains prohibited.
Reborn, kernel, hardware policy and all physical acceptance gates remain unchanged.
Next handoff requires host/ARM userspace and root-image preservation receipts;
the installed candidate's failed qualification record remains historical evidence.

The [candidate handoff audit](roadmap-gap-audit.md#platform-v1-telemetry-candidate-handoff--2026-09-25)
now records 67 host and 5 packaged ARM/QEMU passes, exact five-file image changes,
filesystem/scatter/inode preservation and the original UI fallback. Source
`814c2f3`, release `1.0.0-candidate.2`, is packaged at
`out/y2linux-platform-v1-telemetry-01-candidate/`. The
[warning review and repair report](../validation/PLATFORM-V1-TELEMETRY-01.md)
retains unresolved timer/cache/hardening and physical gates. Nothing was flashed;
the installed image and its qualification status remain unchanged. Next is the
separately authorized installation/Session A boundary, with stop rules intact.

The owner subsequently reports installing Telemetry 01. The
[physical re-entry audit](roadmap-gap-audit.md#telemetry-01-physical-re-entry--2026-09-25)
resumes identity/baseline verification under the original test limits. Software
receipts do not pre-approve the installed image or exempt existing warnings.

Fresh [physical baseline evidence](roadmap-gap-audit.md#telemetry-01-physical-baseline-stop--2026-09-25)
now verifies exact Telemetry 01 metadata/nine file hashes and both telemetry
repairs on boot `3194fa9d-2dee-4105-86f0-4021580bf8d0`. Status/health return and no
wakeup reader remains. Root/data counters are clean and the owner sees the main
menu with normal warmth. The retained kernel warning rule stops Session A;
Wi-Fi health is FAILED/DEGRADED, so **A FAIL; B–D NOT_TESTED** remains the current
session result. No platform physical/endurance acceptance; next resolve warning
and Wi-Fi readiness gates without assuming new hardware or source authorization.

Read-only follow-up confirms no configured Wi-Fi networks, a responsive
supplicant, and the existing 100 Hz periodic broadcast timer configuration.
Intermittent Wi-Fi query delay remains unresolved. These findings refine the
warning/readiness diagnosis without authorizing a build or changing acceptance.

The owner subsequently connected to their router and requests the
[narrow connection check](roadmap-gap-audit.md#owner-requested-wi-fi-connection-check--2026-09-25).
Association/IP/route/DNS and bounded router reachability may be observed without
advancing the full qualification sessions or changing the network configuration.

That check now observes WPA2 association, a DHCP address/default route, 5/5
router and 3/3 Internet-IP ping replies. DNS queries for two hostnames fail;
the platform observer also retains intermittent supplicant_unavailable. Wi-Fi
coverage is PARTIAL with DNS FAIL, not Online or full Session C acceptance.
Next diagnose DNS/readiness; no network settings or production code were changed.
