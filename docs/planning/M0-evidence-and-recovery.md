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

## Second evidence boundary: Y2E-125

[Y2E-125](https://github.com/SchulzCode/Y2Linux/issues/6) accepts owner-observed successful SP Flash Tool recovery using the FM ROM, hashes all 24 package inputs, and corroborates local tool v5.2032.00. Offline preloader/LK analysis resolves the header/runtime discrepancy and confirms kernel entry, ATAGs, ramdisk relocation and command-line source. The exact v6.18 audit supports a candidate appended-DTB/initramfs strategy with unchanged loaders. See [boot-chain.md](../knowledge/boot-chain.md), [recovery.md](../knowledge/recovery.md) and [linux-6.18-support.md](../knowledge/linux-6.18-support.md).

The smallest remaining artifact-design proof is the safe initial memory envelope/exclusions and resulting ATAG-import policy. Installed-loader/security state, observable console and device-specific backups separately gate an experiment. No implementation handoff is ready.

## Coverage contract

| Area | Evidence now available | Remaining evidence before relevant implementation |
| --- | --- | --- |
| Identity/access | Fresh non-root system/build/USB state, errors and permissions | Physical revision, installed image provenance |
| Boot/storage | Package components/map and traced package preloader/LK ATAG handoff | Safe memory envelope, installed-loader policy, special-region semantics and acquisition method |
| Audio/FM | Verified archived bindings and exact HAL/kernel derivatives; later route reports indexed | Current lineage, physical routing, reset/supplies/clocks/mute, reception/revision |
| Display/input | Archived controller and evdev capability identities; application behavior | Panel/interface/timings, wiring/reset/wake and current raw-code relationships |
| Power | PMIC historical identity, conflicting current battery service output | Trusted readings/calibration, charging/suspend/wake behavior |
| Wi-Fi/Bluetooth/firmware | Current filename/module metadata, stock loader declarations and vendor analysis | Controller/transport, loaded firmware hashes, calibration/power and upstream applicability |
| Recovery | Owner-observed successful same-device SPFT/FM restore; exact ROM and local v5.2032.00 tool hashed | Same-device backups/independent copies, exact DA/settings/entry sequence and power prerequisites |

## Execution and gates

Use CONFIRMED/INFERRED/UNKNOWN/DISPROVEN with date, task and raw locator. Preserve host UTC, remote status, command/tool version, bytes and hashes. ADB host 0 does not imply remote success. Private artifacts remain ignored and are never staged. Do not automatically execute historical scripts or vendor debug reads.

The first batch used no device writes, reboot, boot-mode change, radio toggle, raw block-device access, downloaded-agent execution, kernel/DTS change or application implementation. Read-only metadata does not make all possible device reads harmless: each later command needs a known interface and bounded scope.

M0 PASS requires a trustworthy device/build/partition/boot baseline, sufficient subsystem evidence and explicit critical unknowns, verified same-device boot/recovery/personalized-data backups with independent retention, and a documented known-good recovery path with appropriate checks. The owner's successful same-device SPFT recovery now supplies the actual tested-method evidence; do not demand another full-flash rehearsal merely to reconfirm it. Exact procedure/DA/entry details and backups remain incomplete. Preloader/LK/table/calibration writes and formatting stay outside ordinary restoration. Any later device operation still needs a concrete bounded procedure and authorization.

Next rolling-wave research boundary: safe initial boot memory/exclusions and a fixed DT/ATAG policy. Fill the specific acquisition/runbook gaps using the known-good recovery history; pursue console/power evidence only within safe bounded scopes. No speculative implementation backlog or M1 tasks were created. Only after research fixes a concrete intended change should an implementation issue be written.
