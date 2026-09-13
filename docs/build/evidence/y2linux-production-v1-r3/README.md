# Storage03 offline validation

Build commit: 48458605db5f53bd188921d53359e470f1e8bf05.
Package: out/y2linux-production-v1-r3, release 0.1.0-storage.3.

33 regression tests pass, including extracted real MMC request guard, PHY
saved-current/cable-state/fault tests, unchanged platform/audio/ABI/BOOTIMG
contracts. A separate linked-kernel USB test passes. ARM ABI and ALSA utilities,
rescue shell syntax, cached-status valid/short-record decoding and actual ARM
blkid against both final images pass. USB FAILED/-19 in arm-package-check.log
is a deliberately synthetic decoder fixture, not a new device observation.

Final package validator passes ext4 e2fsck read-only checks, label/UUID, contents,
owner-key isolation, module ABI, layout/scatter arithmetic, all image bounds,
stock restoration source hashes and complete SHA256 inventory. Kernel/DT/newc/
BOOTIMG and D08 memory checks pass in the build's artifact-validation.log.
The initial build was followed by an incremental build after fixing a stale
status-display race and a misleading validation metadata label. Final bytes
and versions come from the commit above.

The owner photographs are retained privately; their hashes are in
owner-photo-hashes.json. They establish failed Storage02/DIAG01 boot, not a
Storage03 success. No physical readback, normal internal-root boot, USB recovery,
protected-data preservation or off-state power fix is established. Storage
remains ACTIVE and stops for the owner's manual flash. No assistant physical
internal write or SPFT execution occurred.
