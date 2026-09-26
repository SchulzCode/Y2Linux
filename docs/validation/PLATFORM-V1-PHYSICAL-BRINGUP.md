# Platform v1 physical bring-up campaign

Started 2026-09-26. ACTIVE: broad census and first root-cause cluster complete;
Fix Batch 1 source validation/build in progress. This is not a completed platform
qualification. [Entry audit](../planning/roadmap-gap-audit.md#complete-physical-bring-up-campaign-entry--2026-09-26).
[Master issue table](PLATFORM-V1-PHYSICAL-ISSUES.md).

## Exact starting target

Authenticated owner USB SSH, existing key/pin, captured 14:31:42 UTC:

| Field | Running value |
| --- | --- |
| Boot ID | `09de0586-278c-4d37-a68f-17ac9952740c` |
| Kernel | `6.18.0-y2linux-platform-v1-candidate-01` |
| Kernel/base source | `d04b95aaff713edf943042d97a4c6134ca19fc24` |
| Platform userspace | `814c2f3470566021c1a9c43037fd868de8b2df9c` |
| Reborn | `155608393f6acfc6657f2f2e23cd07d0533479c6` |
| Platform/rootfs | `1.0.0-candidate.2` / `2025.02.18-platform-v1.2` |
| Build | `Y2LINUX-PLATFORM-V1-TELEMETRY-01` |
| Platform API/layout/data schema | 1 / 1 / 1 |
| Root/data | `/dev/mmcblk0p5` → `/`, `/dev/mmcblk0p7` → `/data`, separate rw ext4 |

Complete versions JSON matches the local Telemetry 01 package. It differs from
`out/y2linux-platform-v1-candidate/` as expected from the later installation;
both comparisons are retained. No whole installed-image hash claim. Exact uname,
cmdline, mounts, releases, status, capabilities, full health and boot evidence
are in private receipts `00`–`05`.

Raw evidence is private and ignored under
`out/platform-v1-physical-bringup/20260926T143142Z-census/`. Each command receipt
records host UTC, elapsed time, command, exit code and separate stdout/stderr.
Device wall time is inaccurate; boot ID plus monotonic counters bind results.
Passwords, keys, bonds, calibration and NVRAM must never enter public receipts.

## Initial safety and observation

Kernel taint 0; both ext4 error counters 0; no retained panic/Oops/WARN-stack or
filesystem I/O corruption signature. Warnings include timer/cache descriptions,
temporary deferred probes, PMIC LED node, hardening configuration, legacy p8
clipping, unsupported regulator get_mode and HCI feature quirk. They require
individual disposition, not an automatic campaign stop.

At 14:32 UTC CPU/PMIC die readings are 54.4/50.391°C. Battery is present and
Charging at 3.854443 V; SDP online, configured charge current 450000 µA and
source allocation 500000 µA. These are not measured pack/input current or pack
temperature. Retained charge/hold history will be analyzed with workload/context
limits. Owner screen/case-temperature observation requested.

Status returns in 2.695 s including SSH; full health in 3.475 s. Health is
DEGRADED due to DNS; SD is not mounted and Bluetooth is off. Query success alone
does not qualify the corresponding hardware functionality.

## Campaign controls

Ordinary failures are recorded and independent work continues. Global stops:
unsafe panic/Oops, filesystem corruption, protected-partition access, repeated
uncontrolled reboot, dangerous charger/battery/heat, lost recovery, or destructive
writes outside dedicated test areas. Scratch writes stay within guarded dedicated
directories. No manual flash, OTA apply, full reset, card swap/surprise removal or
unattended power/suspend run happens without its required owner action/approval.

## Broad census findings

Owner confirms main menu and normal case warmth. Current Reborn/ALSA are paused/
closed; no audio fixture has yet played. GPU and AFE runtime status are suspended.
Audio constraints accept stereo S16/44.1 and S16/48; wider combinations reject.
BlueZ/BlueALSA/reconnect services exist, adapter off, no peer/PCM. OTA is Idle.
The source inventory truthfully contains SBC only; Auto has no qualified codec.

Fresh WPA2-PSK/CCMP association, DHCP address/default route and 3/3 router ping
replies (mean 1.709 ms) work. Router and external-resolver DNS time out; libc
returns EAI_AGAIN. The driver advertises fixed generic TX checksum offload but
never enables its adapter/firmware checksum flags. In four alternating bounded
queries to the same router, normal UDP checksum requests time out twice;
socket-local SO_NO_CHECK diagnostic requests receive valid matching two-answer
DNS responses in 25.1 and 4.2 ms. This isolates the checksum path without changing
network configuration or providing a production bypass. The source repair must
restore normal checked traffic. Linux's [checksum-offload contract](https://docs.kernel.org/networking/checksum-offloads.html)
requires completion of CHECKSUM_PARTIAL, or software fallback. Raw receipts `10`
and `13` retain exact probes. NTP/TLS remain unready on the installed candidate.

Both actual MMC ios snapshots confirm one-bit legacy 13 MHz at 3.3 V. The SD
card enumerates and blkid identifies FAT32, but supplies no UUID, so the platform
refuses to mount it. Card contents/identity have not been modified. Root/data
remain clean. The installed 16-MiB storage benchmark records a 4-KiB buffered
sequential write at 0.498 MB/s including final fdatasync, then fails EINVAL at
cache advice: CONFIG_ADVISE_SYSCALLS is disabled. No read result is claimed from
that run. A separate guarded O_DIRECT diagnostic is being used for real media
I/O; cached rates will not be presented as bus throughput.

The 30.506-second screen-off idle sample (`09`, Wi-Fi connected, charging,
Bluetooth off) shows 7.50% aggregate CPU busy (about 30% of one core), 640 context
switches/s, 477 interrupts/s and 3.77 new processes/s. Frequency residency:
598 MHz 74.34%, 747.5 MHz 1.38%, 1040 MHz 24.29%. WFI wall-time residency spans
87.93–93.65% across four cores. Periodic broadcast operates at HZ100; all local
timers report nohz=0/highres=0. HIGH_RES_TIMERS is disabled, but enabling it
alone cannot repair this: tick_switch_to_oneshot rejects nonfunctional dummy
local clockevents. A supported per-CPU clockevent/wake path is still required. Load includes
blocked charger ADC and supplicant kalIoctl waits, so load average is not CPU
utilization. Collection itself is included and needs a lower-overhead comparison.
Reborn RSS is 22204 KiB/16 threads; PSS is unavailable because PROC_PAGE_MONITOR
is disabled. No memory-growth or thermal-envelope conclusion yet.

First storage-run safety observation: same boot, taint 0, ext4 errors 0/0,
CPU/PMIC 53.1/50.391°C, battery 3.859497 V and Charging. Current-boot retained
charger logs contain 301 observations, voltage 3.083642–3.898608 V, no nonzero
fault field and no hold/full event. Most of that history precedes this campaign
and includes offline charging/source changes; it is not a controlled charge-rate
or SOC calibration curve. Previous-boot tails contain hold/recharge behavior,
which must be analyzed separately rather than merged into this boot.

The following sections record the baseline, then the measured Fix Batch 1 work.
No new-image physical regression acceptance or endurance pass is claimed.

## eMMC measurements before fixes

Receipt `14-emmc-direct`: 16 MiB sequential region, 64 random operations per
size, O_DIRECT flag readback, aligned mmap buffers, exact deterministic data
readback, fdatasync after writes, dedicated guarded `/data/.y2-bench` scratch
removed on completion. MB/s is decimal; timing includes validation/guard costs.
Latency percentiles describe individual operations; throughput includes the final
write drain. Small sample counts make tail percentiles preliminary.

| Operation | MB/s | IOPS | Samples | p50 ms | p95 ms | p99 ms | max ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| sequential_write_4096 | 1.246 | 304.32 | 4096 | 2.974 | 3.204 | 4.072 | 35.260 |
| sequential_read_4096 | 1.293 | 315.68 | 4096 | 2.884 | 3.066 | 3.245 | 12.593 |
| random_write_4096 | 1.275 | 311.37 | 64 | 2.813 | 2.960 | 3.394 | 3.394 |
| random_read_4096 | 1.290 | 314.95 | 64 | 2.905 | 2.978 | 3.138 | 3.138 |
| sequential_write_16384 | 1.505 | 91.89 | 1024 | 10.566 | 10.795 | 12.462 | 18.565 |
| sequential_read_16384 | 1.489 | 90.87 | 1024 | 10.677 | 10.871 | 11.306 | 43.206 |
| random_write_16384 | 1.485 | 90.62 | 64 | 10.616 | 10.746 | 11.077 | 11.077 |
| random_read_16384 | 1.486 | 90.70 | 64 | 10.652 | 10.849 | 15.883 | 15.883 |
| sequential_write_65536 | 1.569 | 23.94 | 256 | 41.244 | 41.905 | 47.954 | 51.732 |
| sequential_read_65536 | 1.556 | 23.74 | 256 | 41.799 | 42.088 | 44.149 | 50.520 |
| random_write_65536 | 1.575 | 24.03 | 64 | 41.204 | 41.333 | 46.567 | 46.567 |
| random_read_65536 | 1.545 | 23.57 | 64 | 41.874 | 44.093 | 49.262 | 49.262 |
| sequential_write_1048576 | 1.598 | 1.52 | 16 | 653.971 | 661.973 | 661.973 | 661.973 |
| sequential_read_1048576 | 1.583 | 1.51 | 16 | 661.925 | 662.547 | 662.547 | 662.547 |
| random_write_1048576 | 1.595 | 1.52 | 64 | 654.004 | 668.819 | 672.210 | 672.210 |
| random_read_1048576 | 1.584 | 1.51 | 64 | 661.791 | 664.072 | 669.255 | 669.255 |

No corruption or direct-I/O readback mismatch was observed. 1-MiB transfers
approach the 1.625 MB/s raw one-bit/13-MHz ceiling. This identifies the current
transport limit; higher width/clock still needs board-specific evidence.

## SD integrity stop and isolation

At host 14:50 UTC, read-only `fsck.fat -n` on the unmounted card returns 1:
primary/backup byte 65 differs (dirty flag), root and boot-sector labels differ,
and the dirty bit is set. It reports 19 files/414 allocated clusters without a
reported crosslink or lost-chain error. Its final statement is "Leaving filesystem
unchanged". No repair was run. The volume serial is exactly zero, confirming why
blkid omits a UUID; the identity rejection is distinct from this integrity gate.

Device workloads are paused under the owner's filesystem-integrity stop rule.
Owner removal of the already-unmounted SD was requested to isolate the suspect
medium; internal ext4 errors remain zero and no internal integrity failure has
been observed. Source investigation continues while awaiting the action. Headphones
have been connected by the owner; no sound has been played yet.

Owner removed the unmounted card and confirmed the normal menu. Receipt `16`
confirms no mmcblk1, same boot, taint 0 and internal ext4 errors 0/0. Device work
resumed on internal storage; the removed card remains gated and unrepaired.
The connected headphone jack still reports off: DT intentionally omits the
codec interrupt while upstream jack reporting depends on that interrupt. This
is an observation gap, not evidence that the owner failed to connect the plug.

## Stock source investigation in progress

The retained hash-identified Y2 FM kernel provides concrete board data:
`msdc_drv_probe` reads platform_data at pdev+0x5c and data_pins at hw+0x14.
The identified mtk-msdc platform devices are 0xc09ac298 (id 0, hw 0xc09fe4b0,
8 data pins) and 0xc09ac3a0 (id 1, hw 0xc09fe450, 4 data pins). Host-function
fields are 0/eMMC and 1/SD respectively. One additional byte-pattern match is
code/driver data and is rejected, not treated as board configuration. Extracts
are private under `out/platform-v1-physical-bringup/stock-reference/`.
Stock/current SDC_CFG width encodings agree; the SD pinctrl group already owns
all six CMD/CLK/DAT pins. Production candidate DT now requests eight/four bits,
retaining 13 MHz, 3.3 V and legacy timing. The minimal first-boot DT stays at one
bit. CMD6 policy permits only SDR width encodings 0/1/2; DDR/strobe/timing/area
changes remain refused. The existing MMC core compares read-only EXT_CSD fields
after widening and can fall back to a narrower width. This is evidence-backed
implementation awaiting a new kernel and physical readback/error regression.

Stock generic_timer_register at 0xc095cbc0 requests per-CPU interrupt 29;
generic_timer_set_next_event writes CP15 CNTP_TVAL/CNTP_CTL. generic_timer_setup
calibrates against jiffies and does not simply assume CNTFRQ. Thus an actual
stock local-timer implementation exists. Current secure/nonsecure boot routing,
frequency and a bounded GPT-backed qualification/fallback must be reconciled
before activating a replacement clockevent. No timer or SPM sequence changed.

## SQLite and library baseline

Installed ARM `reborn-bench` through guarded platform scratch, exact Reborn schema,
scanner and UI; all sizes pass quick_check and exact track counts with zero scan
failures. Fixtures are tiny synthetic WAV files, not a representative compressed
music collection. WAL synchronous=NORMAL; cache is uncontrolled and reopening
is not a cold-storage measurement. All scratch files were removed.

| Metric | 1k tracks | 10k tracks | 20k tracks |
| --- | ---: | ---: | ---: |
| initial_population p50_ms | 88.648 | 7689.380 | 20229.702 |
| batch_commit_64 p50_ms | 4.492 | 4.488 | 4.539 |
| batch_commit_64 p99_ms | 6.342 | 3384.874 | 3363.743 |
| incremental_upsert_batch_64 p50_ms | 4.931 | 5.373 | 5.538 |
| incremental_upsert_batch_64 p99_ms | 6.312 | 3398.820 | 29.429 |
| list_page p50_ms | 6.921 | 45.589 | 93.632 |
| list_page p99_ms | 8.551 | 50.571 | 97.552 |
| artist_lookup p50_ms | 4.891 | 20.533 | 42.097 |
| album_lookup p50_ms | 2.940 | 18.727 | 40.816 |
| folder_lookup p50_ms | 5.824 | 29.661 | 60.559 |
| indexed_identity_lookup p50_ms | 0.106 | 0.109 | 0.130 |
| search_like_query p50_ms | 0.704 | 0.704 | 0.816 |
| wal_checkpoint_truncate p50_ms | 1402.088 | 1703.235 | 1063.489 |
| reopen_first_connection p50_ms | 12.291 | 66.129 | 128.988 |
| startup_full_library_load p50_ms | 36.921 | 354.227 | 677.633 |
| initial_scan seconds | 5.632 | 86.740 | 212.821 |
| incremental_scan seconds | 0.153 | 7.502 | 20.637 |
| db_bytes | 348160 | 3264512 | 6516736 |
| wal_bytes_before_checkpoint | 1775752 | 4177712 | 4177712 |

The 10k/20k jobs take 126.7/304.1 seconds including all workloads and cleanup;
child CPU user+system is 55.6/120.3 seconds; peak child RSS 21816/33936 KiB.
Recorded endpoint CPU die reaches 57.2°C; internal ext4 counters stay zero and
kernel taint remains zero. No endurance/memory-growth claim follows from this.
Long WAL stalls implicate the constrained storage path. Listing latency scales
linearly, while indexed identity lookup stays around 0.1 ms, motivating a
separate exact-query/index A/B measurement before changing SQL.

## Metadata and durability calls

Receipt `23-metadata`: 64 samples each, dedicated guarded scratch. Each sync
call follows a fresh 4-KiB write (file calls) or fresh create (directory call).
Individual latency excludes preparation/identity guards. Reported operations/s
includes those guards and preparation; it is harness throughput, not pure syscall
IOPS. MB/s is not applicable to stat/rename/delete/directory sync.

| Operation | Guard-inclusive ops/s | p50 ms | p95 ms | p99 ms | max ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| create | 76.151 | 6.076 | 7.175 | 8.259 | 8.259 |
| stat | 159.430 | 0.058 | 0.069 | 0.100 | 0.100 |
| rename | 84.641 | 5.409 | 6.392 | 7.205 | 7.205 |
| delete | 85.299 | 5.375 | 5.788 | 7.061 | 7.061 |
| fsync | 35.175 | 19.039 | 22.233 | 56.092 | 56.092 |
| fdatasync | 62.362 | 9.054 | 13.186 | 13.513 | 13.513 |
| directory_fsync | 23.684 | 26.230 | 29.964 | 30.311 | 30.311 |

## Wired short playback and Bluetooth adapter

Owner connected wired headphones, confirmed readiness after quiet mixer readback,
and heard all six alternating three-second 44.1/48-kHz tones cleanly in both ears
with no clicks/pops/static/dropouts. Receipt `26-wired-rates` records six rc=0
aplay runs, RUNNING S16_LE stereo at each exact requested rate, advancing AFE
period counters, and closed PCM afterward. No underrun/XRUN was reported. Master
was -24 dB with -30 dBFS fixtures; initial -60 dB Master and disabled output were
restored. This is short direct S16/44.1 and S16/48/rate-switch evidence, not
sustained playback, analog precision or preserved-24-bit qualification.

Reborn Bluetooth on action succeeds (`25`), adapter powered, BlueZ and BlueALSA
owners stable, A2DP-source:SBC advertised, AVRCP target/controller registered.
No paired peer/PCM exists yet. The owner was asked to put a named headset into
pairing mode; no nearby unidentified device will be paired/trusted automatically.

## USB transfer baseline

Receipt `27` and `usb-transfer-summary.json`: authenticated pinned-key SFTP over
USB ECM, 64 MiB deterministic large file plus 128 × 4 KiB files. `put -f` requests
durable upload; the remote helper also fsyncs and hashes every file. All 129
remote hashes/sizes and all downloaded copies match. Guarded scratch is removed.
Large upload: 1.327 MB/s (50.56 s). Warm-cache large download: 5.727 MB/s (11.72 s).
Small durable upload: 0.112 MB/s (4.68 s); small download: 0.856 MB/s (0.61 s).
These include SSH/session overhead; warm-cache download is not an eMMC read rate.
No disconnection occurred. Manual reconnect/cable/PC sleep cycles are still pending.

## Measured query fix

Receipts `29`, `41`, `43` use the installed SQLite 3.53.4, exact current schema
and SELECT, 20k synthetic rows, guarded disposable database, 12 samples per case
and full result hashes. All variants preserve every returned row and quick_check.
Cached query comparisons isolate SQL work; they are not cold-storage rates.

| Query p50 | Original | Ordered browse index + direct predicates + small album index |
| --- | ---: | ---: |
| page (64 rows) | 94.86 ms | 4.92 ms |
| first / last artist | 42.19 / 42.61 ms | 4.73 / 4.77 ms |
| first / last album | 38.77 / 38.02 ms | 1.46 / 1.43 ms |
| folder prefix | 59.42 ms | 4.91 ms |

An ordered partial index removes the full browse sort; direct bound predicates
permit artist seeks. A small partial album index permits album seeks with a
small result sort. It uses ~1.5 MB less than a second wide ordered index with
similar measured album latency. Allocated live database pages increase from
5,885,952 to 8,122,368 bytes. A 64-row same-title rewrite median rises from
0.77 to 2.58 ms (the wide alternative 3.78 ms); these are not durable transaction
or full scanner measurements. New ARM scanner/population/WAL regression remains
required. Schema stays v1 and old queries remain compatible; migration is
transactional and preserves offline/deleted state.

## AirPods bonding diagnosis

The owner identified AirPods Pro 2 and twice reported connection attempts. The
adapter shows ACL Connected, but Paired/Bonded remain false and BlueALSA has no
PCM. A Pair call earlier returned success and Reborn set Trusted, but only
General/DeviceID storage sections exist; no key content was exposed in reports.
A later Pair was AuthenticationRejected. The source completion handler omits
Connect entirely, leaving the sole automatic reconnect owner inhibited after
the explicit pair intent (PHY-013).

Receipt `34` traces a serialized explicit Connect diagnostic: missing link key,
IO Capability Reply **No Bonding / MITM required**, then remote **Pairing Not
Allowed (0x18)**. BlueZ Adapter1.Pairable is false after its 180-second timeout.
Linux 6.18 hci_io_capa_request_evt forces a non-bonding authentication value when
HCI_BONDABLE is clear, even for an initiating Pair. Reborn never opens Pairable
for Pair. This strongly supports PHY-014; a successful physical saved-bond trace
is still required.

Standard temporary Pairable windows (`40`, `45`) enabled the flag while awaiting
the owner's new pairing action; no Pair occurred during the captured windows.
The 600-second quiet SSH channel hit configured Dropbear `-I 600`; receipt `47`
reconnects successfully on the same boot with taint 0, ext4 errors 0/0 and die
temperatures CPU/PMIC 56.5/52.7°C. Pairable had expired automatically. Receipt
`48` restores the original 180-second timeout, stops the identified orphan
monitor, captures private traces and removes its dedicated RAM scratch. Future
long collectors must emit progress before the SSH idle deadline. This was not
a device reboot or lost recovery path.

Fix Batch 1 gives an explicit Pair a 90-second daemon-enforced Pairable window,
checks Paired and Bonded before setting Trusted, restores previous adapter policy,
then sends Connect while retaining the platform operation lease. Failure, cancel
and timeout restore policy; asynchronous failures reach Reborn status. A private
D-Bus test covers missing bonds, authentication rejection, successful ordering
and cancellation. y2-bt-reconnect remains the sole automatic connection owner.
No SBC, AVRCP, reconnect or coexistence pass is claimed yet.

Receipts `49`/`50` again show Connected=true with Paired=false, Bonded=false
and no PCM. The UI independently collapses Connected into Paired, hiding
Pair & Connect and enabling Use for Audio without a transport. The same batch
now carries explicit paired/bonded/connected/audio-ready presentation fields;
an incomplete link retains Pair & Connect, and audio selection requires the
observed playback PCM. Regression cases include a transport disappearing while
the menu is open. A bounded retry window (`51`) was closed (`52`, Pairable=false)
when the owner requested postponing headphone testing. No further pairing or
listening action is currently requested.

## Fix Batch 1 source and host gate

The [admission audit](../planning/roadmap-gap-audit.md#physical-fix-batch-1-admission--2026-09-26)
refreshes every coverage area before implementation. Besides storage/SQL/BT:
disable uninitialized firmware checksum offload, enable ADVISE_SYSCALLS and
PROC_PAGE_MONITOR, annotate unavailable cache advice without claiming cold reads,
bind capabilities to installed release identity, and mark jack observation null
until codec IRQ qualification. No charger current, voltage, trip, SOC/shutdown,
timer/SPM, VBUS or audio format expansion is included.

Current-source Reborn workspace: 179 tests pass after the UI regression. Platform targeted suite: 84
pass, one emitted-DT case pending actual ARM artifact (not silently accepted).
The initial standalone connectivity test invocation lacked its documented
PYTHONPATH; the corrected invocation passes. New meaningful cases cover every
filter combination/migration, Bluetooth failure/cleanup and unavailable-vs-I/O
fault cache advice. Full production artifact tests, ARM build and physical
regression remain required before candidate acceptance.

The preserving system packager now accepts an explicit, separately validated
Telemetry 01 root-overlay receipt for fallback. It checks the exact old BOOTIMG,
root hash/size, on-image versions, filesystem label/UUID/cleanliness, kernel
provenance and unchanged layout/data schema. The full production manifest still
owns geometry; an overlay is not promoted to a full release manifest. An actual
ext4 fixture verifies acceptance and nine mismatched input cases. The retained
current root (`4a8e520946ad1aaa00df46e9f302463341e4408bba063f43728b8cb8f68bb092`)
passes this check. This prepares a coherent fallback; no flashing is performed.

Packaging the retained base exposed a checksum-inventory validator defect: it
excluded every file named SHA256SUMS, including nested evidence receipts that
the package correctly lists. All their hashes already verified. Correct the
writers/validators to exclude only the top-level self-inventory; nested-receipt
acceptance, tampering and omission now have a regression test. The original
base package remains unchanged and validates with this correction.

The first isolated ARM pass compiled kernel and rootfs, then artifact assembly
stopped because cache symlinks pointed outside the device-isolated build mount.
The worktree now has local source/download cache directories. Inspection also
found the early regulatory archive still pinned to 2026.05.30 while locked
Buildroot 2025.02.18 installs 2026.09.03. Align the early archive to Buildroot's
reviewed SHA256 and require byte-identical signed databases in early/root
userspace. This is a packaging consistency correction, not an explanation for
the independently isolated Wi-Fi checksum fault. Resume the full build from
the final committed source pair, including the additional UI fix.

## Additional bounded decoder measurements

Receipts `53`/`54`, unchanged boot: all 25 decoder fixtures pass, including
24-bit/96-kHz input, six artwork cases and three corrupt/truncated cases. ALSA
stays closed. At each existing cpufreq maximum, three warm-cache repetitions
pass; residency confirms the imposed caps. No OPP or voltage is changed.

| Maximum | Suite median | 1-s FLAC median | 0.25-s 24/96 FLAC median, resampled to 48 kHz |
| --- | --- | --- | --- |
| 598 MHz | 1,885 ms | 56 ms | 41 ms |
| 747.5 MHz | 1,531 ms | 48 ms | 33 ms |
| 1,040 MHz | 1,142 ms | 34 ms | 25 ms |

These short fixtures show substantial decoder headroom at 598 MHz (24/96 input
about 6.1 times real time). They do not qualify sustained playback, EQ/crossfade,
radio coexistence or preserved-24 output. Original schedutil and 1,040-MHz
maximum are restored; internal ext4 errors and taint remain zero. A governor
policy change needs end-to-end workload evidence, not only this microbenchmark.
