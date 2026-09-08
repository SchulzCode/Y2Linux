# Display and input evidence

> Y2E-150 resolves the limited LK diagnostic contract: normal final layer 2 at 0xbfb00000, 480x360 RGB565, 960-byte stride; DSI sync-pulse video and normal backlight persistence supported by exact FM/LK instructions. Android runtime metadata differs (32 bpp, stride 1920). [D13](risk-accepted-diagnostic.md) provides guarded access, provenance and limits; no panel initialization/input driver was implemented.

Date: 2026-09-08. Related task: Y2E-101. Historical source: integrity-verified snapshot `2026-07-29_004935/hardware/input-devices.txt`.

CONFIRMED in that archive: `mtk-kpd`, `ACCDET`, `mtk-tpd`, `mtk-tpd-kpd` and AVRCP input devices, including event handlers and capability bitmaps. The application documents a 480 × 360 landscape display and wheel/button navigation. That behavioral resolution is not a panel/controller/timing identification.

Android `KEYCODE_*` mappings are not Linux evdev scan codes. The old input-map note denying wheel acceleration is superseded by newer application source/release behavior; retain ordinary detent, hold/repeat, screen-off and Bluetooth-input distinctions as behavioral reference only.

UNKNOWN: panel part/interface, timings, backlight controller, touch/controller identity, orientation transformation, physical wheel/button wiring, IRQ/reset/power sequencing and wake sources. APT32F/UPDATE I2C names are discovery clues, not established functions.

Next proof is a bounded passive current-device capability/driver inventory matched to stock kernel symbols and display configuration. No synthetic input, panel writes, unreviewed controller sysfs reads, disassembly of an entire subsystem or Linux driver implementation is included in the first M0 batch.

## Y2E-130 memory evidence

The retained stock iomem identifies framebuffer `[0xbfb00000,0xc0000000)` (5 MiB). FM display-size functions were remapped at `0xc05128d4`/`0xc051296c`; the unknown-panel fallback is actually 20 MiB despite its misleading log. LK also forms a scratch pointer framebuffer minus 4 MiB. Buffer lifetime and inherited DMA are not fully established. See [initial RAM policy](initial-ram-map.md); all high display/scratch storage is excluded initially. These findings do not identify the panel or prove DMA shutdown.
