# Session handoff — 2026-09-13, Storage02 retry

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
