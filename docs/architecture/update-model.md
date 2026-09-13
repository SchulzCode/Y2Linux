# Y2Linux update model — layout v1

This is an architecture contract, not an implemented OTA updater. Buildroot
2025.02.17 remains the canonical userspace builder; Linux6.18, glibc, BusyBox,
Dropbear, ALSA and current platform services work without Y2PlayerNative.
No remote downloading, signing, server, checks, automatic install or app code
is implemented here. The release package is an unsigned manual-test candidate.

## Stable update units

| Unit | Location | Version / compatibility | Replacement boundary |
| --- | --- | --- | --- |
| Kernel + DT + rescue | Stock BOOTIMG | kernel_version, kernel_contract | Whole BOOTIMG image at partition-relative zero |
| Buildroot system | ANDROID, ext4 LABEL=Y2ROOT | rootfs_version, rootfs_contract, ARM hard-float/glibc ABI | Whole root image, including exact matching kernel modules |
| Persistent state | USRDATA, ext4 LABEL=Y2DATA, mounted /data | data_schema_version | Never overwritten by normal OTA or preserving reinstall |
| Future Y2PlayerNative | Built-in /usr/bin, /usr/lib, /usr/share; optional future /data/apps/y2player/releases | Independent application version plus minimum runtime ABI | App bundle/file tree; no firmware partition |

`manifest.json` carries layout/release/component versions, build Git commit,
board/capacity/scatter identity, physical and scatter-linear starts, partition
bounds, actual sizes and SHA256 for each raw and SPFT representation. Storage02 uses
raw ext4 directly because the selected legacy DA rejects sparse FILL chunks.
The same hash describes transport bytes and physical image bytes; old failed
RAW/FILL package metadata remains historical evidence. Image offset zero means offset zero *inside that partition*.
The layout file and validators are the future shared installer/updater contract.

BOOTIMG and Y2ROOT can be independent updates only when kernel/userspace contracts
and module ABIs remain compatible. This release embeds a DRM module in both
rescue and rootfs; arbitrary kernel-only replacement can invalidate rootfs module
indexes. A future kernel-only package must carry matching modules as an ancillary
file payload or explicitly prove compatibility. Current manifest treats kernel
contract plus exact packaged module validation as constraints, not permission to
mix arbitrary kernels/root filesystems. Full releases may update both together.

Y2DATA holds settings, apps, y2player state, logs, cache, SSH public authorizations
and generated host keys. State survives root/kernel/app replacement. Root is
writable as requested; ad-hoc edits outside /data are **not** preserved by image
replacement. Use /data/settings for persistent configuration. Schema migrations
must be explicit, bounded, backed up and compatible with intended rollback.

## Future application-only update

A future Buildroot `package/y2player-native/` may install the baseline player in
/usr. An optional signed versioned payload under /data/apps/y2player/releases
could take precedence via a future launcher after verifying runtime ABI and
signature. Stage into a new directory, fsync files/directories, then atomically
switch a same-filesystem activation symlink. Keep the prior version until a
health check; never place state inside a replaceable release directory.
/data/y2player/settings, database, cache and history persist independently.
No launcher, app package, activation mechanism or updater is implemented now.

## Single-slot rescue direction and failure semantics

The stock map has one LK-selected BOOTIMG and a separate stock RECOVERY. Keep
RECOVERY, PRELOADER, LK and tables. No cheap A/B boot selection is evidenced;
no A/B partition is reserved and no new partition table is needed for the
selected **staged single-slot rescue** model. The 820 MiB root slot and 800 MiB
data slot are fixed. The current root filesystem is512MiB with ample package
space; transport compression is small. Future updater must calculate staging
space, keep persistent-data headroom and reject updates that do not fit. It may
use a host or optional SD for staging/recovery, never require SD for normal boot.
Do not promise that arbitrary future root versions and two raw backups fit in data.

Future transaction concept: verify compatibility, exact board/table, allowlisted
target, signature/hash/expanded size and staging capacity in the running system;
write a durable intent record on /data; enter rescue explicitly; verify again;
unmount root; write/flush/read back; retain an interrupted-state record until
healthy boot is acknowledged. Rescue must never mount or overwrite a root being
updated. Filesystem journal recovery alone is not image-update rollback.

A torn Y2ROOT image leaves intact BOOTIMG rescue available. A later updater can
resume a verified staged image or restore a retained compatible previous image.
A torn BOOTIMG can prevent rescue itself starting: **v1 cannot provide automatic
rollback for this case**. Owner-proven SPFT/stock loader recovery remains the
external path. Kernel-only online self-overwrite is not a supported v1 operation.
The current kernel guard permits writes only to ANDROID and USRDATA, and rejects
BOOTIMG and hardware partition switches. A future rescue writer needs a separate
reviewed, narrowly enabled BOOTIMG write policy; no repartition is needed.

Release identity and component versions are independent of layout_version. Reject
unknown layout, incompatible board/capacity/table, oversized or corrupted images,
unsupported ABI/schema and a dependency mismatch before any write. Future signed
metadata must cover these fields and anti-rollback policy. Null signature,
signing_key_id, update_channel and rollback_index are extension points, not a
claim of authenticity or verified boot. Unchanged LK does not verify this release.

## Installer contract and protected exclusions

A future Y2Linux Installer and OTA updater should share this package parser,
layout-version checks, partition allowlist, representation/expanded hash checks,
compatibility rules and readback verification. Installer detection must verify
the physical Y2 and its actual tables, not merely USB VID/PID or a filename.
Only first installation may initialize Y2DATA under explicit data-loss choice.
Preserving reinstalls select BOOTIMG and ANDROID, leaving USRDATA untouched.

Normal update allowlist is BOOTIMG and ANDROID, plus a future validated app file
namespace on Y2DATA. PRELOADER, UBOOT/LK, NVRAM, PRO_INFO, PROTECT_F/S, SECCFG,
SEC_RO, MBR/EBRs, recovery/control resources and unknown vendor regions are
excluded. These contain boot-chain expectations, recovery, calibration or
unique identity that an ordinary system update cannot reconstruct safely.
Leave unused Android CACHE/EXPDB/FAT alone; FAT may contain user media.

The first-install data template is explicitly personalized using a fresh owner
public key. Private client key stays outside the package; no Dropbear host key,
NVRAM, MAC, serial, Wi-Fi/BT identity or calibration is copied from the device.
Production USB uses gadget-generated local MACs rather than copied identities.
Password and SSH forwarding are disabled. Keys are generated from kernel entropy
on first connection and persist on /data; entropy initialization can delay SSH
(the prior physical boot took about154s). Public distribution needs explicit
per-owner provisioning; never distribute this owner's personalized data template
as a universal credential. Rescue ACM is diagnostic-only; no unauthenticated
USB network shell is added. Broader debug access/security remains #32.
