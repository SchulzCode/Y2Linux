# Production Storage / Installation v1 partition audit

2026-09-13, baseline `1583360`. All 21 stock scatter entries reviewed before
implementation. Sources: [reconciled physical Android map](../knowledge/partition-map.tsv),
[map interpretation](../knowledge/partition-map.md), [LK/boot contract](../knowledge/boot-chain.md),
[recovery material](../knowledge/handoff-artifacts.tsv), [native MSDC identity](../knowledge/m2-baseline-hardware-result.md),
[donor/address research](../knowledge/reverse-engineering-audit.md), and local
owner-proven FM scatter. No source or ROM-wide rehash was repeated. Only the
scatter, overwritten-partition stock sources and table bytes are checked for this package.
Current Linux disables eMMC; initial SSH timed out; owner then connected Y2 and read-only SSH succeeded. Native CID/CSD evidence
and stock runtime metadata remain the latest available evidence, not live native
filesystem proof. 48 kHz remains unqualified; M3 is not closed.

All starts below are **physical bytes within EMMC_USER**, except PRELOADER
(EMMC_BOOT_1) and BMTPOOL (unknown sentinel). The full JSON also retains distinct
scatter linear values. Normal scatter linear = physical + 0x1400000. Never add
that difference to Linux disk offsets. PRELOADER's vendor 20 MiB size is NOT a
hardware boot-region extent (physical boot regions are 4 MiB each).

| Stock entry | Classification | Physical start | Stock span | Reason |
| --- | --- | --- | --- | --- |
| PRELOADER | KEEP | `0x0` | `0x1400000` | Boot ROM handoff; special hardware region span unresolved |
| MBR | KEEP | `0x0` | `0x80000` | Stock partition table |
| EBR1 | KEEP | `0x80000` | `0x80000` | Stock extended partition table |
| PRO_INFO | KEEP | `0x100000` | `0x300000` | Device/product identity; exact contents private/unknown |
| NVRAM | KEEP | `0x400000` | `0x500000` | Device-specific calibration and identity |
| PROTECT_F | KEEP | `0x900000` | `0xa00000` | Protected device data |
| PROTECT_S | KEEP | `0x1300000` | `0xa00000` | Protected device data |
| SECCFG | KEEP | `0x1d00000` | `0x20000` | Security configuration |
| UBOOT | KEEP | `0x1d20000` | `0x60000` | Stock LK bootloader |
| BOOTIMG | REUSE | `0x1d80000` | `0x1000000` | Linux kernel + DT + rescue |
| RECOVERY | KEEP | `0x2d80000` | `0x1000000` | Independent stock recovery route |
| SEC_RO | KEEP | `0x3d80000` | `0x600000` | Security/radio configuration |
| MISC | KEEP | `0x4380000` | `0x80000` | Boot/recovery control state |
| LOGO | KEEP | `0x4400000` | `0x300000` | LK display resource |
| EBR2 | KEEP | `0x4700000` | `0x80000` | Stock extended partition table |
| EXPDB | LEGACY | `0x4780000` | `0xa00000` | Legacy crash records; no reclamation without dependency review |
| ANDROID | REUSE | `0x5180000` | `0x33400000` | Y2ROOT ext4; stock /system |
| CACHE | LEGACY | `0x38580000` | `0x7e00000` | Unused Android cache; potential later staging, left untouched |
| USRDATA | REUSE | `0x40380000` | `0x32000000` | Y2DATA ext4; stock /data, destructive first initialization |
| FAT | LEGACY | `0x72380000` | `0x0` | Legacy internal shared/media storage; may contain user files, leave untouched |
| BMTPOOL | UNKNOWN | `0xffff00a8` | `0x1500000` | Vendor sentinel/unaligned special region: DO NOT TOUCH |

FAT's zero scatter length is dynamic: stock runtime resolves 0x15dc00000 bytes
from 0x72380000, ending at 0x1cff80000. Disk capacity is 0x1cff80000.
BMTPOOL has no approved operational range. EBR1's vendor allocation is 512 KiB,
but Linux's stock p1 view is only 1024 bytes; preserve both facts.

## Reuse decision and dependencies

| Component | Stock name / current purpose | Physical / linear start | Partition bytes | Linux stock view | SPFT |
| --- | --- | --- | --- | --- | --- |
| BOOTIMG | BOOTIMG, current M3 Linux boot | 0x1d80000 / 0x3180000 | 16777216 | Whole user disk range; no stock node | Independent NORMAL_ROM row |
| Y2ROOT | ANDROID, stock ext4 /system | 0x5180000 / 0x6580000 | 859832320 (820 MiB) | p5, start 166912 sectors, length 1679360 sectors | Independent YAFFS_IMG row; stock sparse ext4 semantics retained |
| Y2DATA | USRDATA, stock ext4 /data | 0x40380000 / 0x41780000 | 838860800 (800 MiB) | p7, start 2104320 sectors, length 1638400 sectors | Independent YAFFS_IMG row; stock sparse ext4 semantics retained |

LK reads BOOTIMG's legacy Android/MediaTek envelopes and transfers its ramdisk;
it does not need Android /system or /data to run Linux. Preloader loads stock LK
and retains the stock partition contract. No table, LK, preloader, recovery or
security mutation is required. Stock recovery still expects Android filesystems:
never use its wipe/update menus on Y2Linux. Restoring Android requires the verified
FM boot/system/userdata images (factory state, not original personal Android data).
Original installed Android data is not backed up; take a private USRDATA readback
before first install if its personal state must be recoverable. Preserve FAT/media.

Native Linux must check host11230000, nonremovable MMC type, disk capacity and
partition start/size before accepting LABEL=Y2ROOT/Y2DATA. Labels alone are not
write authorization. No fixed mmcblk number or vendor /emmc alias is assumed.
Both native filesystem mounts are **implemented candidate behavior, not yet
physically qualified**. Resolver failure leaves ACM rescue active.

CACHE (126 MiB) cannot hold the current 512 MiB root image. FAT is large but dynamic
and may hold user media; it is unnecessary for v1. ANDROID is sufficient and
preserves the stock tables. USRDATA provides independent persistent storage now.
A/B BOOTIMG is not cheaply available without taking RECOVERY or changing LK's
selection contract. Prefer staged single-slot updates from rescue; reserve no new
partitions. Y2DATA can stage compact current system/app payloads; future updater
must check free space and expanded size, and permit host-assisted recovery if a
larger image cannot be staged. No promise of automatic rollback after torn BOOTIMG.

## Native write policy review

Production MSDC remains at legacy one-bit/13MHz with inherited LK pinmux and rails.
The v6.18 MMC core needs EXT_CSD WRITE_BYTE using command-set1 (see locked
`include/linux/mmc/mmc.h` and `drivers/mmc/core/mmc_ops.c`), including volatile
ERASE_GROUP_DEF initialization that the historical identity-only kernel rejected.
The new profile permits only explicitly reviewed volatile EXT_CSD controls and
read/identification commands; it refuses hardware partition selection, boot
configuration, erase/trim/sanitize, RPMB/vendor operations and firmware writes.
Only sector-addressed cards of the observed capacity in EMMC_USER may issue data
writes, and the complete request must fit ANDROID or USRDATA. CMD23 is disabled
for this profile. Rejected requests finalize through HSQ when queued so a denied
write cannot strand the queue. All other stock partitions remain outside the
kernel write allowlist, including BOOTIMG; future rescue OTA must deliberately
extend the guard for that one component. This is accident containment, not a
security boundary against an attacker who can replace the kernel.
