# Session handoff — 2026-09-13, Storage02 retry

## Latest: Storage03 correction in progress

Owner supplied all three DIAG01 photographs, then requested correction of the
three production images without further device data. Internal MMC capacity is
15269888 sectors, no partitions, both resolver failures; USB preflight -19 with
initial CHRDET=007b, wake refusal and zero polls. Exact rejected MMC opcode is
truncated in the photo. The owner reports cable-dependent power entry and
SD-dependent apparent progress. See [source-backed correction scope](../knowledge/storage03-corrections.md)
and the current roadmap audit. Earlier diagnostic manual boundary is superseded.
No internal-root or USB acceptance. No assistant physical writes or SPFT execution.

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
