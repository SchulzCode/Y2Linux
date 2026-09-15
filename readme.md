# Y2Linux

Reusable general-purpose Linux platform for the physical Innioasis Y2, using
Linux 6.18, Buildroot and standard Linux interfaces. Y2PlayerNative remains
**not started** until the platform milestones and whole-system qualification finish.

**Start here:** [September 16 session handoff](docs/planning/session-handoff-2026-09-16.md)
and [current M5 image and validation](docs/build/evidence/y2linux-m5-connectivity-02/README.md).

M4 POWER-03 is owner-accepted. CONNECTIVITY-02 (`dd84462`) is now running after
manual installation. Its MD calibration advances through 53 filesystem exchanges,
then firmware signals an exception; Wi-Fi and BlueZ are not yet usable. The early
regulatory-data loading error is fixed. Internal root/data, USB SSH and PC charging
remain available in the short inspection. M5 remains open.

[Physical result](docs/hardware-evidence/2026-09-16-m5-connectivity02/README.md)
and [CONNECTIVITY-02 artifacts and validation](docs/build/evidence/y2linux-m5-connectivity-02/README.md).
The corrected BOOTIMG passed 83 production/M4/connectivity tests and 14 package
rejection cases. Installed CONNECTIVITY-01 Y2ROOT and user Y2DATA are preserved.
The completed entry audit at `67cbe8f` remains the starting evidence.

| Milestone | Current status |
| --- | --- |
| M1 | Complete: physical Linux boot and native PID1 |
| M2 | Core/Buildroot physically qualified for progression; #22–26 closed. USB reconnect #27 remains unresolved/deferred under #28 |
| M3 | Active / near completion: clean native S16 stereo 44.1-kHz headphones; period-notification fix deployed and tested. 48 kHz, L/R and stop/restart/repeat remain |
| Production Storage / Installation v1 | Active: internal root/data boot without SD and existing-owner-key SSH confirmed; wider write/stress qualification remains |
| M4 | POWER-03 accepted by the owner; retained as the M5 power-platform baseline |
| M5 | CONNECTIVITY-02 installed; calibration reaches 53 FS exchanges then MD firmware exception; Wi-Fi/Bluetooth/A2DP/coexistence unqualified |

Historical POWER-02 charging, RTC, thermal and suspend failures are retained in
[the M4 evidence](docs/knowledge/m4-end-user-power.md). POWER-03 corrects those
observed source failures and is now accepted by the owner. M5 preserves its
production architecture and must repeat the relevant regression checks after
connectivity deployment.
The retained Storage06 correction
restores the stock eMMC logical disk window. MBR/EBRs and the complete BOOTIMG
hash match read-only Linux reads; internal ext4 Y2ROOT/Y2DATA and owner-key SSH
at `root@10.42.0.1` work with no SD block device. The data seed now authorizes
`~/.ssh/y2linux_ed25519.pub`; BOOTIMG/Y2ROOT updates preserve this persistent key.

Historical SD-backed qualification: `6.18.0-y2linux-m3audio02`, hostname `y2linux`, SSH
`root@10.42.0.1`. Writable removable SD Buildroot, four CPUs, visible display,
wheel/buttons, PMIC/core buses and initial ACM/Ethernet/SSH work. DEV-02 measured
MemTotal 954660 KiB and passed a bounded 256 MiB allocator test; AUDIO-01 measured
954384 KiB with its larger kernel. These are KiB, not 954 MiB. eMMC is disabled in that baseline.
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

Current boundary: CONNECTIVITY-02 is installed and inspected. Target the MD firmware
exception, preserving Y2DATA and protected factory partitions. After initialization
works, qualify networking, Bluetooth audio, coexistence and power behavior. Do not start GPU/lima,
Y2PlayerNative or an OTA updater. [Production Storage v1](docs/architecture/production-storage-v1.md)
and the existing M4 power architecture remain the foundation.

Reviewed hardware logs, small manifests and findings belong in Git. Private raw
captures remain in ignored `evidence-private/`; immutable donorSource, `.cache/`
and generated `out/` trees remain ignored. Never commit private SSH keys.
A local ignored artifact copy is not an independent backup.
