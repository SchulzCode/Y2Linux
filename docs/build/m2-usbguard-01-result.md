# M2-USBGUARD-01 — ready for owner device test

**Superseded as the immediate next test by [M2-USBACM-01](m2-usbacm-01-result.md)**
at the owner's request to combine enumeration and logging. This image remains
an untested diagnostic fallback; the procedure below is its historical handoff.

2026-09-09, #23/#27 prerequisite under active M2 #28, baseline `de1a6d0`.
**Offline validated; not flashed or hardware tested.**

`/home/luca/Dokumente/Code/Y2Linux/out/m2-usbguard-01/BOOTIMG.img`

**1,097,728 bytes**; SHA-256:
`e4078583ab53bd7e6388c94a357023fe8a7504ccc5a7499341cf72287ecd19b5`.

Screen/build **M2-USBGUARD-01**, Linux **6.18.0-y2-m2-usbguard1**.
[Exact diagnostic contract](../knowledge/m2-usb-guard-diagnostic.md).

## Purpose

[Your CHRDET photos](../knowledge/m2-chrdet-hardware-result.md) show successful
PMIC detection in both reported startup conditions. With USB connected at
startup, PHY6a=BE causes the wake guard to refuse before writing. The previous
screen concealed the original guard inputs with unread zero-filled wake data.

This candidate preserves the original MAC/PHY/DMA/IRQ-enable rows when the early
wake guard refuses. Unread fields display dashes; successful wake retains its
before/after rows. There is no change to register accesses, guards or the cached
260-byte snapshot. Existing guarded PHY6a release remains possible under its
proven passive-state conditions. No new PHY/PMIC/clock/VBUS/MAC/DMA write.

## Validation

- One clean pinned Linux 6.18 / Clang 20.1.8 build, zero compiler warnings/errors.
  All **15 selected test methods** pass, including **26 QEMU ARM PID1 scenarios**
  and **49 production PHY-wake scenarios**. Tests preserve distinct raw values
  on early refusal, show unread fields on partial reads, preserve the error and
  finish the bounded heartbeat loop. PHY6a=BE refuses with zero extra reads and
  zero writes. Sentinel fixture values are synthetic, not device measurements.
- Actual artifact/mutation, BOOTIMG, D08, time32 syscall, text/framebuffer and
  watchdog checks pass. Resolved config differs only in LOCALVERSION. DT memory
  and hardware policy remain unchanged; chosen initrd end follows its new size.
  [Layout](results/m2-usbguard-01-layout.json) and
  [iteration record](results/m2-usbguard-01-iteration.json) retain exact results.
- Linked snapshot at c0015208 remains PID1-only/cached and checks size 260.
  Compared with CHRDET-01, its only two changed instructions load the relocated
  `y2-usb-state` resource-name pointer; both old/new strings were checked.
  All other snapshot instructions and the USB-read/PWRAP-read/request/ack/delay
  helpers compared are byte-identical. The guarded PHY write remains c0015b90.
  Hardware-access source hashes match the retained previous layout.
- D08 described RAM remains 24 MiB + 512 KiB. Resident kernel remains
  [0x80008000,0x8029f5b0). Initramfs gzip is 6472 bytes and ends at 0x84001948;
  decompressor workspace ends at 0x803909e8. Complete LK read-tail and 16 MiB
  BOOTIMG partition checks pass. No duplicate build or unchanged provenance audit.
- Output retains source snapshot, input lock, ELFs/config/build and test logs,
  focused linked disassembly/comparison and artifact hashes.

## Owner test — stop here

Use the established **BOOTIMG-only procedure**. Attach the USB data cable to
the Linux PC **before powering on the Y2**, and leave it attached throughout
this boot. Keep the existing maximum **60 seconds from power-on** and manual
BOOTIMG restoration boundary. No other partition or persistent calibration is
part of this experiment.

Expected: continuing heartbeat, `PW:0 /7`, `CLK:0/15`, valid CHRDET and either:

- **Guard refuses again:** LAST ERR PHY WAKE -19; bottom rows `PRE P:… D:…
  HW:… W:-19/0`, `PHY68:…`, `DMA:…`, `IRQE TX:… RX:… USB:…`.
  This is a useful result; it exposes the previously hidden startup state.
- **Wake succeeds:** established WAKE/6A/CTL/TRIM before/after rows. Record this
  result as seen; successful wake alone still does not establish a USB session.

Return **two readable full-screen photos from the same boot**, near BEAT 10 and
near BEAT 50/STOP within the existing time limit, in `out/m2-usbguard-01/`.
Include all four bottom rows, CHR/PW/CLK, LAST ERR, heartbeat/uptime and IRQ count.
Report any freeze/reset and exact cable timing if it differed. If stalled,
record the last visible stage and elapsed time; do not extend the time limit.
No enumeration or ttyACM is expected yet.

Retained fallback: `out/m2-chrdet-01/BOOTIMG.img`, SHA-256
`e511fb89e45326673183f426a4bdd3b6ca708c25bf3e6ceecfc4a67478aa9660`.
Its connected-start guard refusal is now documented; it is not a connected-wake
success fallback. Earlier PHYWAKE-01 remains retained with its recorded evidence.

No physical device operation was performed. #23/#27/#28 remain open; M1 stays
complete, M2 exit blocked, M3/audio and Y2PlayerNative deferred.
