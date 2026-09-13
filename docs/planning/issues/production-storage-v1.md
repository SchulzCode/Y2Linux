# Production Storage / Installation v1 — issue 33

ACTIVE, wider qualification pending. **Storage06 internal boot and owner SSH
are physically confirmed.** The former Storage03 pending-flash body is
superseded by the local Storage06 source and hardware evidence.

The stock logical disk needs a 23552-sector native offset and exposes 15203328
logical sectors. Source `bea30cd5883f524a252d8b94d7e914d9d2f7cbdb` restores
that translation for ordinary block reads/writes without changing the stock
tables or protected-region firewall. The owner manually deployed Storage06 and
then the corrected owner-key Y2DATA initialization. Authenticated SSH confirms
Linux 6.18.0-y2linux-storage06, four CPUs, 954376 KiB MemTotal, internal ext4 p5
root/p7 data, no SD block device and working USB ECM/ACM. Full BOOTIMG readback
and stock MBR/EBR hashes match. Existing owner key/identity are retained.

[Storage06 result and exact artifacts](https://github.com/SchulzCode/Y2Linux/blob/main/docs/build/y2linux-production-v1-r6-deployment.md) · [Hardware evidence](https://github.com/SchulzCode/Y2Linux/blob/main/docs/hardware-evidence/2026-09-13-storage06-owner/README.md) · [Addressing proof](https://github.com/SchulzCode/Y2Linux/blob/main/docs/knowledge/storage06-addressing-correction.md).

The owner explicitly authorized M4 progression and confirmed preserving this
Storage06 layout. Y2LINUX-M4-01 is now built from
`f2d2ac641ee67cd94cd2c57c9f5201b6e78841fc`, awaiting manual BOOTIMG-only
deployment. It reuses current root/data and includes the working Storage06
BOOTIMG fallback. It introduces normal power/thermal/cpufreq/idle/suspend/RTC/
reset APIs with bounded charging inhibition. No M4 hardware success is claimed.
[M4 deployment and qualification](https://github.com/SchulzCode/Y2Linux/blob/main/docs/build/y2linux-m4-01-deployment.md) · [M4 epic](https://github.com/SchulzCode/Y2Linux/issues/30).

- [x] Internal Y2ROOT/Y2DATA mount and Buildroot switch_root with SD absent.
- [x] Owner-key SSH, four CPUs and expected measured RAM.
- [x] Stock table hashes and complete Storage06 BOOTIMG readback.
- [ ] Bounded scratch-write/repeat/persistence and preserving reinstall qualification.
- [ ] Wider protected-region/readback, rescue-negative and recovery qualification.
- [ ] Suspend/resume root/data integrity, display/input/audio regression tests.
- [ ] Controlled reboot, actual hardware-off and subsequent normal power-on under M4.

Two synthetic ext4 buddy-cache inode warnings remain recorded; the image checks
report clean and the system boots. Do not turn that into an unperformed stress,
power-fail or full filesystem acceptance claim. M3's narrow residual checks
remain #29; USB cable reconnect #27 is deferred and does not block M4 unless it
directly prevents suspend qualification. Radios and Y2PlayerNative stay deferred.
The owner alone performs BOOTIMG flashing; root/data, loaders, partition tables,
NVRAM/calibration and unrelated partitions are preserved in this M4 update.
