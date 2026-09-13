# Y2Linux

Reusable general-purpose Linux platform for the physical Innioasis Y2, using
Linux 6.18, Buildroot and standard Linux interfaces. Y2PlayerNative remains
**not started** until the platform milestones and whole-system qualification finish.

**Start here:** [2026-09-13 production handoff](docs/planning/session-handoff-2026-09-13.md).
[Manual install package, exact hashes and complete deployment checklist](docs/build/y2linux-production-v1-r2-deployment.md).

| Milestone | Current status |
| --- | --- |
| M1 | Complete: physical Linux boot and native PID1 |
| M2 | Core/Buildroot physically qualified for progression; #22–26 closed. USB reconnect #27 remains unresolved/deferred under #28 |
| M3 | Active / near completion: clean native S16 stereo 44.1-kHz headphones; period-notification fix deployed and tested. 48 kHz, L/R and stop/restart/repeat remain |
| Production Storage / Installation v1 | Active: internal root/data and install package; physical no-SD acceptance pending |
| M4 | Not started; follows Production Storage v1 and boundary audit |
| M5 | Not started; Wi-Fi/Bluetooth later |

Last qualified running kernel: `6.18.0-y2linux-m3audio02`, hostname `y2linux`, SSH
`root@10.42.0.1`. Writable removable SD Buildroot, four CPUs, visible display,
wheel/buttons, PMIC/core buses and initial ACM/Ethernet/SSH work. DEV-02 measured
MemTotal 954660 KiB and passed a bounded 256 MiB allocator test; AUDIO-01 measured
954384 KiB with its larger kernel. These are KiB, not 954 MiB. eMMC is disabled in that baseline.
The first production flash completed BOOTIMG transfer but failed during ANDROID;
no subsequent boot is established. The corrected Storage02 raw package is ready
for the owner's manual retry.
Initial USB/SSH returns after owner startup/restart; cable reconnect still fails.

ALSA tools and quiet WAVs are now persistent on SD. Current native format is
S16 stereo 44.1/48 kHz; only 44.1 kHz has clean physical acceptance. 32-bit I2S slots
carry 16-bit samples: native 24/32-bit and higher rates are not implemented.

- [Roadmap and standing milestone audit](docs/planning/roadmap-gap-audit.md)
- [Core physical qualification](docs/knowledge/y2linux-dev02-live-qualification.md)
- [Current audio physical result and fix](docs/knowledge/m3-audio-01-live-result.md)
- [Audio architecture](docs/knowledge/m3-audio-architecture.md)
- [Persistent audio tools](docs/knowledge/audio-tools-sd.md)
- [AUDIO-02 deployment and hashes](docs/build/y2linux-m3-audio-02-deployment.md)
- [M0 research/recovery](docs/planning/M0-evidence-and-recovery.md) and [evidence index](docs/knowledge/evidence-index.md)

Current work: [Production Storage v1](docs/architecture/production-storage-v1.md).
Build a stock-boundary internal-rootfs package with OTA-compatible component
metadata. The owner performs the first manual BOOTIMG/ANDROID/USRDATA install.
No assistant physical eMMC writes or SPFT operation. PRELOADER, LK, tables,
NVRAM/calibration and all unrelated partitions remain untouched.

Reviewed hardware logs, small manifests and findings belong in Git. Private raw
captures remain in ignored `evidence-private/`; immutable donorSource, `.cache/`
and generated `out/` trees remain ignored. Never commit private SSH keys.
A local ignored artifact copy is not an independent backup.
