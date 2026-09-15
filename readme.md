# Y2Linux

Reusable general-purpose Linux platform for the physical Innioasis Y2, using
Linux 6.18, Buildroot and standard Linux interfaces. Y2PlayerNative remains
**not started** until the platform milestones and whole-system qualification finish.

**Start here:** [September 15 session handoff](docs/planning/session-handoff-2026-09-15.md)
and [M5 connectivity entry audit](docs/knowledge/m5-connectivity-entry.md).

POWER-02 is installed and its full image-length readback matches. Fresh SSH
finds a latched charging fault; M4 acceptance remains open and there is no M5
deployment candidate. [Current physical evidence](docs/hardware-evidence/2026-09-15-m5-entry/README.md).
[POWER-02 package and fallback](docs/build/y2linux-m4-power-02-deployment.md).
[Working Storage06, owner SSH and exact artifact hashes](docs/build/y2linux-production-v1-r6-deployment.md).

| Milestone | Current status |
| --- | --- |
| M1 | Complete: physical Linux boot and native PID1 |
| M2 | Core/Buildroot physically qualified for progression; #22–26 closed. USB reconnect #27 remains unresolved/deferred under #28 |
| M3 | Active / near completion: clean native S16 stereo 44.1-kHz headphones; period-notification fix deployed and tested. 48 kHz, L/R and stop/restart/repeat remain |
| Production Storage / Installation v1 | Active: internal root/data boot without SD and existing-owner-key SSH confirmed; wider write/stress qualification remains |
| M4 | Active/partial: POWER-02 charging now stopped by voltage fault 0x8; earlier OVP and offline/deep-suspend/RTC/thermal acceptance remain open |
| M5 | Entry audited; integrated implementation/deployment blocked by M4 acceptance and unresolved own controller/calibration mapping |

Current kernel: `6.18.0-y2linux-m4-power-02`, authenticated over SSH and matched
to its BOOTIMG hash. PC charging stopped at 4.200073 V with voltage fault 0x8;
it remains inactive after the reading falls. Earlier OVP 0x10 is unresolved.
SPM entries/resumes are zero on this boot, RTC reports 2082 and thermal accuracy
is unqualified. [Current M4 evidence and limits](docs/knowledge/m4-end-user-power.md).
The USB runtime-PM workaround passed two earlier reconnects and remains installed;
fresh-boot, broader reconnect and power-consumption qualification remain open.
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

Current boundary: the owner requests the production M5 connectivity platform.
Its entry audit found material M4 acceptance failures, so no radio candidate is
ready. Own stock firmware metadata and private read-only calibration copies are
retained; silicon/record/address mapping remains unresolved. Preserve the M4
implementation and finish its gates within #30. [Production Storage v1](docs/architecture/production-storage-v1.md),
owner SSH, stock loaders/tables and protected calibration remain intact.

Reviewed hardware logs, small manifests and findings belong in Git. Private raw
captures remain in ignored `evidence-private/`; immutable donorSource, `.cache/`
and generated `out/` trees remain ignored. Never commit private SSH keys.
A local ignored artifact copy is not an independent backup.
