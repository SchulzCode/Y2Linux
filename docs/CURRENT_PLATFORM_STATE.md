Platform v1 progress (2026-09-23): phase-3 kernel/ARM/rootfs and QEMU checks
passed at `ea50d53` / `ce55f89`. Bluetooth observation, bounded reconnect, AVRCP
and qualified Auto are now host-tested; their ARM proof follows separately.
Optional codecs remain absent and distribution/physical gates remain explicit.
USB transfer and signed rescue-root update implementation are next. No device
access or physical qualification occurred. See the completion ledger for scope.

# Current Y2Linux platform state

Platform v1 completion progress (software only): versioned telemetry/health,
space/SD lifecycle, scratch/library/resource measurement, bounded shutdown,
configurable disabled low-battery policy, Wi-Fi DHCP/DNS/readiness and clock/entropy
contracts are now implemented with focused host tests. Separate fresh ARM receipts
exist for the observation and storage source pairs. Later changes still need a
fresh build. The detailed [completion ledger](validation/PLATFORM-V1-COMPLETION.md)
supersedes this review's missing-software findings where explicitly recorded;
physical findings and adverse evidence below remain unchanged. No device flashed.

**Completion-pass update, 23 September 2026:** owner-authorized software work is
active. The [Platform v1 ledger](validation/PLATFORM-V1-COMPLETION.md) and
[roadmap](planning/platform-v1-roadmap.md) now track implementation and evidence.
Telemetry, health, capabilities and boot-history foundations have new source and
targeted host tests; fresh ARM/image qualification is pending. No physical device
was accessed. The original owner review below remains the entry baseline, not a
claim that subsequent software work is absent.

**23 September 2026 — owner review, no implementation or hardware activity.**
Reviewed Y2Linux `5f6b446` and Y2Reborn `d9ba054`; the latest packaged Reborn
code is `dde3c53`. Existing audits and physical receipts were reviewed before
source inspection. Reborn was inspected only at its platform dependencies.

## 1. Current platform state

**Y2Linux is a working embedded integration platform with substantial hardware
support. It has not yet established the operating limits, failure recovery and
repeatability needed for Reborn to depend on it as an appliance platform.**
Keep the existing architecture and qualify its contracts.

Internal Linux/Buildroot boot, persistent data, owner-key SSH, four CPUs,
display/input, accelerated graphics and narrow wired audio have real hardware
evidence. Earlier Reborn builds ran on the device. The September 23 candidate
has recorded host, ARM/QEMU and package passes; its physical qualification is
pending. Successful checks from different images do not constitute one
qualified integrated release.

The [evidence matrix](planning/platform-review-evidence-2026-09-23.md#evidence-matrix)
distinguishes **implemented, host tested, ARM built, physically proven,
measured, endurance tested and unknown for every area**. In practical terms:

| Reborn dependency | What it can rely on today, within recorded scope | Contract still missing |
| --- | --- | --- |
| Boot and persistence | Internal root/data identity; separate `/data`; preserving image packages | Repeatable boot/recovery, bounded service readiness and defined data-loss limits |
| UI and wired playback | Physical display/evdev, Mali rendering, clean stereo S16/44.1 kHz | Current-image mixed-load latency, screen-off endurance and recovery after failures |
| Compute and memory | Four cores; guarded RAM map; frequency and WFI interfaces | CPU/RAM/I/O budgets and behavior under pressure |
| Power | Charging/offline-boot mechanisms; earlier M4 owner acceptance | Current charging envelope, normal low-battery shutdown and repeated same-boot resume |
| Connectivity | Native Wi-Fi scans, Bluetooth adapter power/discovery, USB SSH | Reliable IP/DNS/reconnect, actual SBC transport and radio coexistence |
| Updates | Validated manual BOOTIMG/root packages with fallback copies | An implemented recoverable updater; signed OTA is absent |

The README and several historical summaries are stale. Use the latest
[boundary audit](planning/roadmap-gap-audit.md) and dated receipts. M4 #30 remains
owner-accepted/closed; this does not erase later suspend failures or qualify
subsequent charging changes.

## 2. Strongest parts

- **Storage and boot containment:** controller/geometry/UUID checks precede
  mounts and writes; root/data are separate; BOOTIMG carries matching modules;
  protected partitions and original loaders remain excluded.
- **Standard Linux ownership:** DRM/Lima, ALSA, evdev, MMC, power_supply,
  cpufreq, cfg80211 and BlueZ provide useful application boundaries. Keep
  applications at these interfaces.
- **Evidence and build discipline:** pinned inputs, checked patches, artifact
  manifests, targeted fault tests and unusually explicit physical failure
  records. The latest receipt corrections are already implemented.

## 3. Weakest parts

- **Qualification is fragmented.** Most physical results are short, from older
  images and isolated workloads. No retained integrated endurance pass was found.
- **Failure behavior is incomplete.** There is no normal-runtime low-battery
  shutdown policy or automatic update recovery; deep resume and reliable USB
  reconnect remain open. Service startup and process existence are weaker than
  usable-service readiness and recovery from hangs.
- **Measurements and status lack a common contract.** Collectors and Reborn
  diagnostics exist, but do not provide a comparable platform baseline.
  `y2-status power` still says charging is unimplemented. Some logs disappear
  with a failed boot; kernel-crash/reset evidence is incomplete.

## 4. Biggest unknowns

The largest uncertainty is **how storage, scheduling, memory and power interact
under real application load**. Both eMMC and SD use one-bit, 13 MHz operation;
no retained throughput, fsync-tail or representative SQLite benchmark was found.
Reborn's scan, artwork, database and session writes can contend with playback.

Battery runtime, actual input/net battery current, pack temperature and the
latest depleted-battery charging behavior are unmeasured. Programmed current
limits and die temperatures cannot answer those questions. Deep-suspend power
savings and same-session recovery are also unknown.

Wi-Fi data transfer/reconnect, actual Bluetooth audio, large-library RSS,
pressure/OOM behavior, flash wear and long-run error rates lack qualification.
Clean-machine reproduction of the complete current release is unproven despite
strong input pinning and historical first-boot reproducibility.

## 5. Missing core platform capabilities

The core gaps are shared service and lifecycle contracts:

- **Runtime contract:** versioned capabilities, readiness/error states,
  deadlines, one recovery owner per service, bounded restart/hang handling,
  boot/reset history and a deliberate system-watchdog policy.
- **Persistence contract:** defined durability for rebuildable library data
  versus user state; measured fsync/SQLite behavior; free-space/WAL/cache/log
  budgets; SD removal semantics; backup and schema-compatible restore.
- **Power lifecycle contract:** trustworthy available telemetry, graceful
  low-battery notification/checkpoint/shutdown, charge/thermal operating limits,
  activity leases and proven suspend/wake behavior.
- **Network and release contract:** distinguish radio, authentication, address,
  route, DNS and secure-download readiness; reliable Bluetooth peer/transport
  recovery; time/entropy provisioning; signed, recoverable installation with
  explicit single-slot limitations, least privilege and maintained dependencies.

USB host/OTG is disabled and physically unproven, including VBUS sourcing and
role detection. It is optional future hardware scope, not a prerequisite for
Reborn. A/B, faster storage modes and deeper CPU idle also require evidence and
separate decisions; none is assumed necessary.

## 6. Measure before optimizing

Use one identified reference image and a small common workload set: settled
idle, wired playback with screen on/off, library scan plus playback, then network
traffic plus SBC once those services are qualified. Record host date, boot ID,
image/source identities, media/filesystem, free space, workload and duration.
Report failures, sample counts, median/p95/p99/max and collector overhead.

| Measurement | Existing evidence | Missing baseline and decision it enables |
| --- | --- | --- |
| eMMC/SD and SQLite | Mount/readback evidence; old DB smoke pass | Separate sequential/random reads/writes, file/directory fsync latency, cold/warm scan/query/commit/WAL checkpoint times, write volume and nearly-full behavior. Sizes such as 1k/10k/20k tracks are proposed workloads, not supported-scale claims. Informs SQLite, media concurrency and OTA staging. |
| CPU and wakeups | Four CPUs; sampled frequencies | Per-process/thread CPU, frequency residency, WFI residency, IRQ/context-switch/wakeup rates and UI/audio scheduling latency under load. Informs governor, polling and scheduling changes. |
| RAM | Historical 954660 KiB MemTotal; short 256 MiB allocator pass | Whole-system lowmem/HIGHMEM availability, RSS/PSS, slabs, page cache, reclaim/OOM and growth across scans/playback. Informs application budgets before considering swap or reservations. |
| Thermal and battery | CPU/PMIC temperatures during a two-minute GPU run; older voltage observations | Ambient-correlated steady temperatures, cooling response, actual current/energy, charge gain and screen-off runtime. Requires a separately scoped power procedure; no inferred SOC. |
| Wi-Fi/Bluetooth/USB | Scans, adapter power and historical reconnect timings | Throughput, loss, IP/DNS readiness and reconnect distributions; SBC underruns/latency during Wi-Fi traffic; USB throughput/reconnect with runtime PM. Informs service and power tuning. |
| Boot, recovery and endurance | Isolated root handoff and first-frame timings | Cold/warm/dirty-filesystem boot distributions, service recovery time, persistent crash evidence and repeat lifecycle tests. Progress to a proposed 24-hour mixed workload only after the power envelope is accepted. |

Current kernel configuration lacks PSI, perf events, ftrace and cgroups. Begin
with available proc/sysfs counters and application timing; add diagnostic kernel
features only when a specific unanswered measurement requires them. A 15 ms
application polling sleep is source evidence, not a measured wakeup rate.

## 7. Recommended platform milestone order

These are **five proposed platform outcomes**, mapped to existing tracking;
they do not activate or close milestones. Security and repeatable validation
apply throughout. Initial measurements must remain within an accepted power
envelope; interruption/deep-sleep experiments need their own reviewed boundary.

| Order | Major milestone | Exit evidence and what it unlocks |
| --- | --- | --- |
| 1 | **Observable, recoverable reference platform** — #16/#27/#28/#32/#33 | Exact image/fallback and owner recovery procedure; truthful service health; bounded persistent diagnostics and boot timings. Establishes a baseline that survives ordinary failures and supports every later comparison. |
| 2 | **Durable storage and measured resource budgets** — #28/#29/#33 | eMMC/SD/SQLite latency and capacity report; defined user-state durability and media-loss behavior; CPU/RAM budgets with playback under contention. Unlocks justified performance changes and update sizing. Electrical interruption acceptance follows the power gate. |
| 3 | **Qualified power lifecycle** — #28/#34, preserving accepted #30 | Current charging/thermal evidence, graceful low-battery handling, USB PM recovery, repeated same-boot wake and measured idle/screen-off behavior. Unlocks longer battery operation and endurance qualification. |
| 4 | **Complete connectivity services** — #31 | WPA authentication through DHCP/DNS/data/reconnect; fresh Bluetooth pairing/trust/SBC/reconnect with concurrent Wi-Fi; truthful degraded states and time readiness. Unlocks dependable remote delivery and wireless playback. |
| 5 | **Maintainable releases and recoverable updates** — #32/#33 | Repeatable paired-source builds, dependency/security/license inventory, scoped privilege reduction and integrated endurance evidence. Prove signed local installation/recovery before adding OTA delivery. Preserve single-slot recovery limits; do not promise automatic recovery from a torn BOOTIMG. |

## 8. The NEXT 3 platform tasks

1. **Create the reference telemetry and health record.** Consolidate existing
   collectors/status into one bounded, versioned capture; correct stale status;
   identify the reference image and fallback; produce an attended idle/playback
   baseline with boot IDs and measured collection cost. Exit: one comparable
   report, explicit unavailable counters, and an independently usable recovery
   route. This is the first future implementation/qualification task.
2. **Characterize storage using Reborn's actual persistence workload.** Use
   disposable files and a disposable database on approved data/SD filesystems.
   Measure fsync tails, scan/query/WAL behavior, write volume and RSS while audio
   runs. Exit: a supported workload/free-space budget and a durability decision,
   before changing bus speed, filesystem or SQLite policy. No raw-device stress.
3. **Close the normal-runtime low-battery contract.** Assign the platform the
   warning/inhibit/shutdown decision and Reborn the bounded checkpoint/stop
   acknowledgement. Specify behavior when voltage data is unavailable, storage
   is full or an app hangs; validate software failure paths first. Physical
   thresholds and shutdown reserve require measured pack behavior under a
   separate attended power scope. Boot-only voltage admission is insufficient.

Full evidence, measurement limits and source references:
[platform review ledger](planning/platform-review-evidence-2026-09-23.md).
No production code, device state, hardware policy, GitHub issue state or Git
identity was changed by this review; no builds, tests or benchmarks were run.
