# Session handoff — 2026-09-13

Canonical SchulzCode/Y2Linux. Production Storage / Installation v1 is ACTIVE,
#33/milestone3, between current M3 and M4. Read the
[complete24-point deployment checklist](../build/y2linux-production-v1-deployment.md)
and [current audit](roadmap-gap-audit.md) before any operation.

Source/build commit `f2ef297a81cc0229155ba5d96f1955bdd230f11d`. Final package:
`out/y2linux-production-v1/`. All payload hashes/offsets are retained in the
checklist and docs/build/evidence/y2linux-production-v1. Kernel/rescue, internal
512MiB Y2ROOT image in stock ANDROID820MiB, Y2DATA800MiB in stock USRDATA,
SPFT sparse transports and first-install/preserve-data profiles are built.
24 tests plus memory/DT/BOOTIMG/ext4/manifest/hash checks pass. No actual updater
or native application was implemented. Read update-model.md for future design.

**Stop for owner manual readbacks/flash.** No assistant physical eMMC write or
SPFT execution. Actual native eMMC boot/write/no-SD/rescue-negative validation
has not occurred. No production milestone closure is justified. Source/docs may
be read and reviewed; do not automatically deploy or start M4.

Current physical Y2 remains6.18.0-y2linux-m3audio02, four CPUs,954384KiB,
SD-backed writable root, Y2Audio and initial SSH. eMMC remains disabled.
44.1kHz clean result stands;48kHz/LR/repeat pending. USB reconnect remains deferred.
No solved M1/M2/M3 bring-up was repeated. Development SSH key unchanged.
Production has a NEW owner-specific key in ignored out/y2linux-production-private;
its private part is not packaged. Production host keys are generated on Y2DATA.

Stock FM boot/system/userdata recovery sources were checked only for overwritten
partitions; no full-ROM re-audit. Preserve SD+AUDIO-02 for the BOOTIMG-only Linux
fallback. Owner must retain required before/after table/protected readbacks and
independent recovery access. Factory userdata is not personal Android backup.
Do not touch PRELOADER, LK, tables, NVRAM/calibration or unknown partitions.

M4 power → M5 radios → remaining GPU/USB/stability → final whole-platform
qualification → Y2PlayerNative. Keep current Git identity and authenticated
SchulzCode account. No co-author/model attribution. Images/caches/private data
stay ignored; small reviewed evidence and metadata remain tracked.
