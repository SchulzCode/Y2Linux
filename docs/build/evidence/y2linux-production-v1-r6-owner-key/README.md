# Production owner-key data initialization evidence

The package at `out/y2linux-production-v1-r6-owner-key` was produced from clean
source `dcb7f45fd7fbe0adffc10195239f8df934491c8b`. It copies the unused Storage04
data seed, changes its authorized public key, and emits a USRDATA-only profile.
It contains no BOOTIMG/Y2ROOT payload and no private key. Original images remain
unchanged. This uses the working Storage06 platform without another kernel build.

The final Y2DATA image is 838860800 bytes, SHA256
`9e0f6b063b2ccfe7bb97bd229f0140cd47aebd917ed636b36e7fa6740ab22fe1`.
Its authorization exactly matches the existing `~/.ssh/y2linux_ed25519.pub`,
public-file SHA256 `bf9cc937fb5dcee8569514b674fb9b3d5ba25396162efac7e8854f88bfe98c25`.
The public line stays in the local package; these records retain only its hash.

The retained manifest/scatter/SHA256SUMS identify the complete local package;
the binary images and referenced metadata are not duplicated in Git. The host
package log records exact key contents/permissions, unused seed inventory,
clean e2fsck, ext4 label/UUID, stock bounds and original-image preservation.
The ARM blkid log uses the actual production target binary through QEMU in the
isolated builder and confirms both the unchanged root and corrected data images.

The acceptance script runs from the repository root. Its log records installed
root symlinks/service agreement and eleven refusals on disposable host copies:
wrong owner key, shifted data geometry, wrong UUID, NVRAM selection, data in
preserving updates, false preservation claim, extra boot payload, wrong native
offset, wrong image hash, unsafe authorization permissions and unexpected host
state. Generic checksums are refreshed so semantic checks must reject them.
All pass; the final candidate is unchanged. Five focused source tests also
passed before packaging; this does not repeat the unchanged 54-test firmware run.

The owner subsequently flashed the data image and [SSH succeeds on hardware](../../../hardware-evidence/2026-09-13-storage06-owner/ssh-result.json).
The frozen package's pending-acceptance text records its original build boundary;
the physical result supersedes that status. No assistant device writes occurred.
