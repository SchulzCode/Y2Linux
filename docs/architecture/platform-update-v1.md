# Signed root-only update contract v1

Software implementation; **PHYSICAL_GATE** for this integrated candidate. Do not
use on Y2 until the owner installs the exact candidate and runs session G. The
initial rescue BOOTIMG must be installed through the existing owner-controlled
manual flow. Automatic BOOTIMG replacement is deliberately absent: this layout
has one rescue-containing slot, so a torn write can remove recovery. No A/B,
partition table, protected partition, NVRAM or calibration write is introduced.

## Authentication and policy

`tools/update/package.py` creates an exact-byte UTF-8 JSON manifest and a 64-byte
`manifest.sig`, the standard libsodium Ed25519 detached signature of **all bytes**
of `manifest.json`. The package also contains `rootfs.ext4.gz`. The native
`y2-update-core` is the single verification/policy authority in both Linux and
rescue. It uses pinned Buildroot libsodium 1.0.22 (ISC), json-c 0.18 (MIT) and
zlib. No Python is added to rescue. Existing 8 MiB regular/4 MiB compressed
rescue budgets remain mandatory.

The manifest binds product, board revision, release version/monotonic sequence,
minimum compatible sequence, Linux/Reborn commits, kernel, rootfs version,
root/data/rescue/library schemas, one allowed Y2ROOT payload, compressed/raw
sizes and SHA256, key ID, signature format, and explicit rollback/downgrade flags.
Current Reborn database schema is **1**, as its current source declares. Root-only
OTA requires the exact running rescue kernel; new kernel/DT/modules require a
separate owner-controlled paired/manual release. No active root write exists.

The trusted registry supports up to 16 distinct active/revoked keys with IDs,
public keys, development/release purpose, sequence bounds and downgrade policy.
Release replay/downgrade is refused against the durable accepted high-water
sequence. Development downgrade needs BOTH a development-key permission and a
signed manifest permission. A separate current sequence tracks explicit restore;
restoring a previous root never lowers the accepted high-water sequence. This
is disk-backed policy, not a tamperproof hardware rollback counter or secure boot.
An owner with root or a physical recovery tool remains trusted.

The candidate embeds only public key `platform-v1-development-20260923`. Its
private key is in the owner's private host directory
`~/.local/share/y2linux/signing/platform-v1-development.key`, mode 0600 inside
0700 directories, outside both repositories and all build artifacts. It is not
read/copied into build inputs or Y2. A maintainer uses the offline keygen/package
CLI; private-key output is never printed. Production release keys are a separate
owner provisioning decision, not this development key renamed.

Rotation/revocation is an immutable-rescue boundary: preprovision multiple public
keys, then switch the signing key. Add/revoke keys in a reviewed BOOTIMG via the
manual recovery flow before depending on that new trust state. Root-only updates
cannot revoke a key that an old BOOTIMG still trusts. After compromise, stop the
channel, preserve evidence, provision a new owner rescue/key registry, and then
install a verified root. No unsafe one-key-forever assumption or unauthenticated
network key refresh exists. Exact key registry/hash is part of candidate evidence.

## Check, stage and apply

The owner may set `/data/updates/channel.json` to
`{"manifest_url":"https://owner.example/release/manifest.json"}`. No public
channel is guessed or contacted automatically. Commands:

```
y2-platform update check --url https://owner.example/release/manifest.json
y2-platform update stage --url https://owner.example/release/manifest.json
# An owner-delivered directory is also supported, without a wall-clock dependency:
y2-platform update stage --package /data/owner-signed-package
y2-platform update status
y2-platform update apply
```

Network requests use standard Python/OpenSSL HTTPS verification, established
platform clock/CSPRNG readiness, no proxies, credentials-in-URL, query tricks or
redirects, bounded manifest/signature/payload sizes, socket timeouts and total
download deadlines. A manifest is authenticated before its payload is downloaded.
A private fresh copy is fsynced, decompressed/hashed/identity-checked by the native
core, published as pending, then durably queued. Incomplete/invalid input cannot
queue. Staging cleanup only touches three known disposable filenames in bounded
private update directories. It never deletes music/database/session data.

Space admission preserves 96 MiB and 128 inodes. The live filesystem is mutable,
so the exact previous image cannot be safely captured until rescue. Rescue
compresses its **actual ext4 extent**, checks reserve on every output block, and
verifies the complete backup by decompression/hash before any root write. If
backup space is insufficient, update fails with the old root untouched. The
worst-case backup is the old raw extent plus gzip overhead; owners should reserve
that or accept safe admission failure. No optimistic backup-space promise is made.

`apply` uses the platform bounded application shutdown/reboot contract. `cancel`
only cancels a queued, not-yet-installed update. Normal userspace has no block
writer interface. A local stage request is explicit owner maintenance, not an
untrusted app's download authority.

## Rescue and rollback

Boot validates internal Y2DATA controller/geometry/UUID/label/schema, performs
bounded-policy e2fsck, and consults the durable update journal before demanding
an intact root superblock. Only the native offline writer may resolve a torn
root by exact internal MMC controller, nonremovable type, capacity, partition
start/size, unmounted major/minor and exclusive block open. Device-node capacity
is independently checked. There is no caller-supplied target device.

External source presence and kernel CSPRNG readiness are required. Source presence
is not proof of safe charging or energy reserve; those remain physical gates.
Normal boot without a pending update does not depend on update crypto entropy.
The standard persistent per-device seedrng may run in rescue; no weak fallback.

States: `Idle → Queued → BackingUp → Installing → PendingHealth → Acknowledged`.
All durable transitions use fsync + rename + directory fsync. Backup SHA256 and
extent are in the journal, and the queued exact manifest hash is rebound in
rescue. The installer validates a single gzip member, raw/compressed lengths and
hashes and Y2ROOT ext4 UUID/label/extent. It writes only that root partition,
fsyncs, reads back and hashes the actual bytes, then permits ordinary root
preflight/fsck/handover. Raw writes are not claimed atomic.

An interrupted `Installing`, failed readback, or next boot without health ack
restores the verified previous image and verifies readback. A failed/corrupt
backup enters `RescueRequired`; there is no automatic retry/reboot loop. RAM
rescue remains present. An owner in rescue can explicitly retry a repaired
verified backup using `/sbin/y2-update-core rescue-restore` with validated data
mounted at `/newdata`; the same geometry, external-power and hash checks apply.
Never replace the journal/hash to force an unverified image through this path.

The normal platform health worker has 180 seconds from its first observation of
`PendingHealth`. It requires the current Reborn boot/PID/start-time generation,
an actually presented first KMS frame, a responsive application after ten seconds,
and healthy root/data/filesystem/display/render/audio/input checks. It then calls
the native exact-source/boot acknowledgement. Timeout queues rollback for the
**next owner reboot**; it does not start a reboot loop. A kernel/app hang is not
automatically recovered by an unqualified AP watchdog. An earlier owner reboot
also rolls back an unacknowledged image. Optional radios need not be enabled.

A signed `rollback_allowed` policy permits `y2-platform update rollback` after
acknowledgement. Only the most recent complete previous-root backup is retained;
starting another update replaces it after safely capturing the current root.
Persistent user/database data is preserved, so incompatible destructive schema
migrations are refused rather than promised reversible by root restoration.

## Evidence and limitations

Host fault tests execute the real C verifier/gzip/writer and actual Ed25519 with
temporary regular-file root images: invalid signature/device/schema/kernel,
truncated/corrupt payload, wrong key/revocation, low backup space, interrupted
root write, failed readback, absent/stale health, corrupted backup, bad journal,
symlinks and replay. Staging tests exercise the real authentication core before
accepting payloads. They are process/file failure evidence, not electrical
power-loss durability. The production build must reject fixture/fault strings in
its rescue executable. ARM/QEMU and exact image results are recorded separately
in the completion ledger; physical root write/restore remains owner-controlled.

Reconstruction: keygen takes an external mode-0700 directory; package takes
`--image`, `--versions`, `--output`, `--private`, `--key-id`, `--sequence`, optional
minimum sequence/development downgrade/rollback flags. Root images and manifest
fields must come from a validated current-source release. The release builder
never needs the private key. The maintained manual `system_update.py` remains the
owner preserving BOOTIMG+root packaging tool, distinct from this signed OTA unit.

Crypto reference: [libsodium detached signatures](https://doc.libsodium.org/public-key_cryptography/public-key_signatures).
