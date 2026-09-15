# Y2Linux

Reusable general-purpose Linux platform for the physical Innioasis Y2, using
Linux 6.18, Buildroot and standard Linux interfaces. Y2PlayerNative remains
**not started** until the platform milestones and whole-system qualification finish.

**Start here:** [September 16 session handoff](docs/planning/session-handoff-2026-09-16.md)
and [M5 candidate, hashes and manual installation](docs/build/y2linux-m5-connectivity-01-deployment.md).

The owner accepts M4 POWER-03 and authorizes M5 implementation. The current
read-only SSH baseline reports `6.18.0-y2linux-m4-power-03`. This acceptance
does not manufacture a new agent-run M4 measurement series. The completed
M5 entry audit at `67cbe8f` is reused; its earlier blocking status is historical.

Native CONSYS, AHB Wi-Fi, BTIF/STP/HCI, own-data factory/calibration handling
and Buildroot connectivity services are built in CONNECTIVITY-01 from `022c701`.
The candidate passes 81 production/M4/connectivity tests, eight isolated ARM
userspace checks and 16 package rejection cases. M5 remains open for manual
deployment and physical qualification. The
[accepted POWER-03 package](docs/build/y2linux-m4-power-03-deployment.md)
is the kernel fallback; existing Y2DATA is preserved.

| Milestone | Current status |
| --- | --- |
| M1 | Complete: physical Linux boot and native PID1 |
| M2 | Core/Buildroot physically qualified for progression; #22–26 closed. USB reconnect #27 remains unresolved/deferred under #28 |
| M3 | Active / near completion: clean native S16 stereo 44.1-kHz headphones; period-notification fix deployed and tested. 48 kHz, L/R and stop/restart/repeat remain |
| Production Storage / Installation v1 | Active: internal root/data boot without SD and existing-owner-key SSH confirmed; wider write/stress qualification remains |
| M4 | POWER-03 accepted by the owner; retained as the M5 power-platform baseline |
| M5 | CONNECTIVITY-01 built and host-validated; awaiting owner BOOTIMG/Y2ROOT installation and physical Wi-Fi/Bluetooth/A2DP/coexistence qualification |

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

Current boundary: CONNECTIVITY-01 is ready; stop for the owner's manual
BOOTIMG + Y2ROOT installation. Preserve Y2DATA and protected factory
partitions. After deployment, qualify native networking, Bluetooth audio,
coexistence and power behavior in one coherent session. Do not start GPU/lima,
Y2PlayerNative or an OTA updater. [Production Storage v1](docs/architecture/production-storage-v1.md)
and the existing M4 power architecture remain the foundation.

Reviewed hardware logs, small manifests and findings belong in Git. Private raw
captures remain in ignored `evidence-private/`; immutable donorSource, `.cache/`
and generated `out/` trees remain ignored. Never commit private SSH keys.
A local ignored artifact copy is not an independent backup.
