# Y2LINUX-M4-01 build evidence — 2026-09-14

Kernel source: `f2d2ac641ee67cd94cd2c57c9f5201b6e78841fc`.
BOOTIMG: 5355520 bytes, SHA256
`5b2bdfa784aa790a81cd4c4f495ffcb1bd2dbfd2e26ffa33c0881c67967cb883`.
Package: `out/y2linux-m4-01`. [Deployment and exact physical tests](../../y2linux-m4-01-deployment.md).

One fresh build tree was used. Configuration and compiler integration errors
were corrected with in-place resumes; `kernel-build.log` retains that history
and the final successful Image/zImage/module build and ARM observer self-test.
There is one completed integrated M4 BOOTIMG, no per-feature images, and no
Buildroot/root/data rebuild. The two independent watchdogs remain distinct.
The original Kconfig symbol/dependency and missing-header errors do not remain
in the final configuration/build. The build environment's BusyBox find warning
concerns the empty built-in initramfs scan; the external production initramfs is
independently constructed and every archive entry is checked.

The final regression log contains 56 production/artifact tests plus 10 power,
audio-clock, wheel and display tests: **66 passed, none skipped**. ARM glibc
thread/TLS, VFP, clock/allocator ABI and installed BusyBox script syntax pass.
The USB failure fixture was updated to model the production freezable queue;
its obsolete schedule_delayed_work stub was a harness compile failure, not an
unresolved kernel failure. Test/helper/documentation follow-ups change no
kernel, DT, config, BOOTIMG or installed root/data bytes.

Power tests execute the actual ADC callback with transaction faults/timeouts,
CPU PLL transition callback with ownership/voltage/readback faults, PMIC wake
callbacks with mask/parent-wake failures, and eMMC suspend callback with cache,
status and busy failures. Conversion arithmetic runs with undefined-behavior
traps and checks invalid calibration, known reference coefficients and units.
Emitted DT mutation checks reject altered OPP voltage/frequency, thermal trips
and calibration addresses. These are simulated tests, not sensor or suspend
measurements on the physical Y2.

Artifact validation confirms enabled RTC device/sysfs interfaces, cpufreq
cooling, shared CPU policy dependencies, linked power drivers, ARM ABI/module
ownership, M3 startup fix, exact reservations, Storage06 protection/addressing
and Android/MTK envelopes. Image is 10134720 bytes, kernel span 11263168,
DT 11643, compressed initramfs 1937493. LK's complete read ends at 5355520,
within the 16 MiB BOOTIMG partition with 5547 zero tail bytes. No memory expansion.

`package-rejection-check.py` rejects 12 isolated mutations: root/data contract,
protected runtime writes, coordinates, module version, second/root payload,
scatter selection, incompatible/corrupted/misidentified fallback and BOOTIMG
identity. It never modifies the original package or a physical device. The
selected fallback is the physically working Storage06 image, not Storage04:
5193728 bytes, SHA256
`3ba809e44bd9c945c994209d1d93f41fe0da9259a303487dd6ecf49724bf49a8`.

The unchanged rootfs reference is
`out/y2linux-production-v1-r4/Y2ROOT.img`, SHA256
`814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f`.
Existing Y2DATA and its corrected owner SSH key remain installed. Historical
root/data seed identities in the base manifest are build provenance only;
they are not hashes of live mounted filesystems or deployment payloads.

M4 remains ACTIVE/PARTIAL. Physical battery voltage/charger events, calibrated
die temperatures, frequency transitions, idle, screen/audio PM, same-session
suspend/Power+RTC wake, root/data integrity, reboot and actual poweroff/normal
power-on are **not tested on this candidate yet**. Charging is inhibited pending
validated pack thermometry/policy. Capacity, measured current, pack temperature
and full-charge detection are unsupported. CPU idle/suspend uses WFI/s2idle
retention; deep SPM and externally measured power savings are not claimed.

The agent did not flash, change live PM state, alter root/data images, write
calibration/NVRAM, change Git identity or start a radio/player workstream.
