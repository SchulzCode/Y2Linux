# Platform v1 software completion roadmap

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
