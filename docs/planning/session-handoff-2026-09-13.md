# Session handoff — 2026-09-13, Storage05 boot failed

## Current: Storage06 boots internally; prepare owner-key Y2DATA only

Owner flashed the working23552-sector correction. [ACM evidence](../hardware-evidence/2026-09-13-storage06-owner/README.md)
confirms MBR/EBR55aa, p5/p7, ext4rw internal root/data and Buildroot handover.
Dropbear is reachable but rejects the existing owner key. Owner selects
~/.ssh/y2linux_ed25519.pub and confirms only initial state exists on Y2DATA.
Prepare a validated USRDATA-only initialization; preserve the working BOOTIMG
and Y2ROOT. No new kernel build or diagnostic path. Never inspect/copy/package
the private key; authorization stays persistent in Y2DATA across system updates.
All entries below are history.


## Latest: readback verified; stock address correction in progress

Corrected owner readback at21:10:43 matches all six samples, including intact
MBR/EBRs and flashed ext4 identity prefixes. The actual stock FM kernel adds
23552 native EMMC_USER sectors to stock logical block requests and subtracts
prefix/tail from disk capacity. The public donor source had incorrectly suggested
zero offset. [Proof and production correction](../knowledge/storage06-addressing-correction.md).

One BOOTIMG-only production correction is in progress under the original owner
scope. Keep root/data and tables untouched; do not request another readback.
No device/GUI access or assistant flash. Roadmap entry audit is updated; #33
remains ACTIVE, all physical/later gates unchanged. Earlier entries below are history.


## Latest: readback completed; acquisition addresses need correction

The owner completed all six samples at 21:00:03 CEST. The
[retained readback result](../hardware-evidence/2026-09-13-storage05-owner/README.md)
records SPFT success but a failed BOOTIMG identity gate. The assistant supplied
physical offsets; this MT6582 SPFT path actually uses legacy global DA addresses
(`NUTL_ADDR_LOGICAL`, `EMMC_PART_UNKNOWN=0`). The supposed Y2ROOT sample exactly
matches stock SEC_RO. This proves an acquisition-mode mismatch, not corruption
at the intended table/filesystem locations or a Linux LBA translation defect.

The [corrected six addresses](../build/storage05-readback.md) match stock scatter
linear coordinates and the retained PMT report. Change only start addresses in
the same Readback mode; filenames/lengths stay the same. Prior samples and full
logs are retained in evidence-private/20260913-storage05-readback-210003. The
corrected batch has not run. No GUI change, device command, firmware build or
flash was initiated by the assistant in this analysis. #33 stays ACTIVE and all
later gates stay unchanged; no table/root/data rewrite is justified.

## Earlier: Storage05 ACM captured; independent sector bytes still needed

The owner connected the running device and granted `/dev/ttyACM0` access.
[Initial and continuation logs](../hardware-evidence/2026-09-13-storage05-owner/README.md)
retain 578653 bytes and contiguous kernel sequences 0–2735. EXT_CSD context reports
sector-addressed=1, 15269888 sectors, PART_CONFIG=0x48: user-area access bits are zero.
CMD18 sector 0 still returns signature0000 with successful SBC/R1; no eMMC guard
rejection or reported transport error precedes rescue failure at 40.853244 seconds.
Removable-host command timeouts are separate. Normal Buildroot still does not start.

The connection/permission prerequisite below is now satisfied. LOG1 has no shell
or raw-sector command. A [single 13.5-KiB manual SPFT Readback batch](../build/storage05-readback.md)
is prepared in out/storage05-readback with six exact user-area samples and known
source-prefix hashes. It has not run. Match the BOOTIMG/address control before
inferring media corruption from the table samples. No second firmware, table
rewrite, root/data rewrite, assistant reboot or flash. Storage #33 and all later
phase gates remain unchanged. Capture metadata names its retained bytes/hashes;
neither this capture nor the photo proves a whole-image device hash.

## Latest: owner flashed Storage05; internal discovery still fails

[Photo evidence and limits](../hardware-evidence/2026-09-13-storage05-owner/README.md)
supersede the pre-flash entry below. Screen identifies STORAGE05 RESCUE,
15269888 sectors, CMD18 sector 0/4096 bytes/signature0000, SBC8/00000900 and
R1=00000900. No partitions or normal Buildroot. Explicit software SBC did not
solve the physical boot blocker. Source defect fixes are not a proven explanation
of the zero signature. Do not rewrite MBR/EBR or invent a partition/offset fallback.

Host inspection found no Y2 USB/ACM/network device. A request to connect the
already-running rescue for LOG1 evidence is pending. LOG1 is not a shell or
arbitrary-read protocol. Independent sector-byte evidence with verified address
semantics is needed to distinguish incorrect read data from invalid metadata.
A bounded 20-second ACM wait also found no device; no request was sent.
No second firmware candidate or additional flash was prepared. Storage #33 stays
ACTIVE; the original one-candidate scope and all later gates remain unchanged.

## Earlier: one production BOOTIMG-only correction prepared

[Deployment and hashes](../build/y2linux-production-v1-r5-deployment.md),
[source findings](../knowledge/storage05-mmc-correction.md),
[offline evidence](../build/evidence/y2linux-production-v1-r5/README.md).
One kernel candidate from `5401007791dc2d915c95baac31e91fd062c8edc0`, package
out/y2linux-production-v1-r5, Linux 6.18.0-y2linux-storage05. BOOTIMG is 5193728
bytes, SHA256 2af64e0fe0831a038ea681e82b93209bb21360995772a54699b7f0ef6be22efa.
Y2ROOT and Y2DATA are byte-identical Storage04 images and are not packaged again.
SPFT Download Only: MT6582_BOOTIMG_only_scatter.txt, BOOTIMG only; all other rows
unchecked. Fallback is the copied Storage04 BOOTIMG, hash
2f9d27a9d306ef5d49a1378f5063a7178edc18dec777340bf78330a23ac3a4f6,
restoring the observed rescue/ACM state.

Production fixes: explicit checked SBC/CMD23 sequencing, opcode-based one-block
CMD18/CMD25 data type, data/command faults cannot report success, failed partition
selection invalidates context, and valid BusyBox exec handover replaces fatal
`set -i`. Source defects are reproduced; the exact physical zero-signature cause
remains unproved. Upstream Linux parses retained stock tables at exact p5/p7
geometry. No table rewrite, alternate DT, dev MMC behavior or new subsystem.
The hardware DT/config are unchanged except initrd-end/release version.

52 regression tests plus actual ARM ABI/shell, images, manifest, bounds and
package rejection checks pass. Read-only host e2fsck and ARM blkid identify both
existing ext4 images. No physical assistant writes or new hardware evidence.
STOP for the owner's single manual flash, then one boot without SD; expected
normal screen is `Internal system started.`. Storage #33 stays ACTIVE until
physical internal-root/write acceptance. Existing SSH authorization and absent
matching local client key are unchanged. No charging/power/radio/GPU/app/reconnect
work is authorized by this handoff. All older sections below are history.

## Latest observation: owner-flashed Storage04 reaches rescue

[Live evidence](../hardware-evidence/2026-09-13-storage04-live/README.md), obtained
through LOG1 after the owner granted host ACM access, confirms storage04 Linux,
four CPUs and MemTotal 954380 KiB. Internal MMC exposes 15269888 sectors but no
partitions; the driver reports sector-zero signature `0000`. At 41.163161 seconds
the boot enters rescue for missing/invalid internal Y2ROOT/Y2DATA. Normal rootfs
and SSH do not start. USB enumeration and bounded ACM log capture now work;
Ethernet carrier, normal-root access and broader USB behavior remain unqualified.
Physical metadata contents/addressing require read-only verification before any
repair inference. The documented production private SSH key is missing locally;
the development public key does not match the packaged Y2DATA authorization.

Owner identifies the earlier failure to turn on as an empty battery. This is
owner-reported cause, not measured battery telemetry; charging remains absent.
This observation supersedes the earlier pending Storage04 flash/USB statements.
Storage #33 stays ACTIVE, with no M4 activation, build, flash or physical write.

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
