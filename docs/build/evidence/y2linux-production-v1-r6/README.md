# Storage06 production candidate evidence

Source: `bea30cd5883f524a252d8b94d7e914d9d2f7cbdb`. One kernel build restores
the actual stock eMMC logical disk window. Root/data images were retained.
BOOTIMG SHA256 `3ba809e44bd9c945c994209d1d93f41fe0da9259a303487dd6ecf49724bf49a8`.

54 production tests pass, followed by actual ARM shell syntax and ABI checks.
The retained regression log is the completion portion (the first21 test starts
were returned directly in the tool transcript); its final54-test summary and
ARM results are intact. Parser/MMIO/filesystem/mount inputs are simulated in
unit tests; this alone is not hardware qualification.

Artifact validation verifies kernel/module/DT/rescue, memory bounds and
Android/MTK envelopes. Complete LK read ends at5193728 bytes within the16MiB
BOOTIMG allocation, with5546 zero tail bytes. All71 DT nodes and reservations
match Storage04 except calculated initrd-end; config differs only in release.
13 isolated package mutations are refused, including wrong native offset,
capacity/tail exposure, wrong DA coordinates, protected writes/selections,
root/data identity and module/boot corruption. Original candidate is preserved.
Both retained ext4 images pass host e2fsck, identity/hash and reuse checks.

The owner flashed before the final response. [Actual ACM evidence](../../../hardware-evidence/2026-09-13-storage06-owner/README.md)
confirms55aa, partition discovery, internal ext4rw root/data and Buildroot
handover. SSH service is reachable but the original seed has the wrong owner
key. No new kernel is needed for the authorized Y2DATA template correction.
The frozen BOOTIMG package remains unchanged; physical findings supersede its
pending-acceptance text. No repeat/stress/power-fail qualification is claimed.
