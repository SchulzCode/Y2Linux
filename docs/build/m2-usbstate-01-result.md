# M2-USBSTATE-01 — ready for owner device test

> Returned [physical photos](../knowledge/m2-usb-state-hardware-result.md) confirm
> all 21 reads, zero DMA control words and BEAT 50 without reported errors.
> The original pending-test handoff below is historical.

2026-09-09, active M2 #28 and USB prerequisite #27/#23.
**Offline validated; this candidate has not been flashed or hardware tested.**
The preceding [clock-probe photos](../knowledge/m2-usb-clock-hardware-result.md)
confirm no errors through BEAT 50, timer IRQ reporting, VUSB and inherited USB
clock enables. M1 remains complete; M2 exit remains unqualified.

## Candidate

`/home/luca/Dokumente/Code/Y2Linux/out/m2-usbstate-01/BOOTIMG.img`

**1,095,680 bytes**; SHA-256:
`8c1f701b61a1adb70b1377071e6cb885cda989a422fd75d626cd5c7da5bdb5e7`.

Screen/build **M2-USBSTATE-01**, kernel **6.18.0-y2-m2-usbstate1**.
[Exact read contract](../knowledge/m2-usb-state-probe.md).

The first successful sleep is followed by the proven cached PWRAP and clock
snapshot, then 21 read-only USB observations if all supply/clock guards hold:
MAC POWER/DEVCTL/HWVERS, three interrupt-enable masks, seven digital PHY bytes
and eight DMA control words. No new hardware write is added. This neither
initializes the controller nor changes clocks, PHY calibration, USB mode, VBUS
or DMA ownership. USB enumeration/ttyACM is not expected yet.

## Validation

- One clean pinned Linux 6.18 / Clang 20.1.8 build, no compiler warnings/errors.
  Full top-level suite: 20 methods pass, including 17 QEMU ARM PID1 scenarios,
  37 USB state guard/read-failure scenarios, 20 PWRAP and 10 clock scenarios.
  All four observation-tool tests also pass using host PTYs, not a physical Y2.
- Emitted PID1-only snapshot is 164 bytes, cached before access. USB adapter
  `0xc0015c50` enforces address and width, executes ldrb/ldrh at c0015cc8/c0015cd8,
  and stores only to result RAM. Claim/map/release paths and prerequisite guards
  are reviewed; no read of interrupt status, FIFO or endpoint index is introduced.
- Only resolved CONFIG_LOCALVERSION changes. DT source, D08, CPU0, watchdog
  instructions and framebuffer guard remain unchanged. New gzip is 5489 bytes.
  Native init and linked time32 syscall checks pass; no new syscall is added.
- [Layout](results/m2-usbstate-01-layout.json) and
  [iteration](results/m2-usbstate-01-iteration.json) retain exact dimensions.
  Resident kernel [0x80008000,0x8029f530), initramfs ends 0x84001571,
  decompressor workspace ends 0x80390618. BOOTIMG wrappers, complete read tail,
  memory bounds and 16 MiB BOOTIMG partition bound pass.

Ignored output retains ELFs, config, input lock, source snapshot, logs, focused
disassembly and SHA256SUMS. No repeated unchanged ROM/recovery/source audit or
duplicate reproducibility build was performed. No physical-device action occurred.

## Owner test and evidence

Use the established **BOOTIMG-only** test/recovery procedure and **60 seconds
maximum from power-on**. Stock preloader/LK, partitions and calibration remain
protected. Expected terminal stage is `STOP: RESTORE ANDROID` at BEAT 50.

Expected: screen/heartbeat, briefly `READ USB STATE`, then continuing progress.
Successful reads show **USB RC:0 VALID:001FFFFF**, **PW:0 /3 CLK:0/15**, increasing
timer IRQ count and these raw rows:

- POWER / DEV / HW: MAC power, device control and hardware version.
- PHY68: seven bytes, offsets 0x68 through 0x6e in order.
- DMA: eight consecutive four-hex-digit words, channels 0 through 7.
- IRQE TX / RX / USB: interrupt-enable masks, not pending interrupts.

Raw values are unknown until the trial; neither nonzero DMA nor PHY state should
be silently interpreted as failure/success. Only valid-mask bits are evidence:
bits 0–5 are POWER/DEV/HW/TXE/RXE/USBE, 6–12 PHY68..6E, 13–20 DMA0..7.
On supply/clock guard refusal, rows instead show PERI/MUX/PLL/PWR for diagnosis.
`USB RC:-19` refuses prerequisites, -16 is a resource conflict, -12 a mapping
failure; the heartbeat should continue. A bus hang may freeze at the pre-probe
stage; use the existing bounded manual recovery procedure.

Return full-screen photos near **BEAT 10 and BEAT 50/STOP**, or the exact last
visible stage on failure. Include USB RC/VALID, PW/CLK, all four raw rows, timer
count, uptime, MemTotal, mount/sleep status and LAST ERR. Also report selected
image hash (flashed/readback hash if available from the usual procedure), host
elapsed time, USB cable state, any reset/freeze and restoration outcome if done.

Retained prior image: `out/m2-usbclk-01/BOOTIMG.img`, SHA-256
`602080eb199f3a4be30082ee114dfeb97ef414e10d8ad4acf5d239f2096cd6ec`.
The owner-proven stock/FM restore and M1 fallback boundaries are unchanged.
Work stops here for the owner test; #23/#27/#28 remain open and M3 is deferred.
