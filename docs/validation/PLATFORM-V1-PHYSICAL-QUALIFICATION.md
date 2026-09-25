# Platform v1 physical qualification

2026-09-25 UTC. Owner-supervised qualification of the already-installed
`out/y2linux-reborn-ui-v1-candidate/`, clarified by the owner after the initial
identity check found a different Reborn from the original plain Platform v1
package. The [owner test plan](PLATFORM-V1-OWNER-QUALIFICATION.md) and explicit
no-flash/no-build/no-production-change limits remain authoritative.

**Session A: FAIL; stopped at baseline. Sessions B/C/D: NOT_TESTED.** Identity
metadata and installed application hashes match the owner-clarified package.
Two mandatory telemetry commands fail, and kernel warning/error-priority
messages trigger the owner's Session A stop rule. No physical or endurance
acceptance is declared. Sessions E/F/G and unattended long runs remain unstarted
and separately gated. Recommended next step: corrective software work and
warning review, then repeat Session A under the resulting exact identity.

## Identity

Authenticated identity read: 2026-09-25 14:23:49 UTC. Boot ID:
`083d4abb-a447-4ac6-a063-e8898c150bca`.

| Field | Observed |
| --- | --- |
| Kernel | `6.18.0-y2linux-platform-v1-candidate-01` |
| Y2Linux release/build | `1.0.0-candidate.1` / `Y2LINUX-PLATFORM-V1-CANDIDATE-01` |
| Y2Linux source | `d04b95aaff713edf943042d97a4c6134ca19fc24` |
| Reborn source | `155608393f6acfc6657f2f2e23cd07d0533479c6` |
| Reborn version/package | `0.1.0-ui-v1-candidate.1` / `Y2LINUX-REBORN-UI-V1-CANDIDATE-01` |
| Rootfs | `2025.02.18-platform-v1.1`; Buildroot `2025.02.18` |
| UI review metadata | `4fe236feaa986dccd014758a860554d18b94fbab` |

All installed versions JSON fields equal the clarified package metadata.
The original target expected Reborn `6c8aa128550ec80addd08ef3145e9d5a846ddf2e`
/ `0.1.0`; qualification stopped at that mismatch until the owner supplied the
actual package path. Metadata matching is not installed-image readback.

Local integrity: plain Platform v1 package 1,112/1,112 checksums; clarified UI
candidate 133/133. UI root SHA-256:
`edcecfaae26b8102c071000b5d2e9e5f84626ad08e3ff5818b050174f301a044`.
Required Platform v1 BOOTIMG SHA-256:
`f7b4a950a0504a411ad72db0aac9398f04dc1ccabd6a6a0a3a7fdab198ca2622`.
These are expected local image hashes, not measured whole installed images.
All four installed application ELF SHA-256 values match this UI candidate's
manifest: `reborn`, `rebornctl`, `reborn-bench`, and `libreborn_media.so`.
Installed platform `cli.py` and `observe.py` hashes also match the inspected
local source; no runtime replacement was made.

Exact `uname -a`:

```text
Linux y2linux 6.18.0-y2linux-platform-v1-candidate-01 #1 SMP @1764542530 armv7l GNU/Linux
```

## Evidence and preliminary results

Private directory (Git ignored, mode 0700):

```text
evidence-private/platform-v1-physical-qualification/20260925T141911Z-owner/
```

Per-command receipts retain host UTC, exact command/SSH arguments, exit status,
elapsed time, stdout/stderr and limitations. `test-register.json` records each
attempted or unexecuted test, exact source/image context, boot ID or its limits,
timestamp, command, result, manual observation, status and evidence path.
`test-register-original-preflight.json` preserves the original-target preflight.
`SHA256SUMS` covers the final private evidence bundle.

The owner reports **“i just see the main menu normal warm”**. This confirms a
visible main menu and subjective normal warmth only. No buttons, playback,
screen cycling or cable action were requested. The annotation time is retained
in `owner-observations.json`; an exact physical observation time was not supplied.

The initial failed SSH command obtained no boot ID. Later diagnostics are
bracketed by the same authenticated boot ID through 14:36:05 UTC; the stalled
status PID was identified in that boot. Device wall time was not verified and
is not substituted for host UTC. No private-key contents, passwords, bond
secrets, NVRAM or calibration were inspected or exported.

| Host UTC | Command / action | Measurement | Status | Evidence basename |
| --- | --- | --- | --- | --- |
| 14:19:11 | `y2-status system --json` through pinned SSH | SSH exit 255: stale host key; no command output | FAIL | `01-system` |
| 14:21:30 | Host USB sysfs and `ip route get 10.42.0.1` | ACM/ECM same USB parent, direct `10.42.0.2` route; reported USB speed 480 Mbit/s, not measured throughput | PARTIAL | `host-usb-topology.json` |
| 14:22:30 | Owner-authorized host-key update | Only Y2 host entry replaced, original retained; existing login key and strict checking preserved | PASS | `host-trust-update.json` |
| 14:22:30–14:23:20 | `y2-status system --json` | 50.050 s timeout; no stdout/stderr; host wrapper code 124 | FAIL | `11-system` |
| 14:23:49 | `uname -a; cat /proc/sys/kernel/random/boot_id; cat /etc/y2linux/versions.json; cat /etc/y2linux/build-id; cat /etc/os-release` | Exit 0, 0.209 s; identity above; authenticated USB shell works | PASS | `18-ssh-identity-diagnostic` |
| After owner clarification | Compare observed versions to UI candidate metadata | All fields equal; original mismatch retained | PASS | `target-clarification.json` |

The new host pin was accepted on explicit owner instruction with direct USB
topology evidence; it was not independently read from a device console.
Authenticated SSH alone does not prove password rejection, USB-only binding,
Wi-Fi exclusion or SFTP readiness. The status timeout and health exception are
diagnosed below; no silent software fix or service restart has been performed.

## Session A evidence and failures

All rows below use the exact UI candidate identity and boot ID above. The
private receipt named in each row contains the complete command, timestamps,
exit status and unabridged output. Manual observation is absent except for the
display/warmth row. PASS applies only to the stated narrow observation.

| Host UTC | Command / observation | Measured result | Status | Evidence basename |
| --- | --- | --- | --- | --- |
| 14:30:57–58 | `timeout -s TERM -k 2 15 y2-platform capabilities` | Exit 0; current capability declarations captured | PASS | `21-capabilities` |
| 14:30:58 | `timeout -s TERM -k 2 20 y2-health --full --json` | Exit 1, 0.786 s; `UnboundLocalError`, no health JSON | FAIL | `22-health-full` |
| 14:30:58–59 | `dmesg -r` | 171,210 bytes; warning/error-priority messages present | FAIL (warning gate) | `23-dmesg`, `kernel-review.json` |
| 14:30:59 | Boot ID, uptime, taint, process and mount reads | Same boot; taint 0; distinct read-write root/data; stalled status query present | PARTIAL | `24-runtime-context` |
| 14:32:48 | Status PID `/proc` state, FDs, wakeup-source table, platform source hashes | PID 1945 sleeping in `pm_get_wakeup_count`; FD 3 is `/sys/power/wakeup_count`; charger wakeup source active | FAIL (status command) | `25-stalled-status` |
| 14:32:48–49 | Boot-stage, application-ready and boot-history JSON reads | Timings below; first-frame record carries current boot ID and Reborn PID/start ticks; prior UI boot marked orderly | PARTIAL | `26-boot-readiness` |
| 14:32:49 | Mountinfo, controller sysfs, `blkid -p`, geometry, ext4 counters, `df -k` | Expected controller/UUIDs/geometry, separate read-write mounts; both error counters 0 | PASS (identity/current mounts) | `27-storage-identity` |
| 14:32:49 | Thermal/power/radio sysfs, ALSA hw_params, meminfo | CPU 48.500°C; PMIC 46.298°C; memory snapshot below; PCM closed | PARTIAL | `28-safety-readings` |
| 14:32:49–54 | Installed ELF `sha256sum`; `timeout -s TERM -k 2 5 y2-audio-contract` | Four matching ELF hashes; query exit 0, no PCM started; constraints below | PASS (hashes/query only) | `29-installed-elfs` |
| Owner observation during this boot | Look at screen and assess case warmth without operating controls | Main menu visible; normally warm | PARTIAL (display/heat only) | `owner-observations.json` |
| 14:34:44 | Exact-identity cleanup of the orphaned diagnostic PID | SIGTERM only to PID 1945 after boot/start-ticks/cmdline/executable checks | PASS (collection cleanup) | `30-collector-cleanup` |
| 14:34:44–49 | `timeout -s TERM -k 2 5 y2-status system --json` | No output; SSH invocation returns 255 after 5.217 s; subsequent independent SSH works | FAIL | `31-post-system` |
| 14:36:03–04 | `y2-platform capabilities` | Exit 0; after-session capability capture | PASS | `32-post-capabilities` |
| 14:36:04–05 | `y2-health --full --json` | Same exception, exit 1, 0.767 s | FAIL | `33-post-health` |
| 14:36:05 | `dmesg -r` | 200,810 bytes; same 28 kernel priority 0–4 entries | FAIL (warning gate) | `34-post-dmesg`, `kernel-review.json` |
| 14:36:05 | Boot ID, taint, uptime, process list and ext4 error counters | Same boot, taint 0, both error counters 0, no lingering status queries; SSH available | PASS (final context) | `35-post-context` |

The required before/after baseline commands were attempted and preserved, but
the baseline is **incomplete** because system status and health produced no
valid JSON. Direct diagnostic reads are supplemental evidence, not replacements
that make the failed public commands pass. Combined shell commands retain errors
even when the last command exits 0: `/proc/1945/stack` was unavailable, and the
queried `/run/y2/services.json` and `/data/reborn/logs/supervisor-last.json` did not
exist. Those absent queried paths alone do not prove a failed production service.

### Confirmed telemetry defects

1. **Health CLI dispatch fails.** In
   [cli.py](../../tools/platform/y2_platform/cli.py), the `network-check` branch
   imports a local `check` inside `main()`. That shadows the module-level health
   function throughout `main()`. The health branch calls this unassigned local
   at line 197, producing the captured `UnboundLocalError`. Owning subsystem:
   platform CLI dispatch. This is independent of physical sensor health.
2. **Status observation can block indefinitely.**
   [observe.py](../../tools/platform/y2_platform/observe.py) reads
   `/sys/power/wakeup_count` synchronously in `cpu()`. The actual process was
   sleeping in `pm_get_wakeup_count` with that FD open. Kernel `wakeup_count_show`
   calls `pm_get_wakeup_count(..., true)`, which waits for active wakeup events
   to finish. The charger wakeup source was active. Owning subsystem: platform
   observation/bounding; this is not evidence that charger wake policy should
   be changed. Snapshot/collection paths sharing `cpu()` require review before
   dependable baseline or endurance collection. The original host timeout left
   its remote read process alive; that exact diagnostic process was cleaned up.

No production file, charger policy, wakeup source, service or kernel parameter
was changed to bypass either defect. No software fix was implemented.

### Kernel findings and stop rule

The retained before/after buffers each contain **28 priority 0–4 kernel log
entries**, including repeated rate-limiting notices. These are not 28 independent
crashes. There are no retained `WARNING:` stack reports, Oops, kernel panic,
`EXT4-fs error` or `I/O error` matches; taint remains 0. Severity-coded warnings
still trigger the owner's stop condition. Relevant messages include:

| Kernel message | Likely owning area / limitation |
| --- | --- |
| `dummy_timer is not functional.` (four entries) | Timer/clockevent configuration; boot-time warning, not a measured reset |
| `cacheinfo: Unable to detect cache hierarchy for CPU 0` | CPU/cache description |
| `mt6323-led: Failed to locate of_node [id: -1]` | PMIC LED driver/device-tree lookup |
| `Kernel memory protection not selected by kernel config.` | Kernel configuration/hardening |
| `mmcblk1: p8 size 4291225599 extends beyond EOD, truncated` | Retained stock partition-layout clipping; not an observed ext4 corruption; no table change attempted |
| `regulator VCAMA doesn't support get_mode`; same for `VRTC` | Regulator capability/query handling |
| `Bluetooth: hci0: broken local ext features page 2` | Bluetooth controller/feature compatibility; pairing/audio remain untested |
| MSDC/OVL/printk callbacks or lines suppressed | Diagnostic rate limiting; limits completeness of some log streams |
| `<wifi> send Wi-Fi Start command` at warning priority | Driver severity convention; not independently a failed association |

An early informational `Temperature check failed (-61)` precedes later readable
CPU/PMIC temperatures; retain it without treating current thermometry as absent.
Early ext4 orphan cleanup is followed by both filesystems reporting clean and
zero current error counters. It is not counted as a new filesystem error.
Charger log samples carry `fault=0x0`; this does not qualify charging. Radio
snapshot reports `error=0 transport_errors=0 recoveries=0`; no packet-loss
percentage or connection success is inferred.

No owner reboots, rescue transition, input cycle, playback, benchmark or radio
test followed the warning review. Session A is incomplete and failed; no
more-invasive session was started. No historical warning is silently waived.

### Narrow positive observations

Boot stages on the observed boot: initramfs 0.62 s; storage discovery 0.91 s;
preflight 7.54 s; fsck 9.12–9.28 s; root/data mounted 9.65 s; switch_root 9.73 s;
platform start 23.498 s; services-started marker 48.485 s; first-frame readiness
record **57.785 s**; application-ready stage **64.718 s**. These are recorded
monotonic boot times, not a new cold-boot timing experiment. Actual library-ready
timing and full service health remain unverified. Historical orderly-shutdown
records do not count as the requested three reboot tests.

Internal eMMC is on `11230000.mmc`, exposing 15,203,328 sectors. Y2ROOT is
`/dev/mmcblk1p5`, UUID `79324c69-6e75-4801-8000-000000000101`, start 166,912,
length 1,679,360 sectors. Y2DATA is `/dev/mmcblk1p7`, UUID
`79324c69-6e75-4801-8000-000000000102`, start 2,104,320, length 1,638,400 sectors.
Both are separate ext4 read-write mounts; `/data` also has `nosuid,nodev`.
Boot fsck reports both clean; both current `errors_count` values are 0 before
and after collection. Available space: root 360,592 KiB, data 730,824 KiB.
This does not measure write durability, throughput or recovery under faults.

Instantaneous memory snapshot, KiB: MemTotal 952,304; MemAvailable 866,608;
LowTotal 723,952; HighTotal 228,352; Cached 53,356; Slab 11,308. Reborn RSS/PSS,
memory growth, workload CPU and frequency/WFI residency were not collected.
CPU 48.500°C and PMIC 46.298°C are die-sensor snapshots, not pack temperature
or a thermal-load qualification. Battery telemetry was 3,893,994 µV; configured
current is not measured battery current or energy.

`y2-audio-contract` accepts S16 stereo 44.1-kHz constraints. It also reports
direct 48-kHz constraints; the product-enabled profile remains **S16 stereo
44.1 kHz only**. S24/S32 and 88.2/96-kHz constraints are rejected. The command
reports `pcm_started:false`; captured hw_params says `closed`. No playback,
unsupported mode or listening test occurred.

## Requested result ledger

| # | Requested result | Finding |
| --- | --- | --- |
| 1 | Exact installed Y2Linux | `d04b95aaff713edf943042d97a4c6134ca19fc24`; release/kernel/build/rootfs above |
| 2 | Exact installed Reborn | `155608393f6acfc6657f2f2e23cd07d0533479c6`, `0.1.0-ui-v1-candidate.1`; metadata and four ELF hashes match owner-clarified package |
| 3 | Session A | **FAIL**, stopped during baseline; narrow observations above retained |
| 4 | Session B | NOT_TESTED |
| 5 | Session C | NOT_TESTED |
| 6 | Session D | NOT_TESTED |
| 7 | Actual internal eMMC speeds | NOT_TESTED; no MB/s or ops/s measurement |
| 8 | Actual SD speeds | NOT_TESTED; no SD benchmark or card-cycle action |
| 9 | fsync p50/p95/p99 | NOT_TESTED |
| 10 | SQLite 1k/10k/20k | NOT_TESTED at each size |
| 11 | Reborn 20k navigation | NOT_TESTED |
| 12 | Idle/playback/scan CPU | NOT_TESTED for each workload |
| 13 | Frequency residency | NOT_TESTED |
| 14 | Memory/RSS/PSS | PARTIAL: instantaneous system memory above; Reborn RSS/PSS and growth NOT_TESTED |
| 15 | CPU/PMIC temperatures | PARTIAL: 48.500°C / 46.298°C snapshot; owner says normally warm; no load/endurance qualification |
| 16 | Wi-Fi association | NOT_TESTED; an active radio/core is not association proof |
| 17 | DHCP | NOT_TESTED |
| 18 | DNS | NOT_TESTED |
| 19 | Wi-Fi throughput | NOT_TESTED |
| 20 | Wi-Fi reconnect | NOT_TESTED |
| 21 | Bluetooth pairing/trust | NOT_TESTED |
| 22 | Negotiated SBC PCM | NOT_TESTED |
| 23 | Audible SBC | NOT_TESTED |
| 24 | Bluetooth reconnect | NOT_TESTED |
| 25 | AVRCP | NOT_TESTED |
| 26 | Wi-Fi + SBC coexistence | NOT_TESTED |
| 27 | USB/SSH/SFTP | PARTIAL: ACM/ECM enumerate, pinned owner-key SSH works, service declares SFTP/USB-only binding; no SFTP transaction, password-refusal or Wi-Fi-side listener probe |
| 28 | Kernel/storage/audio warnings | Kernel messages listed above; no retained Oops/panic/ext4-I/O errors; audio playback warnings/XRUNs unmeasured |
| 29 | Software fixes needed | Health dispatch shadowing and blocking/unbounded status observation; review kernel warnings with owning subsystems; no repair performed |
| 30 | Remaining physical gates | Three reboots and rescue, service/library readiness, full input/display cycles, audible wired audio, USB/SFTP confinement, all B–D and endurance; E/F/G retain their separate gates |
| 31 | Acceptable as Platform v1 Core? | **Not yet:** mandatory baseline tools fail, warning stop rule triggered, and required sessions remain incomplete |
| 32 | Recommended next test/work | Corrective software work and explicit warning review, then repeat Session A with exact resulting versions; endurance, power, OTA and Reborn acceptance are not the next boundary |

Unexecuted checks have no command execution timestamp, measurement or manual
observation. The private register enumerates these as NOT_TESTED, including all
input/audio actions, all three reboots and rescue, storage/SD lifecycle, SQLite
sizes, workload resource comparisons, Wi-Fi state/failure/reconnect/throughput
cases and Bluetooth/SBC/AVRCP/coexistence cases. No short observation is used to
infer endurance or electrical durability.

## Longer collections prepared, not started

Seven `ui-plan-*.json` records were generated locally using:

```sh
python3 tools/platform/qualify.py capture --profile wired-8h \
  --versions out/y2linux-reborn-ui-v1-candidate/metadata/versions.json
```

Profiles: wired 8h; Bluetooth 8h; Wi-Fi + playback; Wi-Fi + Bluetooth;
scan + playback; SD cycles; USB cycles. All report `network_contacted:false`.
No `--run` was used. The original plain-candidate `plan-*.json` files remain
historical and must not be used for this differing source pair.

Execution requires corrected/verified telemetry, relevant short sessions passed,
an exact versions file for the actual resulting installed pair, explicit owner
approval for duration/workload, a fresh private output directory, the existing
owner private-key path and the verified/current host pin. The collector does not
start a workload or operate hardware. Approval for an eight-hour run has not
been requested because these prerequisites have not passed.

Wired requires observed S16 stereo 44.1-kHz media at an owner-selected safe
volume. Bluetooth requires a known paired/trusted peer, actual negotiated SBC
PCM and listening confirmation. Wi-Fi combinations require a controlled AP and
throughput peer plus proven address/route/DNS. Scan requires a declared dataset
and playback workload. SD/USB cycles require scheduled manual observations;
request each removal/reinsert explicitly and use disposable media for surprise
removal. Record media hashes, workload, observations, collection overhead,
counter changes and gaps. Preserve the same stop conditions.

Only documentation and private host evidence were written in this repository.
The host's Y2 public host-key pin was updated on owner instruction. On-device
cleanup only terminated the exact query started by this qualification attempt;
production code, services, hardware policy and protected data were not changed.
The final process snapshot confirms no leftover status query. No build, flash,
production test suite, reboot or long workload was run. Validation is limited
to evidence consistency/hashes, local links and `git diff --check`.
