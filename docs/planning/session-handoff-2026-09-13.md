# Session handoff — 2026-09-13, Storage02 retry

## Current: Storage03 failed physical boot; one-platform correction active

The owner flashed Storage03 and its screen reached `Y2Linux STORAGE03 RESCUE`:
missing/invalid internal Y2ROOT or Y2DATA, no internal partition rows, USB
preflight -19/configured=0/polls=0, PHY recovery -19/written=0. This supersedes
all pending-flash statements below. No internal-root or USB acceptance exists.
Off-state/cable-dependent response was also reported before the Storage03 flash;
its cause is not established. Charging management is not implemented/qualified.

The owner now requires one production platform: shared drivers/DT/boot logic,
normal Linux storage semantics with permanent range protection, production USB
startup, and diagnostic userspace observing those same interfaces. The current
[scope audit](roadmap-gap-audit.md) records authorization and unchanged gates.
No more separate diagnostic BOOTIMG; do not overwrite frozen Storage03 artifacts.
No physical writes or automatic flash; stop at the next reviewed owner package.

## Latest: Storage03 complete; owner manual flash boundary

[Full 24-point deployment report](../build/y2linux-production-v1-r3-deployment.md).
Build commit 48458605db5f53bd188921d53359e470f1e8bf05, release 0.1.0-storage.3,
package out/y2linux-production-v1-r3. BOOTIMG 5193728 bytes, SHA256
499d7c3f839eeaafc4c3f79d0b38bc9af9ff1589cce368f24f7f9802380e9190.
Y2ROOT/Y2DATA are raw ext4, 512/800 MiB, layout/schema 1 unchanged. Matching
BOOTIMG+Y2ROOT required; Y2DATA template only for explicit initialization, with
preserve-data profile available. No sparse transport or partition-table change.

Owner DIAG01 photos confirm internal MMC at 15269888 sectors, no partitions,
both resolvers failed, USB PREFLIGHT -19/zero polls/initial CHRDET=007b. The exact
rejected MMC command is truncated. Owner cannot provide more device data and
requested correction of all three images. Storage03 handles bounded MMC/FUA/
metadata requests and unchanged user-area reselection, suppresses boot-area
probing, uses serialized eMMC requests, adds guarded MT6582 saved-state PHY
recovery for cable-present startup, and prints compact rescue status without SD.

[Correction rationale](../knowledge/storage03-corrections.md) and
[validation evidence](../build/evidence/y2linux-production-v1-r3/README.md).
33 regression tests, one linked ARM USB test, ARM ABI/ALSA/reader/blkid checks,
rescue syntax, ext4 integrity/identity, kernel/module/DT/newc/D08/BOOTIMG, stock
scatter/restore bounds and hashes pass. No physical success is claimed. The
reported off-state power/button issue has no proven cause or verified fix;
stock loaders/PMIC power-entry/battery controls were not changed. No physical
assistant writes or SPFT execution. STOP for owner manual operation; no more
diagnostic evidence requests before that boundary. Earlier entries are history.

## Current boundary: DIAG01 on-screen observation ready

The latest owner restart/USB connection still produced no Linux enumeration.
Two bounded ACM waits timed out. No repeated restart request or speculative
kernel fix follows. [BOOTIMG-only DIAG01 package/checklist](../build/y2linux-storage-diag-01-deployment.md)
is built in out/y2linux-storage-diag-01 from rescue commit 8bf2d64, retaining exact
Storage02 kernel/module c2db489. Seven tests plus ARM/shell/newc/DT/memory/BOOTIMG/
scatter/hash validation pass. Three screen pages expose cached USB error state,
internal block geometry, allowed root/data signatures and relevant logs.
No block filesystem mounts/repairs or root handover occur in this diagnostic.

STOP for owner manual BOOTIMG-only flash. ANDROID/USRDATA remain unchecked.
Boot unplugged, attach once after at least ten seconds; photograph all pages.
Source/recovery/install details are in the checklist. Actual cause of internal
root discovery failure remains unknown. No hardware acceptance is inferred.

## Latest: rescue failure identified; USB startup instructions corrected

[Rescue console photo](../hardware-evidence/2026-09-13-storage02-owner-flash/rescue-console.md)
shows `missing/invalid internal Y2ROOT or Y2DATA` at about 41.66 seconds. Normal
Buildroot has NOT booted. Earlier no-SD photo establishes Linux fbcon/display,
with repeated empty-slot CMD55/CMD8 timeouts from removable host 11240000.

The existing USB driver rejects initial CHRDET asserted. Boot USB UNPLUGGED,
wait at least ten seconds after Linux starts, then attach once, as AUDIO-02
already required. Assistant instructions to keep the cable connected across
restart were wrong. Tracked install instructions are corrected; frozen Storage02
package documents predate this erratum. Rescue has ACM intent, no SSH service.
A 120-second ACM wait timed out; no corrected-sequence device capture yet.
The actual packaged ARM blkid probes root/data image UUIDs successfully under
QEMU; stock table arithmetic still matches. Need ACM block/driver logs before
choosing a root-detection fix. No speculative firmware change or new flash.

## New owner result: transfer passed; running-system verification pending

[Storage02 owner evidence](../hardware-evidence/2026-09-13-storage02-owner-flash/README.md)
supersedes the pending-retry statements below: manual SPFT Download Only completed
successfully. The owner then reported a black/unresponsive start, followed by
“it worked” after holding Power + Volume Up. USB connected and no SD are confirmed; the exact resulting screen/boot stage
awaits clarification. Later host events show three-second preloader appearances,
without a Linux gadget. Host inspection found no Y2 USB/ACM/
Ethernet, so no SSH or internal-root qualification has yet been obtained.
No new firmware change or assistant physical write. Preserve the installed
Storage02 state for observation; do not automatically reflash or declare success.

## Prepared package and prior boundary


Canonical SchulzCode/Y2Linux. Production Storage / Installation v1 remains ACTIVE,
#33/milestone 3, between current M3 and M4. Start with the
[complete corrected 24-point deployment checklist](../build/y2linux-production-v1-r2-deployment.md)
and [current roadmap/gap audit](roadmap-gap-audit.md).

Source/build commit: `c2db4893b79eca25458b6adc318232c80dd99872`.
Validated package: `out/y2linux-production-v1-r2/`, release `0.1.0-storage.2`.
Evidence: docs/build/evidence/y2linux-production-v1-r2. Select only BOOTIMG,
ANDROID→Y2ROOT.img and USRDATA→Y2DATA.img for first initialization, using the
new package scatter and Download Only. All three matching images are required.

The owner attempted the original package: BOOTIMG transfer/checksum completed,
ANDROID failed 3154 after sparse detection, and USRDATA was not reached.
[Exact DA parser analysis](../hardware-evidence/2026-09-13-storage-flash-failure/README.md)
proves it rejects the FILL chunks emitted by that package. Do not retry those
.spft.img files. Storage02 uses raw ext4 images directly. It also distinguishes
physical eMMC capacity 15269888 sectors from historical Android's 15203328-sector
export. No stock partition boundary or write allowlist expanded; tail not reclaimed.

26 regression tests and ARM ABI/ALSA/shell, kernel/DT/BOOTIMG, ext4 identity,
contents/ownership, raw transport, scatter/manifest/hash checks pass. Synthetic
readback tests reject root corruption and protected-byte mutation; they are not
physical evidence. The new per-owner client key remains private under ignored
out/y2linux-production-private; only its public half is in Y2DATA. Host keys are
generated on device and persisted on /data. No universal production credential.

**Stop for the owner's manual readback/flash.** No assistant physical eMMC write
or SPFT execution. No post-failure boot result is established. Last qualified
running baseline was AUDIO-02 on SD: four CPUs, MemTotal 954384 KiB, ALSA, internal
eMMC disabled. 44.1 kHz clean stands; 48 kHz/LR/repeat and USB reconnect remain
pending/deferred. Do not redo solved M1/M2/M3 research or close storage from SPFT
completion. Native internal boot/write/no-SD/rescue-negative acceptance remains.

Retain the working SD and AUDIO-02 BOOTIMG for BOOTIMG-only Linux fallback.
Verified original FM boot/system/userdata sources support factory restoration;
factory userdata is not a personalized Android backup. Keep independent recovery
access and before/after table/protected readbacks. A baseline captured only after
the failed flash cannot prove preservation across that earlier attempt.

Future OTA/installer contracts use stable layout 1, independent component versions,
allowlists, root/data separation and staged rescue updates. No updater, application,
A/B repartitioning or forced Android cleanup was implemented. M4 power → M5 radios
→ remaining GPU/USB/stability → whole-platform qualification → Y2PlayerNative.
Preserve existing Git/GitHub identity and donor copyright. No attribution trailers.
