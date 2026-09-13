# Storage05 production BOOTIMG-only correction

**Physical result: FAILED internal boot.** The owner's
[Storage05 photograph](../hardware-evidence/2026-09-13-storage05-owner/README.md)
shows sector-zero signature0000 and missing root/data after the manual flash.
The intended boot result below did not occur. Do not repeat this flash as a fix.
No successor image was built; preserve the running device for read-only evidence.
The frozen package predates this photo; this report supersedes its pending status.

One candidate was built. Source commit
`5401007791dc2d915c95baac31e91fd062c8edc0`, Linux `6.18.0-y2linux-storage05`,
release `0.1.0-storage.5`. Later evidence/test-runner commits do not change its
kernel, DT, rescue or image bytes.

## Findings and correction

[Storage04 ACM](../hardware-evidence/2026-09-13-storage04-live/README.md) proves
expected capacity and a successful-looking sector-zero read with signature0000,
followed by no partitions and rescue. Physical metadata corruption is unproved.
Source execution reproduces one-block CMD25 encoded as a single-block protocol,
DMA faults reported with success/4096 transferred bytes, and fatal BusyBox `set -i`
before switch_root. None by itself establishes the physical zero-signature cause.
The vendor driver supports AutoCMD23; do not call that silicon facility broken.

The production host now explicitly sends and checks software SBC/CMD23, preserves
count/FUA/tag semantics, uses the correct CMD18/CMD25 protocol for one or many
blocks, propagates DMA/command errors, and invalidates failed partition-selection
context. Rescue uses a supported exec-failure trap and normal switch_root.
Reads keep Linux LBAs without a vendor/scatter offset. Normal writes remain
allowed wholly within ANDROID/USRDATA; protected regions and table writes remain
refused. The actual Linux 6.18 parser needs no compatibility change for retained
stock MBR/EBR metadata. [Full comparison and limits](../knowledge/storage05-mmc-correction.md).

## Immutable artifacts

Paths are relative to /home/luca/Dokumente/Code/Y2Linux.

| Role | Path | Bytes | SHA256 |
| --- | --- | ---: | --- |
| New BOOTIMG | out/y2linux-production-v1-r5/BOOTIMG.img | 5193728 | 2af64e0fe0831a038ea681e82b93209bb21360995772a54699b7f0ef6be22efa |
| Fallback | out/y2linux-production-v1-r5/fallback/BOOTIMG-storage04.img | 5193728 | 2f9d27a9d306ef5d49a1378f5063a7178edc18dec777340bf78330a23ac3a4f6 |
| Existing Y2ROOT, unchanged | out/y2linux-production-v1-r4/Y2ROOT.img | 536870912 | 814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f |
| Existing Y2DATA, unchanged | out/y2linux-production-v1-r4/Y2DATA.img | 838860800 | 01bcf65da1716081707202af074969e21fc1196540cb1de6ca76ef9098d44079 |

The sole new payload is BOOTIMG. Both ext4 images pass read-only integrity,
label/UUID and content-contract checks. Rootfs stays 2025.02.17-storage.4 and data
schema stays 1; matching module remains owned by BOOTIMG. No ext4 rebuild or edit.
This validates local flashed-source images, not a fresh on-device readback.

Stock geometry is unchanged: ANDROID/Y2ROOT starts at sector166912 with1679360
sectors; USRDATA/Y2DATA starts at2104320 with1638400 sectors. MBR/EBR are not
rewritten. BOOTIMG remains inside its16777216-byte stock allocation, including
LK's complete read tail. All 71 hardware DT nodes and reservations match Storage04
except calculated initrd-end; kernel configuration changes only LOCALVERSION.

## SPFT selection prescribed by this package

Use the already established DA and **Download Only** with
out/y2linux-production-v1-r5/MT6582_BOOTIMG_only_scatter.txt.
Select **BOOTIMG → out/y2linux-production-v1-r5/BOOTIMG.img only**.
Every other row is unchecked and has filename NONE, including PRELOADER, MBR,
EBR1, EBR2, UBOOT, ANDROID and USRDATA. No Format or Firmware Upgrade.
BOOTIMG physical address is0x1d80000; the stock scatter linear address0x3180000
remains distinct. Do not manually translate addresses or substitute another
partition table. The scatter already supplies exact stock values.

Fallback uses the same scatter and BOOTIMG-only selection with
fallback/BOOTIMG-storage04.img. It restores the observed Linux rescue/ACM state;
it is not a working internal-root fallback or a full Android restore.

## Intended physical test — internal boot failed

The requested test was one boot with external SD removed. Intended sequence:
sector-zero55aa, mmcblk0p5/p7, verified internal ext4 Y2ROOT/Y2DATA, writable
mounts, `switching to Buildroot`, then `Internal system started.` on screen.
ACM logs can observe this same platform. The existing SSH authorization is
preserved; no matching local private client key was present at entry.

[Offline evidence](evidence/y2linux-production-v1-r5/README.md) contains 52 passing
regression tests, actual ARM checks, preserved userspace receipt, kernel/artifact
validation and eight package rejection cases. Software fixtures and source
tests do not prove device contents or physical write preservation. Internal boot
acceptance failed in the subsequent owner result; storage #33 stays ACTIVE. No physical assistant write,
SPFT execution, new diagnostic firmware or unrelated subsystem work occurred.
