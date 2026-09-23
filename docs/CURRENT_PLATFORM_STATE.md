# Current Y2Linux platform state

**Y2Linux Platform v1 Candidate is ready for owner-controlled physical
qualification.** Core software contracts are implemented, host tested, ARM built
and image validated at Linux `d04b95a` / Reborn `6c8aa12`. Closing repository
commits contain documentation only; exact built identities remain in the package.
No Y2 was contacted or flashed. This candidate is not physically or endurance
qualified. Artifacts: `out/y2linux-platform-v1-candidate/` (no Y2DATA replacement).

The [completion ledger](validation/PLATFORM-V1-COMPLETION.md),
[60-capability status report](validation/PLATFORM-V1-CAPABILITY-REPORT.md),
[owner sessions A–G](validation/PLATFORM-V1-OWNER-QUALIFICATION.md),
[API contract](architecture/platform-api-v1.md),
[source reconstruction](build/platform-v1-reconstruction.md) and
[roadmap](planning/platform-v1-roadmap.md) are the active state. The original
[owner entry review](CURRENT_PLATFORM_STATE.entry-2026-09-23.md) is preserved as
history; its missing-software statements are superseded where the ledger records
implementation. Reborn's later correctness closure remains respected.

Implemented software now includes versioned telemetry/health/readiness and
capabilities; boot/crash diagnostics; SD identity/lifecycle and exFAT; storage
reserves, safe scratch/SQLite/library/resource benchmarks; independent bounded
shutdown and a disabled configurable low-battery mechanism; Wi-Fi DHCP/DNS and
single-owner reconnect, standard time/entropy policy; Bluetooth observation,
bounded reconnect, SBC, AVRCP and gated Auto; USB-only authenticated SFTP with
reserve/hash-verified publication; signed root-only OTA, rescue backup/readback/
restore, health acknowledgement, scoped reset and state export; and passive
endurance/qualification tools. SQLite/FFmpeg/BlueZ/BlueALSA/Linux driver ownership
and the existing stock loaders, memory map and root/data layout are preserved.

Explicit unavailable/gated capabilities: measured battery current/SOC/pack
thermal sensor; final low-battery thresholds and current charger envelope; deeper
cpuidle and qualified deep suspend; AP watchdog recovery/retained panic cause;
USB host/OTG/VBUS/UAC host; optional AAC/aptX/aptX HD/LDAC encoders; true S24/S32
or preserved 24-bit internal output; 88.2/96 kHz; automatic BOOTIMG OTA; production
signing/revocation deployment; byte-identical reproducibility. No production Auto
codec is eligible until matching platform qualification is deliberately recorded.
See [hardware evidence gates](knowledge/platform-v1-hardware-gates.md).

Prior physical evidence remains narrow: Storage06 internal identities/boot/USB
SSH, GPU-01 Lima/UI/input/S16 stereo 44.1 kHz and runtime PM, CONNECTIVITY-10 radio
adapters/scans, and two earlier USB workaround reconnects. GPU-01 same-boot deep
resume failed; later source corrections are not a physical pass. No prior image
receipt qualifies this integrated candidate. Existing #16/#27/#28/#29/#31/#32/
#33/#34 remain open, with #30's earlier narrow acceptance preserved.

After the owner qualifies the advertised core on the exact candidate, Y2Linux
can move to maintenance and application work can return to Y2Reborn. Optional
hardware expansion is a separately audited milestone, not a reason to keep
changing the platform underneath the application.
