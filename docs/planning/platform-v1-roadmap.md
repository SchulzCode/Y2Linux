# Platform v1 software completion roadmap

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
