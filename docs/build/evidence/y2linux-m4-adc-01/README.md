# M4 ADC production update: build and owner boot receipt

Source commit: `bbd0664dcbc1190a5e16e3b3f5889f2154cb04fd`.
Kernel: `6.18.0-y2linux-m4-adc-01`. One clean kernel build completed without
source corrections/resumes. The existing production userspace was reused;
no Buildroot rebuild, Y2ROOT/Y2DATA image rewrite or assistant flash occurred.

BOOTIMG: `out/y2linux-m4-adc-01/BOOTIMG.img`, **5355520 bytes**, SHA256
`c3c9778e396413fced69364190eb892456dd71c66a0c23fa650be2dcd30e7461`.
The package includes the working M4-01 fallback, also 5355520 bytes, SHA256
`5b2bdfa784aa790a81cd4c4f495ffcb1bd2dbfd2e26ffa33c0881c67967cb883`.
[Saved package and manual restoration](../../y2linux-m4-adc-01-deployment.md).

Configuration, actual ARM kernel/module build, DT, Storage06 addressing and
protected-write boundaries, rescue archive/module ABI, memory/layout, Android
and MTK envelopes, partition bounds and final package hashes pass. The package
validator verifies existing root/data reference identities and exact fallback
compatibility. It does not hash the subsequently mounted live filesystems.

Eight targeted checks pass, none skipped: six power tests (including emitted
production DT mutations), clock/PMIC write firewalls, and actual serialized
PWRAP callbacks. ADC failures/timeouts exercise all four channels, preserve
unrelated request bits and propagate restore errors. Additional request bits
do not admit charging enable, charger-limit or protection writes. The initial
wildcard baseline command mistakenly ran a legacy D08 memory fixture against
the production DT; its rejection was an invocation mismatch. The explicit
production test selection passes without changing validation or memory layout.
`targeted-checks.json` records the invocation/result; retained logs contain
the actual build, userspace reuse and artifact validation output.

The owner manually deployed before the packaging handoff. SSH verifies the
new release, four CPUs, 954044 KiB MemTotal, internal p5/p7 and fresh raw IIO
acquisition. A readback hash of the flashed BOOTIMG was not collected. Existing
ext4 synthetic-inode boot warnings remain; the filesystems recover cleanly
and mount normally. There is no new audio/suspend/storage-stress acceptance.

[120 physical samples and result](../../../hardware-evidence/2026-09-14-m4-adc/result.json)
show BATON at 570.575–570.630 mV with no resolved response to normal handling.
ISENSE/BATSNS voltage acquisition works, but current calibration does not.
**Charging remains inhibited and blocked at pack thermometry.** This is an
ADC prerequisite, not a completed charging implementation or M4 qualification.
The next sensor experiment can use the installed kernel; no further image is
needed merely to collect readings.
