# Display and input evidence

Date: 2026-09-08. Related task: Y2E-101. Historical source: integrity-verified snapshot `2026-07-29_004935/hardware/input-devices.txt`.

CONFIRMED in that archive: `mtk-kpd`, `ACCDET`, `mtk-tpd`, `mtk-tpd-kpd` and AVRCP input devices, including event handlers and capability bitmaps. The application documents a 480 × 360 landscape display and wheel/button navigation. That behavioral resolution is not a panel/controller/timing identification.

Android `KEYCODE_*` mappings are not Linux evdev scan codes. The old input-map note denying wheel acceleration is superseded by newer application source/release behavior; retain ordinary detent, hold/repeat, screen-off and Bluetooth-input distinctions as behavioral reference only.

UNKNOWN: panel part/interface, timings, backlight controller, touch/controller identity, orientation transformation, physical wheel/button wiring, IRQ/reset/power sequencing and wake sources. APT32F/UPDATE I2C names are discovery clues, not established functions.

Next proof is a bounded passive current-device capability/driver inventory matched to stock kernel symbols and display configuration. No synthetic input, panel writes, unreviewed controller sysfs reads, disassembly of an entire subsystem or Linux driver implementation is included in the first M0 batch.
