# First-device launch gates after the M1 offline wave

2026-09-08; Y2B-235. **Offline artifact acceptance is complete only when its reproducibility report passes; hardware launch is still blocked.** This review used retained package disassembly and audited upstream sources, with no ADB, device mode change, raw readback, DA execution, flash or boot. Known-good owner recovery remains strong evidence. No research was delegated.

## U07b — Installed LK and authentication

The FM `lk.bin` analysis is package evidence, not installed bytes. Re-reading retained `20260908-ram-map/lk.thumb.txt` establishes concrete branch conditions at `0x81e207d0`: the word at a runtime structure's +0x10 selects the software-secure path when equal to 1 (`0x81e207e8..0x81e207ec`); otherwise a policy helper at `0x81e20210` is called with argument 1. A zero return branches to `0x81e209e0` and the “Don't check” path. Other paths include BOOTIMG verification at `0x81e2081a..0x81e20820`, as well as unlock-only and SEC_RO checks. Static recovered register annotations are not treated as live values across branches.

**Unresolved proof:** installed LK/preloader identity and the selected normal boot's actual policy/configuration state. Package branch existence, stock-ROM flash success and an Android SHA-1 image ID cannot supply that proof. Next bounded acquisition must establish installed loader hashes and appropriately exposed stock security state/logs, with an approved backup/readback method if ordinary non-root exports cannot. No fuse/security setting changes, bypass, or unsigned-image trial is proposed as a discovery shortcut.

## U07c — DMA, entry mode and secure state

Retained FM instructions at `0x81e05334` set the eMMC DMA stop control and poll the active bit until clear (`0x81e0535e..0x81e05362`). The selected normal storage paths call this helper. Final `0x81e00458` still contains only its LED/GIC cleanup calls, with cache/MMU cleanup elsewhere. This strengthens the storage-specific argument but does not establish display, modem, connectivity or secure-world bus-master quiescence/containment for the installed loader.

D08 excludes LK/heap uncertainty, display scratch/high framebuffer, modem and connectivity storage. The actual artifact exposes no peripheral DMA driver, but these facts cannot stop inherited DMA. The D09 no-hyp build also requires the traced SVC entry contract to match the installed loader. The first artifact has no watchdog driver: inherited watchdog enable/timeout must be accounted for in launch timing, not assumed disabled. Next proof is a bounded final-handoff state analysis for the installed normal path and safe existing stock observations; no arbitrary register sweep or experimental boot. Full RAM expansion/physical rank discovery remains deferred.

## U10a — Observable console

FM LK's `0x81e00fc8` programs GPIO arguments `0x8000006d`/`0x8000006c`, then chooses UART3 at `0x11005000` unless its conditional helper selects UART0. It assigns baud `0x000e1000` (921600) and an 8-bit line setting. These are SoC/software identities, **not board-pad locations or voltage measurements**.

The offline DT selects only this UART3 candidate, upstream `ttyS0`, with the audited 26 MHz clock and IRQ54. Detailed v6.18 earlycon review found that the generic parser sets `device->baud` from stdout options before the driver callback. Therefore stdout-path is simply `serial0`, with no current-speed property; earlycon preserves inherited baud and only masks UART interrupts. Normal console configuration then requests `921600n8`. Relevant sources: `drivers/tty/serial/earlycon.c:278`, `8250/8250_mtk.c:664`, `8250/8250_early.c:154` in the locked source.

**Unresolved proof:** safe accessible routing/pads and logic voltage, actual installed selected port/baud, reliable captured stock output, and a stable capture arrangement. No blind wiring or serial register probes. No DEBUG_LL/decompressor console or USB fallback is claimed. A hang before earlycon can be silent. If evidence identifies UART0 instead, revise and revalidate the artifact before launch.

## Backup/recovery and power

Owner-confirmed SP Flash Tool restore with the FM ROM is accepted; local tool v5.2032.00 is corroborated and the package/scatter hashes are already recorded in [recovery.md](recovery.md). Remaining prerequisites are concrete: same-device boot/recovery and personalized NVRAM/calibration/protect/security backups with verified independent retention; exact tool/DA/mode/connection/target semantics; trusted power and a consistent acquisition state. Retained research files are not these backups. Loading a DA or changing boot mode is a separate operation, not passive ADB inventory. No destructive recovery rehearsal is required.

## Conditional first experiment — specification for later review, not executable authorization

1. Close the four gate groups above and retain their evidence. Verify exact artifact hashes and the D08 report. Confirm a stock console capture works before changing BOOTIMG. Record the operator's proven SPFT tool/DA/connection steps and a BOOTIMG-only restore path using the verified same-device backup.
2. After a separately authorized bounded procedure, use that proven transport in Download Only mode with **only BOOTIMG** selected. Target the established eMMC-user BOOTIMG interval `[0x01d80000,0x02d80000)`; write the exact validated file starting at its beginning, including its zero read tail. Require bounded readback/hash verification where the approved transport supports it. Do not modify preloader, LK, partition tables, recovery, Android/system/userdata or personalized regions. No Format All, repartition or rootfs installation.
3. Capture one normal stock-LK launch into the candidate, timestamped from before power-on. Expected evidence is Linux 6.18 banner, controlled cmdline, CPU0, D08 memory, PID1 `Y2DIAG READY` and at least three five-second heartbeats. Confirm proc/iomem/meminfo and interrupt diagnostics; described RAM is 24.5 MiB, not predicted MemTotal. No input or persistent writes are needed.
4. Planned observation window: at most 60 seconds, subject to the confirmed watchdog/power procedure. On silence, panic, reset loop or unexpected memory/CPU state, stop, retain the log and return to research; do not repeat with expanded RAM or changed loaders. Restore only the backed-up BOOTIMG through the known-good bounded route, then verify stock boot and owner-defined functional/calibration checks.

Exact DA/entry/readback and console-wiring steps remain intentionally unresolved until evidence supplies them. This is therefore a conditional target experiment, not a ready-to-run flashing recipe. No verified nonpersistent `fastboot boot` route is available to substitute. M0 and the hardware completion of M1 remain open.
