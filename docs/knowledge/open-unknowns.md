# Unknowns and architecture decisions

Date: 2026-09-08. [Y2E-125](https://github.com/SchulzCode/Y2Linux/issues/6) extends the five completed initial research tasks. **M0 remains open.** Research/architecture decisions precede any implementation handoff.

## Resolved or materially improved

| ID | New evidence / decision |
| --- | --- |
| U01/U02 recovery selection | Owner confirms successful SPFT flashing of this exact Y2 with the FM ROM. It is the owner-proven fallback; 24 current files and scatter are hashed. Exact current installed hashes and historical-byte identity remain distinct unknowns. |
| U03 tool identity | Local executable identifies v5.2032.00 / 5.2032.00.sn100; archive matches executable; recent-file history names the FM scatter. Previously used build strongly corroborated; DA/settings not proved. |
| U06 recovery viability | Successful owner-observed same-device recovery/flash accepted as strong evidence. Do not re-prove from scratch or require deliberate damage. Exact emergency entry/runbook details remain incomplete. |
| U07 address/boot-data discrepancy | Package LK overrides header addresses: kernel `0x80008000`, tags `0x80000100`, ramdisk `0x84000000`, machine ID 6582. ATAG construction and final entry traced. No RAM-alias explanation is required. |
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
| U07a | Exact safe RAM envelope/exclusions at LK exit, including display/preloader/secure-world/DMA state | **Smallest remaining artifact-design proof.** Resolve DT memory and ATAG import policy before specifying an image. Current iomem is insufficient to identify all reservations. |
| U07b | Active image-authentication policy and exact installed LK | Launch gate. Static LK has both conditional verification and no-check paths. Stock flashing does not prove unsigned-kernel acceptance. |
| U08 | Exact FM kernel reservation functions / minimal Y2 board description | Remap old symbols against FM bytes. Old kernel fixup/reservation mechanisms are evidence, not a source port. |
| U09 | Conflicting battery readings / trusted power and consistency prerequisites | Future acquisition/restore/boot procedure; passive evidence only. |
| U10a | Physically usable console, routing, voltage/pins, baud and stock output | Observable first experiment. ttyMT3 string and UART3 MMIO are candidates, not pinout proof. No blind probing. |
| U10b | Panel/input, audio supplies/reset/clocks/analog path, radios/firmware/calibration | Later subsystem work, one bounded proof at a time. |
| U11 | Y2-compatible clocks/pinctrl/MMC, SMP release and full peripheral support | Later persistent rootfs/platform work. Source names/sibling compatibles do not prove support. |

## Decisions

- **D01:** Y2Linux owns platform evidence, recovery, kernel/DT research and implementation issues; Y2PlayerNative is the later native Rust + C application. Initial issues were transferred, preserving stable IDs/history.
- **D02:** Research and architectural choices belong to the research owner. Implementation receives tiny fully specified changes. No current task is `state:luna-ready`.
- **D03:** Failed generated-distribution checks quarantine those claims; independently verified historical evidence and safe observations remain useful. Missing/denied evidence is not silently promoted to success.
- **D04:** Closed Y2E-101/105/110/115/120 represent completed bounded investigations, not M0 passage. Y2E-125 updates their conclusions without duplicating their backlog.
- **D05:** Candidate boot strategy: retain preloader/LK; legacy 2 KiB BOOTIMG with KERNEL(zImage + appended Y2 DTB), ROOTFS(small initramfs), CPU0 and observable console. No eMMC writes in the first experiment. DT/ATAG-import policy waits for U07a.
- **D06:** Accept owner-observed successful SPFT recovery and the corroborated v5.2032 installation. Focus remaining recovery work on exact procedure/backups, not proving viability again. Preserve preloader/calibration and prohibit destructive formatting.
- **D07:** Treat the supplied Debian/replacement-BOOTIMG description as a consistent engineering lead, not implementation evidence. Kernel target is upstream 6.18 LTS. Do not repurpose everything after BOOTIMG; recovery/security/table partitions intervene.

Next rolling-wave proof: U07a, supported by remapping the FM reservation functions and existing stock memory/display evidence. Deliver an explicit usable-memory/exclusion map and a decision on ATAG import with all load/decompression/initrd intervals accounted for. Console/security/recovery checks remain separate launch gates. No speculative backlog or implementation issue is created at this boundary.
