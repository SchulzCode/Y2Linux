# M0 — Evidence & Recovery Baseline

Updated 2026-09-08. Canonical milestone: [Y2Linux M0](https://github.com/SchulzCode/Y2Linux/milestone/1). **Gate: NOT PASSED.**

## Ownership and history

Y2Linux owns all reverse engineering, stock firmware/kernel analysis, hardware evidence, recovery, Linux 6.18 research, kernel/device-tree work and platform bring-up. Y2PlayerNative owns the later native Rust/C application and its application architecture. The local Android reference remains `/home/luca/Dokumente/Code/Y2Player/`.

The first five issues were transferred from Y2PlayerNative without duplication. Original issue URLs redirect to Y2Linux and GitHub preserves the transfer history. The old native-repository M0 milestone was closed as superseded, not passed. The supplied blueprint remains a historical architecture reference; the current repository/role instruction supersedes its assignment of research tasks to an implementation role.

Research owner: investigation, read-only captures, reverse engineering, evidence interpretation, knowledge documents, architecture and precise implementation specifications. Implementation owner: only the bounded intended change after those questions are answered. No current research task is implementation-ready.

## First evidence boundary and results

| Stable ID | Research outcome | Prerequisite review |
| --- | --- | --- |
| [Y2E-101](https://github.com/SchulzCode/Y2Linux/issues/1) | [Provenance audit](../knowledge/evidence-index.md): 24 original-package files hashed, both 93-file snapshots verify, generated-package failures quarantined | Independent passive device inspection allowed; no recovery certification |
| [Y2E-105](https://github.com/SchulzCode/Y2Linux/issues/2) | [Current non-root baseline](../knowledge/stock-system-baseline.md): one authorized target, remote error markers, denied/missing exports explicit | Partition metadata allowed; exact installed image still unknown |
| [Y2E-110](https://github.com/SchulzCode/Y2Linux/issues/3) | [21-entry partition reconciliation](../knowledge/partition-map.md), eight exported starts/counts cross-check, special vendor spans unresolved | Exact packaged boot can be inspected offline; no raw device acquisition |
| [Y2E-115](https://github.com/SchulzCode/Y2Linux/issues/4) | [Boot structure](../knowledge/boot-chain.md): stock component lineage verified, no DT/config markers in bounded search, load-address discrepancy documented | Recovery capability assessment allowed; Linux artifact construction blocked |
| [Y2E-120](https://github.com/SchulzCode/Y2Linux/issues/5) | [Recovery gap packet](../knowledge/recovery.md): package/tool candidates, access/personalization/retention gaps and three next proofs | First research boundary reviewed; M0 remains open |

The issue specifications in `issues/` retain the original bounded goals with corrected canonical ownership/research role. Results above and the linked knowledge documents are authoritative for subsequent decisions. Denied/missing/failed observations can complete an investigation without satisfying the underlying hardware/recovery requirement.

## Coverage contract

| Area | Evidence now available | Remaining evidence before relevant implementation |
| --- | --- | --- |
| Identity/access | Fresh non-root system/build/USB state, errors and permissions | Physical revision, installed image provenance |
| Boot/storage | Package header/components, runtime/vendor map and ordinary filesystem relationships | Loader handoff, memory reservations, special-region semantics and exact acquisition method |
| Audio/FM | Verified archived bindings and exact HAL/kernel derivatives; later route reports indexed | Current lineage, physical routing, reset/supplies/clocks/mute, reception/revision |
| Display/input | Archived controller and evdev capability identities; application behavior | Panel/interface/timings, wiring/reset/wake and current raw-code relationships |
| Power | PMIC historical identity, conflicting current battery service output | Trusted readings/calibration, charging/suspend/wake behavior |
| Wi-Fi/Bluetooth/firmware | Current filename/module metadata, stock loader declarations and vendor analysis | Controller/transport, loaded firmware hashes, calibration/power and upstream applicability |
| Recovery | Stock-family package candidates and existing tool/source artifacts identified | Verified exact device backups, independent copies, compatible tool/DA/host and authorized restoration proof |

## Execution and gates

Use CONFIRMED/INFERRED/UNKNOWN/DISPROVEN with date, task and raw locator. Preserve host UTC, remote status, command/tool version, bytes and hashes. ADB host 0 does not imply remote success. Private artifacts remain ignored and are never staged. Do not automatically execute historical scripts or vendor debug reads.

The first batch used no device writes, reboot, boot-mode change, radio toggle, raw block-device access, downloaded-agent execution, kernel/DTS change or application implementation. Read-only metadata does not make all possible device reads harmless: each later command needs a known interface and bounded scope.

M0 PASS requires a trustworthy device/build/partition/boot baseline, sufficient subsystem evidence and explicit critical unknowns, verified same-device boot/recovery/personalized-data backups with independent retention, a compatible recovery path independent of working Android, and an explicitly authorized bounded stock restoration rehearsal with readback and functional/calibration checks. Preloader/LK/partition-table/calibration writes and formatting stay outside ordinary restoration. Recovery rehearsal is a separately controlled step before experimental Linux work, not a reason to waive recovery prerequisites.

Next rolling-wave research boundary: (1) recovery provenance and acquisition-method semantics; (2) bootloader handoff/address proof; (3) individually scoped passive subsystem evidence, beginning with power-reporting inconsistency. No speculative implementation backlog or M1 tasks were created. Only after the next proof is researched and a concrete intended change exists should an implementation issue be written.
