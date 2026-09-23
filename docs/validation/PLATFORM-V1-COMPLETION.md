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

### Fresh observation build and measurement continuation

The recipe correction completed the phase-1 build at Linux `82da3f9` / Reborn
`d9ba054`. `out/platform-v1-observation-build` retains kernel/ABI, Buildroot ARM,
Reborn/FFmpeg verification, artifact validation, production tests and an eight
check QEMU userspace pass (`hardware_validation: false`). ARM Python 3.12.14
loaded SQLite 3.53.4/OpenSSL 3.5.8 and exercised fixture status under Cortex-A7
QEMU. Installed target is approximately 74 MiB, including 15 MiB Python standard
library. This receipt does not cover subsequent phase-2 source changes or declare
a final installation package/image qualified.

DONE_SOFTWARE / HOST_VALIDATED: `y2-platform bench-library` invokes the actual
Reborn database, scanner and UI through an inherited, private scratch directory
FD. It supports 1k/10k/20k tracks, optional generated short WAV scanning and
reserve/inode/deadline checks. Reborn commits `8732287`, `17dd464` implement the
benchmark and remove full Track cloning/per-wheel row allocations; track screens
build only visible rows. SQLite schema/cache/checkpoint policy is unchanged.
Host raw records are in `out/platform-v1-host-measurements`: 20k short-WAV scan
11.7 s, incremental 411 ms; wheel p50 before 2.713 ms / after 0.033 ms (1k:
0.129 / 0.002 ms; 10k: 1.334 / 0.016 ms). Host page cache was uncontrolled;
these are neither cold-device nor Y2 budgets. Reborn library benchmark test,
13 UI + 27 runtime tests and four storage tests passed.

`collect` emits bounded JSONL for CPU/residency/memory/thermal/wakeups/services
and optional Reborn metrics. Memory growth starts after a chosen warmup and uses
boot/PID/start-time identity; its slope is not a leak diagnosis. No workload,
radio, suspend or power transition is triggered by collection. The output limit
is 64 MiB; retain output on the owner host for long physical runs.

22 platform tests pass, including real SQLite WAL and checkpoint ENOSPC at an
injected pwrite boundary and recovery of committed rows after abrupt process
exit. The test-only preload shim targets one new private scratch database and
is never installed in the image. This is syscall fault injection, not electrical
power-loss durability. Prior Reborn operational-error/corruption tests remain
applicable; storage replacement tests protect the replacement card via pinned
FDs. SD device-instance identity additionally rejects reused block minors.

### Shutdown and normal low-battery mechanism

DONE_SOFTWARE / HOST_VALIDATED: independent supervised power owner, private
credential-checked request/ack socket, immutable monotonic deadline, bounded
service/journal/sync steps and BusyBox init handoff. Reborn acknowledges only
after audio/session/SQLite shutdown; ordinary commands share the platform path.
Three focused platform tests exercise ignored app, failed sync/init, stale/PID
reuse acknowledgements, daemon restart and synthetic debounce/hysteresis. Reborn
runtime/library/platform suites passed, plus a real database checkpoint/close
acknowledgement test. No final voltage thresholds or charger changes are made.
ARM/image validation is pending. Threshold/reserve and physical power transitions
remain PHYSICAL_GATE; see `architecture/platform-power-v1.md`.

### Wi-Fi, time/entropy and platform lifecycle integration

DONE_SOFTWARE / HOST_VALIDATED: epoch-checked DHCP configuration, passive Wi-Fi
failure monitoring, bounded DNS probes and fresh service readiness, standard NTP,
clock/TLS readiness, persistent per-device seedrng and owner network/throughput
helper. Reborn distinguishes authenticated from Online and rejects stale records.
SD lifecycle now runs independently of Reborn, including removed-device claim
validation and five-attempt failed-unmount bounds. Four network/time and two new
SD tests pass; the new Reborn freshness and SD-instance tests pass. This does not
qualify RTC, Wi-Fi connections, SD removal or performance on Y2.

Fresh phase-2 ARM build at Linux `5049761` / Reborn `17dd464` completed under
out/platform-v1-storage-build; eight QEMU userspace checks and the installed ARM
1k database/scanner/UI benchmark pass. Its first production test run exposed the
locked host libc's lack of the obsolete off64_t name in the test-only ENOSPC shim;
`44cf4ac` uses the explicit 64-bit ABI type. Rerun is required and recorded below.
Phase-3 sources are not covered by this earlier ARM receipt.

### Bluetooth observation and AVRCP foundation

DONE_SOFTWARE / HOST_VALIDATED: native read-only D-Bus observer, typed exact PCM
and runtime/mutual capability distinctions, generated build codec inventory that
rejects unexpected optional encoder enablement. Four focused host tests pass,
including native C compilation/private D-Bus missing-owner execution, peer loss,
wrong direction, owner change, unsupported format and accidental AAC enablement.
AVRCP in Reborn routes only approved BlueZ calls through semantic Actions; two
host tests include real private D-Bus registration, control delivery, metadata
projection and unauthorized-sender rejection. ARM/image proof follows later.

The phase-2 production rerun with current host/tooling source passed 32 platform,
67 production and 38 subsystem tests, plus ARM ABI/ALSA utility smoke. This does
not promote phase-3 sources to ARM_BUILT: their separate fresh build is ongoing at
Linux `ea50d53` / Reborn `ce55f89`, out/platform-v1-services-build.

The phase-3 services build (`ea50d53` / `ce55f89`) completed fresh kernel,
Buildroot ARM, Reborn ARM, FFmpeg/package/ELF validation. Its 32 platform,
67 production, 38 subsystem tests and eight QEMU userspace checks passed.
Receipts are in out/platform-v1-services-build. No physical qualification.

Reconnect now has bounded attempts, persistent same-boot inhibition, shared
user/automatic operation exclusion, and unknown-completion containment. Native
host compilation and six Bluetooth tests pass; Reborn's 34 platform tests pass.
These newer Bluetooth changes still need the next fresh ARM/image receipt.

Codec Auto has a typed eligibility/session policy and actual SelectCodec control
path through Reborn, plus stopped-playback/shared-PCM exclusion. Two policy tests
exercise gate/fallback/peer generation/deadline/unknown-completion bounds; the
real file-lock test proves open PCM and selection exclude each other. Requested
codec never overwrites negotiated PCM. Optional codecs remain unbuilt and
distribution-gated, not claimed implemented. See platform-bluetooth-v1.md.
