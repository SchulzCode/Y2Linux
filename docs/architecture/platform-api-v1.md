# Platform API v1

The machine interface is `y2-status --json` (or `y2-platform status`),
`y2-health --json`, and `y2-platform capabilities`. JSON is the default; the
`--json` flag is accepted for explicit clients. Schema names carry `/v1`.
Unknown added keys must be ignored. Missing measurements are `null`, empty
collections mean no interfaces observed, and neither implies a measured zero.
`y2-status cpu|memory|thermal|power|storage|wifi|bluetooth|system|audio|usb|readiness`
selects one section while retaining record identity. Rescue keeps a separate
minimal shell status tool, usable without Y2ROOT or Python.

Each record includes UTC wall timestamp, monotonic nanoseconds, boot ID, Linux
commit, Reborn commit when packaged, kernel, rootfs release, build ID, workload,
parameters, units, result, failure and evidence level. Wall time is not trusted
merely because it is formatted as UTC. Source identity comes from the installed
build receipt. Observation does not promote a release's evidence level.

CPU times, frequency and idle residency, IRQs, context switches, VM and block
counters are cumulative. `--interval 0.1` through `10` samples CPU deltas; without
an interval utilization is null. Guests are not double-counted. Counter reversal
or a newly online core has no utilization sample. PSS is opt-in:
`y2-status memory --pid PID --pss`. KiB, kHz, microseconds, microvolts,
microamps and millicelsius are identified in the response. Charge limits are
configured values, never inferred net battery current, SOC or pack temperature.

Storage reports one exact mount, controller, UUID, boot-scoped mount generation
and filesystem space. A surviving mountpoint is not a source. Missing block
identity, stacked mounts, a mismatched internal UUID, read-only mounts and missing
statvfs are distinct errors. Clients must recheck UUID and generation before
destructive reconciliation. Free-space policy is Normal / LowSpace /
CriticalSpace / ReadOnlyRisk / Failed / Unavailable. Low is below max(96 MiB,
10%); critical is below max(32 MiB, 5%) or no free inodes. These are software
capacity reserves, not qualified capacity/performance claims. This interface
does not automatically delete any files.

Readiness is Ready / Starting / Degraded / Failed / Unavailable. Wi-Fi's detailed
state separately reports Off / Starting / Scanning / Associating / Authenticated /
AcquiringIP / Online / Failed. Online requires authentication, an address, default
route and a successful DNS probe younger than 120 seconds, on the same boot,
address and association. Configured DNS servers and a live PID cannot establish
Online. Radio and connection owners remain wpa_supplicant and y2-bt-reconnect.
Unavailable Bluetooth observation remains explicit until the D-Bus observer is
installed; codec requests must never be interpreted as negotiated output.

Capabilities distinguish implemented, enabled and qualified. Enabled radio
preferences are resolved at query time. Physical evidence for an older narrow
profile is named separately; current image qualification is false until the
owner supplies matching evidence. Driver support does not enable USB host,
deeper idle, optional codecs, high-resolution audio, low-battery thresholds or
automatic BOOTIMG updates.

`y2-health` checks identity, space, filesystem error counters, nodes, sensors,
USB and service readiness. It returns OK / DEGRADED / FAILED / UNAVAILABLE with
per-check reasons; exit 1 means a failed check. `--full` adds bounded ALSA and KMS
capability queries; it does not play audio, change display modes, connect radios
or invoke physical actions. Additional explicit scratch tests use the benchmark
contract. Interface presence is a narrow observation, not physical qualification.

Boot stages retain initramfs, discovery, preflight, fsck, mounts and switch_root
timing in `/run/y2/boot-stages.jsonl`. A durable, bounded eight-boot journal in
`/data/system/platform/boot.json` retains identity, count, last stage, previous
boot ID and orderly-shutdown marker. Three unclean predecessors recommend
recovery; no automatic reboot follows. An unclean record does not distinguish
power loss, panic, reset or removal of power. Watchdog sysfs and pstore inventory
are observations only; no AP watchdog is armed and no RAM region is reserved.
Failures before validated writable data cannot be promised persistent retention.

The interpreter uses only the standard library; no pip or network packages are
installed. Files are size-bounded, PID requests capped at 32, and subprocesses
have output caps and deadlines. A kernel task stuck in uninterruptible I/O cannot
be made time-bounded by a userspace timeout. Full status is an on-demand tool,
not a high-frequency polling daemon; benchmark records include collection cost.
