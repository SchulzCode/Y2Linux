# Y2 Hardware Final qualification

ACTIVE,2026-09-27. One final preserving candidate and one owner-controlled
installation/qualification cycle. No automatic root OTA apply, flash, loader
write, Y2DATA replacement or full discharge is authorized by a test checklist.

The installed Hardware02 baseline and exact boot/source pair are in
[the campaign](Y2-HARDWARE-FINAL.md). Raw receipts stay private. Every test records
its actual source pair, clock/codec/format, load, duration, boot identity, before/
after errors/temperature and result. Failed or unperformed cases are not passes.

## Current-device execution

1. Identity/root/data/SD/taint/health, MMC CID/CSD/EXT_CSD/IOS/error counters,
   DMA activity, per-core clockevents/highres/NO_HZ and full sensors/services.
2. Bounded scratch13/25/50MHz sequential/random/readback and metadata/fsync/
   fdatasync/directory fsync/SQLite. Retain lower cap on any error. Repeat final
   setting and matched1k/10k/20k library/scan/startup/memory.
3. RAM TCP both directions, eMMC/SD/RAM SFTP, USB IRQ/DMA/CPU/thermal/retransmits;
   Wi-Fi baseline and SBC/AAC coexistence, observation failures recorded.
4. Local timer continuity across32-bit low-word rollover, sub-tick sleeps,
   per-core hotplug and supported-frequency stress; restore original policies.
5. Product SBC/AAC, real headset AVRCP/listening, live volume regression,
   wired44.1/48 switching/screen-off/stop/resume; temporary userspace restored.
6. One persistent pm_test stage at a time, explicit host and radio/storage/
   display recovery before next. Then actual Power wake with same boot and RTC
   alarm. An affected-stage failure stops progression, independent work continues.
7. Meter/pack-dependent cases stay blocked without physical evidence. No battery
   current/temperature/SOC inference from settings or die sensors; no reserve guess.

## Combined source/image gate

Accumulate fixes, audit scope, commit by capability without pushing. Run current
kernel/DT/platform/Reborn tests, fmt/strict clippy, ARM, Buildroot, QEMU/installed
ARM, FFmpeg/ALSA, package/ELF/source/license inventories, preserve-data and fallback
validation. Record all exact build identities and failed attempts. One package:
`out/y2linux-hardware-final-candidate/`, only BOOTIMG+Y2ROOT and exact Hardware02
fallback. Nothing is image-validated until that package actually passes.

## Owner installation and final physical boundary

Owner performs the established Download Only preserving install using reviewed
package instructions/checksums. Verify fresh exact identity, recovery/root/data/
SD, timers/DMA, radios/audio/UI before opt-in high OPPs or power experiments.
New guarded voltage paths require all transitions, error/thermal/integrity checks
and known-safe fallback; defaults remain baseline until measured. No intermediate
candidate is warranted merely to test an independently completed source feature.

After short tests pass:8h wired,8h Bluetooth and declared Wi-Fi/playback/scan/USB/
SD/screen-off combinations with continuous bounded telemetry. Qualify supported
peer codec quality modes and coexistence; never infer unsupported peer codecs.
Root OTA signing/staging/rescue/readback/health/rollback only after explicit owner
approval of the concrete apply operation. Recovery failures do not authorize
preloader/LK/protected-region corruption. Report exact durations/failures and all
remaining capability statuses. Full power-off/retention/reboot cycles need owner
Power/cable actions; do not silently treat reboot as full power loss.
