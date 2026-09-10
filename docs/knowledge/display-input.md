# Display and input evidence

> 2026-09-10: [historical experiment audit](reverse-engineering-audit.md) finds a
> dated correction to 480×360 and cold-display PHY/GPIO112 reset failures.
> v6.18 already avoids the old OVL pitch/address and PMIC single-key bugs.
> Preserve our live-LK display; validate remaining variant/pin/rail details and
> the pending M2-INPUT-01 mappings on our older board.

> 2026-09-09: [donor audit](donor-audit.md) supplies concrete GPIO navigation,
> APT32F protocol, keypad, DRM and GC9503V starting points. These supersede the
> discovery-only next steps below. The first implementation uses donor GPIO
> layout/mapping with upstream polled keys and evdev; older-board wiring and
> events still need physical USB capture. Keep our proven framebuffer address;
> donor comments/timings/address and backlight ownership need correction.

> Current hardware result: [owner-observed M1 runtime success](m1-runtime-hardware-result.md) confirms Linux 6.18, native PID1, increasing BEAT/uptime, working sleep, proc/sysfs mounts, CPU0, D08 RAM visibility, framebuffer diagnostics and stopped-watchdog status. **M1 core achieved.** Exact flashed hash and unreported diagnostic fields remain unverified. Earlier dated statements below are historical.

> Y2B-240 hardware result: the owner reports **solid green**, which the implemented state machine reaches through initramfs `/init` running as PID1. M1's Linux 6.18 plus PID1 boot objective is achieved. No heartbeat/checkerboard success is claimed. [Y2B-245 / D14](on-screen-diagnostics.md) is the offline follow-up for readable runtime diagnostics.

> Y2E-150 resolves the limited LK diagnostic contract: normal final layer 2 at 0xbfb00000, 480x360 RGB565, 960-byte stride; DSI sync-pulse video and normal backlight persistence supported by exact FM/LK instructions. Android runtime metadata differs (32 bpp, stride 1920). [D13](risk-accepted-diagnostic.md) provides guarded access, provenance and limits; no panel initialization/input driver was implemented.

Date: 2026-09-08. Related task: Y2E-101. Historical source: integrity-verified snapshot `2026-07-29_004935/hardware/input-devices.txt`.

CONFIRMED in that archive: `mtk-kpd`, `ACCDET`, `mtk-tpd`, `mtk-tpd-kpd` and AVRCP input devices, including event handlers and capability bitmaps. The application documents a 480 × 360 landscape display and wheel/button navigation. That behavioral resolution is not a panel/controller/timing identification.

Android `KEYCODE_*` mappings are not Linux evdev scan codes. The old input-map note denying wheel acceleration is superseded by newer application source/release behavior; retain ordinary detent, hold/repeat, screen-off and Bluetooth-input distinctions as behavioral reference only.

UNKNOWN: panel part/interface, timings, backlight controller, touch/controller identity, orientation transformation, physical wheel/button wiring, IRQ/reset/power sequencing and wake sources. APT32F/UPDATE I2C names are discovery clues, not established functions.

Next proof is a bounded passive current-device capability/driver inventory matched to stock kernel symbols and display configuration. No synthetic input, panel writes, unreviewed controller sysfs reads, disassembly of an entire subsystem or Linux driver implementation is included in the first M0 batch.

## Y2E-130 memory evidence

The retained stock iomem identifies framebuffer `[0xbfb00000,0xc0000000)` (5 MiB). FM display-size functions were remapped at `0xc05128d4`/`0xc051296c`; the unknown-panel fallback is actually 20 MiB despite its misleading log. LK also forms a scratch pointer framebuffer minus 4 MiB. Buffer lifetime and inherited DMA are not fully established. See [initial RAM policy](initial-ram-map.md); all high display/scratch storage is excluded initially. These findings do not identify the panel or prove DMA shutdown.
