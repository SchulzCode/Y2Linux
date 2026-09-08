# Open unknowns and decision register

Date: 2026-09-08. M0 remains NOT PASSED. Research ownership and repository authority follow the current project instruction, which supersedes earlier implementation-assigned research plans.

| ID | Unknown / evidence needed | Blocks | Current decision |
| --- | --- | --- | --- |
| U01 | Current physical PCB/FM revision and installed boot/system hashes/lineage | Compatible stock fallback selection | UNKNOWN; owner restoration history requested; do not infer from fingerprint. |
| U02 | Trusted vendor origin and relationship between OriginalFirmware, Y2 FM package and modified outputs | Recovery input certification | Catalog separately; matching scatter is insufficient. |
| U03 | Exact flasher/DA/host and non-destructive connection/readback semantics | Per-device acquisition | No tool/device connection or payload execution. |
| U04 | PRELOADER hardware region, EBR span and BMTPOOL semantics | Generic range-based acquisition | Ordinary boot/recovery metadata reconciles; special ranges remain unapproved. |
| U05 | Consistent personalized-data backup and independent retention | Recovery gate | No factory-data substitution; protect filesystems currently writable. |
| U06 | Emergency-mode reachability and actual bounded stock restore proof | Any experimental Linux boot | No deliberate corruption; later operator procedure needs explicit authorization. |
| U07 | Header 0x10000000-family addresses versus runtime 0x80000000-family memory; loader ATAG/DT contract and reservations | Linux image construction | Offline loader research next; no guessed addresses/DTS. |
| U08 | Config/DT absent in bounded searched inputs; compiled board configuration | Minimal kernel/device description | Preserve negative search scope; absence elsewhere unproven. |
| U09 | Conflicting battery service values and trusted physical power readings | Recovery power prerequisites | Passive service/sysfs investigation only. |
| U10 | Current panel/input wiring; audio clocks/reset/power/analog route; radio transport/firmware/calibration | Subsystem implementation | Resolve one subsystem at a time; no implementation-ready tasks. |
| U11 | Actual upstream v6.18 applicability and smallest board delta | Implementation specification | Deferred until evidence and recovery boundaries support a concrete experiment. |

Decision D01: all platform documents/tasks/milestones belong to Y2Linux; the five issues were transferred, not duplicated. Y2PlayerNative is the later application repository.

Decision D02: research, reverse engineering, read-only captures and architectural decisions precede implementation. The implementation role receives only fully researched, bounded changes. No current research task is `state:luna-ready`.

Decision D03: checksum failures quarantine the affected generated distribution claim; independent verified historical evidence and passive reads may proceed after review. A completed observation task can report DENIED/UNKNOWN without passing recovery.

Decision D04: closing Y2E-101/105/110/115/120 means this evidence boundary was assessed. M0 remains open until the [recovery checklist](recovery.md) is satisfied. No speculative implementation backlog was added.
