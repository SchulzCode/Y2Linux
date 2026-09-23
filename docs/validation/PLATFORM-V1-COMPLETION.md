# Platform v1 completion ledger

Started 2026-09-23. Work is in progress; **Platform v1 Candidate is not yet
declared**. No physical Y2 activity is authorized or performed.

| Repository | Starting HEAD | Entry worktree |
| --- | --- | --- |
| Y2Linux | `5f6b4468fb43605ca1da679420823afa72cea73f` | Modified `docs/planning/roadmap-gap-audit.md`; untracked `docs/CURRENT_PLATFORM_STATE.md`, `docs/planning/platform-review-evidence-2026-09-23.md`. Supplied review content preserved. |
| Y2Reborn | `d9ba0549e6b34eff029bd13f7c33a491fee09e66` | Untracked `docs/architecture/bluetooth-codecs.md`, `docs/audit/`, `docs/review/`; preserved. |

`git status`, `git rev-parse HEAD`, and the latest 30 commits were inspected in
both repositories before edits. Existing configured identities are unchanged.
No reset, clean, forced checkout, history rewrite, push or physical flashing.

The [roadmap](../planning/platform-v1-roadmap.md) orders implementation. The
[September 23 evidence ledger](../planning/platform-review-evidence-2026-09-23.md)
is the starting assessment. Reborn's later `LUNA-CORRECTNESS-CLOSURE-01.md`
supersedes repaired R1–R10 findings; R9B's plug-hidden transport remains limited.

Every capability will retain separate implementation, host, ARM, image, physical
and endurance evidence. DONE_SOFTWARE / HOST_VALIDATED / ARM_VALIDATED /
IMAGE_VALIDATED are workflow labels; PHYSICAL_GATE / BLOCKED_BY_EVIDENCE /
DEFERRED retain their literal limits. No previous image's physical qualification
is transferred to the new candidate.

## Entry audit

Read retained Storage06, GPU-01, CONNECTIVITY-10 and USB reconnect receipts.
Internal boot and root/data identity, narrow wired audio and rendering have
physical evidence. Same-boot deep resume failed; current USB PM ownership already
exists. Association/DHCP/DNS, peer/SBC audio and current charging envelope remain
unqualified. Read-only GitHub inventory confirms #16/#27/#28/#29/#31/#32/#33/#34
open, #30 closed; no tracker change is warranted or made. All implementation is
owner-authorized by the completion request; no planning epic supplies hardware
authorization.

## Implementation and validation

### Telemetry, health, capability and boot record foundation

DONE_SOFTWARE / HOST_VALIDATED for the implemented observation contracts.
`tools/platform/y2_platform` provides versioned JSON, CPU/residency/VM/PSS,
die thermal/power, mount identity/space, Wi-Fi readiness, audio/USB/update state,
capabilities, bounded queries and boot history. Rescue's stale charging statement
is corrected without adding a Python dependency to rescue. Initramfs stages are
instrumented, without removing any identity, fsck or recovery gate.

`python3 -m unittest tests.test_platform_contract -v`: 10 pass, exercising source
disappearance/reinsertion, counter reset, missing measurements, DHCP timeout,
stale DNS epochs, corrupt records, space reserves, previous boot retention,
symlink refusal and subprocess deadlines/output bounds. Full health currently
adds query-only checks; scratch/SQLite integration follows in phase 2.

ARM_BUILT and IMAGE_VALIDATED are pending a fresh build. No physical/endurance
claims. Python target footprint/collection cost need measurement. D-Bus transport
observation, richer service fault counters, time readiness and update readiness
remain implementation work in their dependent streams. Persistent early crash
cause and AP watchdog recovery remain PHYSICAL_GATE / BLOCKED_BY_EVIDENCE;
unclean shutdown is never labelled panic or watchdog without evidence.

The first fresh phase-1 build completed the kernel and ARM ABI selftest, then
exposed an unexercised BlueZ 5.87 headers-only recipe: Python selects it, but the
old recipe installed `lib/*.h` after upstream moved headers to `lib/bluetooth`.
The pinned modernization adapter now updates that path. This failed build is
retained at `out/platform-v1-observation-build/buildroot-build.log`; it is not an
ARM userspace pass. Validation resumes after the focused recipe correction.

### Space, SD lifecycle and storage benchmark tools

DONE_SOFTWARE / HOST_VALIDATED: platform mount claims, failed unmount handling,
read-only unmounted checks, low/critical admission reserves, bounded disposable
cleanup and descriptor-pinned scratch measurements. Added Linux exFAT and pinned
exfatprogs; no controller/electrical change. `python3 -m unittest
tests.test_platform_storage_tools tests.test_platform_contract -v`: 19 pass,
including actual scratch I/O, generation loss, readback, ENOSPC error propagation,
failed eject, exFAT selection, symlink refusal and cleanup exclusions. Tests use
host files and fake mount command responses, not physical block devices.

ARM/image validation is pending for this stream. Physical card lifecycle,
throughput/latency, flush semantics and endurance remain PHYSICAL_GATE. SQLite,
Reborn scaling and long-run resource collection follow; no database policy was
optimized without measurements. Detailed contract: `architecture/platform-storage-v1.md`.
