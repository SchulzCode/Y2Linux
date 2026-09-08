# Unknowns and architecture decisions

Date: 2026-09-08. [Y2E-130](https://github.com/SchulzCode/Y2Linux/issues/7) fixes the offline memory policy after Y2E-125. **M0 remains open.** The owner subsequently authorized direct M1 offline implementation; its [validated result](../build/first-boot-result.md) does not clear hardware gates.

## Resolved or materially improved

| ID | New evidence / decision |
| --- | --- |
| U01/U02 recovery selection | Owner confirms successful SPFT flashing of this exact Y2 with the FM ROM. It is the owner-proven fallback; 24 current files and scatter are hashed. Exact current installed hashes and historical-byte identity remain distinct unknowns. |
| U03 tool identity | Local executable identifies v5.2032.00 / 5.2032.00.sn100; archive matches executable; recent-file history names the FM scatter. Previously used build strongly corroborated; DA/settings not proved. |
| U06 recovery viability | Successful owner-observed same-device recovery/flash accepted as strong evidence. Do not re-prove from scratch or require deliberate damage. Exact emergency entry/runbook details remain incomplete. |
| U07 address/boot-data discrepancy | Package LK overrides header addresses: kernel `0x80008000`, tags `0x80000100`, ramdisk `0x84000000`, machine ID 6582. ATAG construction and final entry traced. No RAM-alias explanation is required. |
| U07a / U08 memory fixups | FM symbols independently remapped. D08 fixes a 24.5 MiB static DT map, disables ATAG import, and specifies exact permitted load/decompression/DTB/initramfs intervals and artifact rejection rules. [Full policy](initial-ram-map.md). This resolves offline memory architecture, not DMA-safe launch. |
| U08/U11 boot fundamentals | Upstream v6.18 includes mt6582.dtsi, ARMv7/GIC/sysirq/GPT/UART foundation, GENERIC_DT fallback and appended-DTB/ATAG conversion. No new legacy board-file port is presently justified. This is not proof of a complete working Y2 port. |

## Remaining unknowns

| ID | Bounded missing evidence | Blocks / current action |
| --- | --- | --- |
| U01 | Current PCB/build and installed loader/image lineage | Pin the actual experiment target. Do not infer exact installed bytes from fingerprint or package name. |
| U02 | Original download authenticity and current bytes versus historical flashed bytes | Record provenance accurately; does not negate the owner's successful same-device fallback. |
| U03 | Exact historical DA, flash mode/targets and reviewed acquisition/connection semantics | Future backup/runbook. Bundled DA hash is a candidate, not an established historical selection. |
| U04 | PRELOADER hardware-region configuration, EBR span and BMTPOOL semantics | Special-region acquisition. Ordinary boot/recovery metadata is reconciled; no guessed raw ranges. |
| U05 | Consistent per-device boot/recovery/calibration/protect backups and independent copies | Recovery gate. No factory-data substitution or unapproved filesystem freeze/unmount. |
| U06 | Detailed independent-of-Android entry sequence and recorded post-restore checks | Future operator procedure; reuse owner-proven recovery, no broad viability re-test. |
| U07c | Live inherited DMA/secure state; loader heap extent, display scratch lifetime, physical ranks and secure/shared aliases remain incompletely observed | **Launch gate.** Reduced static memory avoids identified storage but is not bus-master containment. Trace the selected normal loader path and obtain bounded stock evidence; no arbitrary MMIO probes. Full rank topology/high-memory reclamation is deferred. |
| U07b | Installed loader identity and hardware-selected authentication policy | Y2E-145 finds package ROM_INFO +0xe8=0x22 still consults bit 1 of 0x10206060; not unconditional disable. Current ADB exposes neither installed bytes nor a reliable policy result. [Assessment](launch-readiness.md). |
| U08 | Actual runtime working set and board execution | Offline sizes, DTB and symbol-derived D08 layout are now validated and reproducible. Runtime boot/memory/interrupt behavior remains untested; no RAM expansion is authorized. |
| U09 | Conflicting battery readings / trusted power and consistency prerequisites | Future acquisition/restore/boot procedure; passive evidence only. |
| U10a | Physically usable console, routing, voltage/pins, baud and stock output | Y2E-140 confirms installed ttyMT0/UART0 controller selection and corrects/revalidates the artifact. Board TX/GND, pad voltage, live baud and stock LK+kernel serial capture remain open; host PTY tests and ADB last_kmsg are not that proof. [Evidence](observation-path.md). |
| U10b | Panel/input, audio supplies/reset/clocks/analog path, radios/firmware/calibration | Later subsystem work, one bounded proof at a time. |
| U11 | Y2-compatible clocks/pinctrl/MMC, SMP release and full peripheral support | Later persistent rootfs/platform work. Source names/sibling compatibles do not prove support. |

## Decisions

- **D01:** Y2Linux owns platform evidence, recovery, kernel/DT research and implementation issues; Y2PlayerNative is the later native Rust + C application. Initial issues were transferred, preserving stable IDs/history.
- **D02:** Research precedes small specified implementation checkpoints. For M1 the owner explicitly authorized the same lead to implement directly, with no delegation. No task in this wave is `state:luna-ready`.
- **D03:** Failed generated-distribution checks quarantine those claims; independently verified historical evidence and safe observations remain useful. Missing/denied evidence is not silently promoted to success.
- **D04:** Closed Y2E-101/105/110/115/120 represent completed bounded investigations, not M0 passage. Y2E-125 updates their conclusions without duplicating their backlog.
- **D05:** Candidate boot strategy: retain preloader/LK; legacy 2 KiB BOOTIMG with KERNEL(zImage + appended Y2 DTB), ROOTFS(small initramfs), CPU0 and observable console. No persistent eMMC rootfs or Linux-initiated eMMC writes; any later BOOTIMG-only deployment separately requires passed launch gates and explicit authorization. DT/ATAG policy is now fixed by D08.
- **D06:** Accept owner-observed successful SPFT recovery and the corroborated v5.2032 installation. Focus remaining recovery work on exact procedure/backups, not proving viability again. Preserve preloader/calibration and prohibit destructive formatting.
- **D07:** Treat the supplied Debian/replacement-BOOTIMG description as a consistent engineering lead, not implementation evidence. Kernel target is upstream 6.18 LTS. Do not repurpose everything after BOOTIMG; recovery/security/table partitions intervene.

- **D08:** [Initial RAM policy](initial-ram-map.md): 24 MiB low RAM and 512 KiB initramfs bank; all other DRAM omitted, first 16 KiB retained. Appended DTB with no ATAG import; controlled DT bootargs and exact inner initrd length. Fixed caps and symbol-based relocation/overread checks permit bounded offline implementation. Failure to fit returns for review; no automatic memory expansion. U07c remains a launch gate.

Next implementation boundary can be a tiny offline artifact/layout task under D08, after its concrete issue is specified. No implementation issue was created in Y2E-130. Next research is a bounded prerequisite for a hardware launch: inherited DMA/secure-state assurance or physical console evidence, with installed-loader and recovery prerequisites preserved. Do not bundle all peripherals into a speculative backlog.

- **D09:** [Kconfig compatibility and console policy](../build/kernel-policy.md). One reviewed prompt-visibility patch makes the existing D08 `ARM_VIRT_EXT=n` requirement selectable on v6.18; upstream default and all memory limits stay unchanged. Static `PHYS_OFFSET=0x80000000` is checked. The original UART3 candidate is superseded by D10; physical readiness remains U10a.

## M1 offline boundary

Y2B-201 through Y2B-235 deliver a reproducible, strictly validated offline candidate. [Launch-gate review](first-boot-launch-gates.md) records the rechecked FM security branches, storage DMA stop, final cleanup and UART selection, plus the smallest missing live/backup proofs. U07b/U07c/U10a and recovery/power remain open; M0 is not passed and M1 has no hardware-boot result. No device interaction occurred during this wave.

## Y2E-140 observation boundary

- **D10:** [Installed observation evidence and controller correction](observation-path.md): UART0 at 0x11002000, IRQ51, sole serial0/ttyS0 alias; D08 and kernel features unchanged. Two complete clean builds pass and match. Expected native logic is 1.8 V; 921600 8N1 is the firmware-supported receive candidate, not a measured board rate.
- U10a remains open for board-specific TX/GND/level and actual stock LK/kernel capture. No safe physical connection was established. Bounded alternative: trace the FM preloader's existing UART-over-USB accessory selection/electrical requirements if native TX access is unavailable; do not guess a cable or change the PHY live.
- This later research used passive stock ADB, including a previous kernel RAM-console tail. It made no device state change or experimental boot. After U10a passes, resolve installed loader/authentication U07b, with U07c and backup/recovery/power gates preserved.

## Y2E-145 GO / NO-GO boundary

**NO-GO** for the requested 60-second first BOOTIMG-only experiment. Ordinary USB/ADB does not provide the proved early observation channel; current BOOTIMG backup/independent retention and installed authentication remain missing. Normal package LK arms a nominal 10-second watchdog interval with IRQ/dual-mode behavior; identified disable calls are Download/Fastboot, not normal BOOTIMG handoff. Treat it as inherited until disproved. Current candidate has no watchdog servicing, so a reviewed minimal timing/watchdog policy may be needed after installed-lineage proof. No feature or D08 change was made to guess past this gate.

[Complete decision/evidence](launch-readiness.md), [blocked BOOTIMG backup/restore procedure](bootimg-recovery-runbook.md). Y2E-140 (#16) and Y2E-145 (#17) stay open. No first-boot execution issue was created because GO was not reached.
