# Y2Linux

Reusable general-purpose Linux platform for the physical Innioasis Y2, using
Linux 6.18, Buildroot and standard Linux interfaces. Y2PlayerNative remains
**not started** until the platform milestones and whole-system qualification finish.

**Start here:** [September 18 GPU handoff](docs/planning/session-handoff-2026-09-18.md).

M4 POWER-03 is owner-accepted. CONNECTIVITY-10 is installed: native Wi-Fi scanning,
radio restart and BlueZ power-on alongside Wi-Fi work with zero errors/recoveries.
[Physical evidence](docs/hardware-evidence/2026-09-17-m5-connectivity10/README.md).
Remaining M5 connection/audio qualification is pending by owner choice. Production
GPU implementation is now authorized using Linux 6.18, Lima, Mesa and existing KMS.
Reborn and M6/OTA remain out of scope. Only the owner deploys physical images.

| Milestone | Current status |
| --- | --- |
| M1 | Complete: physical Linux boot and native PID1 |
| M2 | Core/Buildroot physically qualified for progression; #22–26 closed. USB reconnect #27 remains unresolved/deferred under #28 |
| M3 | Active / near completion: clean native S16 stereo 44.1-kHz headphones; period-notification fix deployed and tested. 48 kHz, L/R and stop/restart/repeat remain |
| Production Storage / Installation v1 | Active: internal root/data boot without SD and existing-owner-key SSH confirmed; wider write/stress qualification remains |
| M4 | POWER-03 accepted by the owner; retained as the M5 power-platform baseline |
| M5 | Native Wi-Fi scans/restart and concurrent BlueZ power verified; connection/audio/reconnect/coexistence qualification pending |
| GPU | Active: production Lima/Mesa implementation; first integrated candidate pending |

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

Current boundary: implement and validate one Y2LINUX-GPU-01 candidate while
preserving M4, CONNECTIVITY-10, Y2DATA and all RAM reservations. Stop for owner
manual deployment, then perform the coherent GPU qualification. The
[Production Storage v1](docs/architecture/production-storage-v1.md) component
boundaries remain unchanged. Do not start Reborn or M6/OTA.

Reviewed hardware logs, small manifests and findings belong in Git. Private raw
captures remain in ignored `evidence-private/`; immutable donorSource, `.cache/`
and generated `out/` trees remain ignored. Never commit private SSH keys.
A local ignored artifact copy is not an independent backup.
