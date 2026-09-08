# Linux 6.18 boot-fundamentals audit

Date: 2026-09-08. [Y2E-125](https://github.com/SchulzCode/Y2Linux/issues/6). Status: **source support CONFIRMED; Y2 execution UNKNOWN**. Target remains upstream Linux **6.18 LTS**, not 6.12 or a forward-ported Android kernel.

Pinned upstream tag: `v6.18`; tag object `f7b88edb52c8dd01b7e576390d658ae6eef0e134`; peeled commit `7d0a66e4bb9081d75c82ec4957c50034cb0ea449`. This is a reproducible architecture baseline, not a final selection of the 6.18.y patch release. The [kernel.org release page](https://www.kernel.org/category/releases.html) identifies the longterm series; that does not certify Y2 support. Successful source files/URLs/hashes are in [linux-6.18-sources.tsv](linux-6.18-sources.tsv). Failed/rate-limited requests remain explicitly recorded privately; claims below use retrieved primary source.

## Existing support in v6.18

| Fundamental | Exact source evidence | Implication for Y2 |
| --- | --- | --- |
| CPU / SoC | [mt6582.dtsi](https://github.com/torvalds/linux/blob/v6.18/arch/arm/boot/dts/mediatek/mt6582.dtsi) defines four Cortex-A7 CPUs and mt6582 compatibility; [proc-v7.S](https://github.com/torvalds/linux/blob/v6.18/arch/arm/mm/proc-v7.S) provides ARMv7 support. | A real MT6582 CPU0 foundation exists. |
| Platform selection | [Kconfig](https://github.com/torvalds/linux/blob/v6.18/arch/arm/mach-mediatek/Kconfig) supports ARCH_MULTI_V7 and selects GIC/MTK_TIMER. [mediatek.c](https://github.com/torvalds/linux/blob/v6.18/arch/arm/mach-mediatek/mediatek.c) omits mt6582 from its compatible list. | Do not invent an mt6589 root compatible. [devtree.c](https://github.com/torvalds/linux/blob/v6.18/arch/arm/kernel/devtree.c) provides GENERIC_DT fallback; [time.c](https://github.com/torvalds/linux/blob/v6.18/arch/arm/kernel/time.c) has default OF clock/timer initialization. A legacy machine-ID 6582 port is not inherently required. |
| Interrupts | Cortex-A7 GIC at `0x10211000` / `0x10212000` plus additional CPU-interface ranges; sysirq at `0x10200100`. [irq-mtk-sysirq.c](https://github.com/torvalds/linux/blob/v6.18/drivers/irqchip/irq-mtk-sysirq.c) matches mt6577-sysirq fallback in the dtsi. | GIC and polarity-controller mechanisms exist; board behavior untested. |
| Timer | MT6577-compatible GPT, actual reg `0x10008000`, length `0x80`, SPI 112 active-low; [timer-mediatek.c](https://github.com/torvalds/linux/blob/v6.18/drivers/clocksource/timer-mediatek.c) registers this compatible. | GPT is the starting timer. The dtsi node name is `timer@11008000` but its reg says `0x10008000`: derive MMIO from reg, and retain this inconsistency for review. |
| UART | Four disabled UART nodes at `0x11002000` through `0x11005000`, SPI 51–54; mt6582-uart / mt6577-uart. [8250_mtk.c](https://github.com/torvalds/linux/blob/v6.18/drivers/tty/serial/8250/8250_mtk.c) implements normal and earlycon paths. | UART3 is a candidate from LK's default string, not a physical pinout. Upstream ttyS naming, aliases, inherited routing and baud must be specified. |
| Initial clocks | dtsi supplies fixed 13 MHz system, 32 kHz RTC and 26 MHz UART clocks. | Minimal timer/UART scaffolding, not a full clock/reset/power implementation or independent clock measurement on Y2. |
| Watchdog | dtsi `0x10007000`, mt6582-wdt / mt6589-wdt; [mtk_wdt.c](https://github.com/torvalds/linux/blob/v6.18/drivers/watchdog/mtk_wdt.c) matches the latter. | Driver path exists; inherited state and experiment policy are unverified. |
| Secondary CPUs | mt6582 CPU nodes lack enable-method; [platsmp.c](https://github.com/torvalds/linux/blob/v6.18/arch/arm/mach-mediatek/platsmp.c) does not establish mt6582 CPU-release semantics. | Use CPU0 initially. Four CPU nodes do not prove working SMP. |
| eMMC | No MMC node in mt6582.dtsi; [mtk-sd.c](https://github.com/torvalds/linux/blob/v6.18/drivers/mmc/host/mtk-sd.c) has no mt6582/mt6589/mt6577 match. Audited clock/pinctrl directories contain no mt6582-named implementation. | Persistent storage needs additional research. A sibling compatible is not proof of hardware support. Start with initramfs. |

The [Prestigio tablet DTS](https://github.com/torvalds/linux/blob/v6.18/arch/arm/boot/dts/mediatek/mt6582-prestigio-pmt5008-3g.dts) is an upstream consumer of the SoC description. Its 1 GiB memory and UART choices are **tablet settings, not Y2 evidence**. No Y2 board description is established in these inputs. Panel, audio/DAC, controls, USB, charging, radios and complete clock/pinctrl support are outside the proof delivered here.

## DT kernel behind an ATAG bootloader

The [ARM boot protocol](https://github.com/torvalds/linux/blob/v6.18/Documentation/arch/arm/booting.rst) accepts tags or FDT through r2 and requires initialized RAM, suitable CPU/cache state and quiesced DMA. [boot-chain.md](boot-chain.md) traces stock LK's `r0=0`, `r1=6582`, `r2=0x80000100`, entry `0x80008000`, with ATAGs.

[ARM Kconfig](https://github.com/torvalds/linux/blob/v6.18/arch/arm/Kconfig) and [compressed/head.S](https://github.com/torvalds/linux/blob/v6.18/arch/arm/boot/compressed/head.S) support `ARM_APPENDED_DTB` for old loaders. The DTB belongs immediately after zImage **inside KERNEL**, not in an assumed Android DT section. This lets a DT-based upstream kernel run behind a loader that still supplies ATAGs; it does not require a legacy Android board-file port.

`ARM_ATAG_DTB_COMPAT` uses [atags_to_fdt.c](https://github.com/torvalds/linux/blob/v6.18/arch/arm/boot/compressed/atags_to_fdt.c), which accepts LK's two-word CORE tag and transfers standard MEM, CMDLINE, INITRD2 and SERIAL data. It does not translate vendor framebuffer/modem reservations. MEM can replace `/memory/reg`; default command-line policy replaces DT bootargs, while EXTEND appends loader arguments after DT arguments. A small DT memory range or clean DT command line therefore does not automatically win.

The outstanding memory proof must decide between importing tags with adequate explicit exclusions and disabling import with a fully specified static memory/initrd description. For a controlled experiment, `CMDLINE_FORCE` is available to avoid LK's Android console/root defaults. External ramdisk location/size may be imported from INITRD2, or specified only when packaging lengths are fixed. No config was authored here, and the implementation role must not be asked to choose these policies.

## Decision boundary

Candidate: retain preloader/LK; package zImage + appended Y2 DTB in a legacy BOOTIMG KERNEL payload, and a small initramfs in ROOTFS; use CPU0 and an observable console. This is supported by static evidence, not a demonstrated working Y2 recipe.

The smallest remaining artifact-design proof is the safe initial memory envelope/exclusions, including inherited DMA, to fix DT/ATAG policy and all memory intervals. Installed loader/security-state evidence, a usable observation channel and the backup/recovery controls separately gate experimentation. No kernel/DTS implementation, boot image or implementation-ready issue was created.
