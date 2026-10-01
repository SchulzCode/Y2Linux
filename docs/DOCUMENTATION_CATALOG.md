# Y2Linux complete document catalog

Updated 2026-10-01. Every project-owned Markdown page in this repository
is listed once; vendored upstream package documentation is excluded.
Start with the current-state page; a historical
report records its own build/session, not the presently installed image.
Raw hardware and build evidence is preserved unchanged. The catalogs
document links, rather than relocating or deleting proof. See the
[shared knowledge rules](KNOWLEDGE_BASE.md)
and the other repository's catalog for product-wide context.

Indexed pages: **269**. Explicit historical/contract scope notices:
**183**.

## Entry

| Page | Scope |
| --- | --- |
| [Y2Linux working rules](../AGENTS.md) | Repository working rules |
| [Y2Linux](../readme.md) | Current entry / index |
| [2026-09-23 platform entry snapshot](CURRENT_PLATFORM_STATE.entry-2026-09-23.md) | Historical entry snapshot |
| [Current Y2Linux platform state](CURRENT_PLATFORM_STATE.md) | Current entry / index |
| [Y2Linux complete document catalog](DOCUMENTATION_CATALOG.md) | Current entry / index |
| [Knowledge-base authority and maintenance](KNOWLEDGE_BASE.md) | Current entry / index |
| [Y2Linux documentation](README.md) | Current entry / index |

## Architecture

| Page | Scope |
| --- | --- |
| [Platform API v1](architecture/platform-api-v1.md) | Source contract |
| [Bluetooth platform v1 contract](architecture/platform-bluetooth-v1.md) | Source contract |
| [Owner reset and state export](architecture/platform-maintenance-v1.md) | Source contract |
| [Network, clock and entropy contract v1](architecture/platform-network-time-v1.md) | Source contract |
| [Platform power contract v1](architecture/platform-power-v1.md) | Source contract |
| [Storage lifecycle, space and measurements](architecture/platform-storage-v1.md) | Source contract |
| [Signed root-only update contract v1](architecture/platform-update-v1.md) | Source contract |
| [USB device and owner file transfer v1](architecture/platform-usb-v1.md) | Source contract |
| [Manual first installation — Production Storage v1](architecture/production-install.md) | Historical design / runbook |
| [Production v1 restoration](architecture/production-recovery.md) | Historical design / runbook |
| [Production Storage / Installation v1 partition audit](architecture/production-storage-v1.md) | Historical design / runbook |
| [Reborn FFmpeg 9.0.2 production build](architecture/reborn-ffmpeg9-build.md) | Source contract |
| [Y2Linux update model — layout v1](architecture/update-model.md) | Historical design / runbook |

## Build

| Page | Scope |
| --- | --- |
| [Legacy BOOTIMG package contract](build/bootimg-format.md) | Historical build/deployment receipt |
| [Locked offline build environment](build/environment.md) | Historical build/deployment receipt |
| [REBORN-BASELINE-01 candidate evidence](build/evidence/reborn-baseline-01/README.md) | Preserved raw/reviewed evidence |
| [Radio UI correction receipts](build/evidence/reborn-radio-ui-01/README.md) | Preserved raw/reviewed evidence |
| [Reborn splash receipts](build/evidence/reborn-splash-01/README.md) | Preserved raw/reviewed evidence |
| [GPU-01 retained build evidence](build/evidence/y2linux-gpu-01/README.md) | Preserved raw/reviewed evidence |
| [GPU-02 retained build evidence](build/evidence/y2linux-gpu-02/README.md) | Preserved raw/reviewed evidence |
| [Retained packaging metadata](build/evidence/y2linux-m3-audio-01/README.md) | Preserved raw/reviewed evidence |
| [Retained packaging metadata](build/evidence/y2linux-m3-audio-02/README.md) | Preserved raw/reviewed evidence |
| [Y2LINUX-M4-01 build evidence — 2026-09-14](build/evidence/y2linux-m4-01/README.md) | Preserved raw/reviewed evidence |
| [M4 ADC production update: build and owner boot receipt](build/evidence/y2linux-m4-adc-01/README.md) | Preserved raw/reviewed evidence |
| [M4 charging candidate: build receipt](build/evidence/y2linux-m4-charge-01/README.md) | Preserved raw/reviewed evidence |
| [M4-POWER-02 integrated candidate receipt](build/evidence/y2linux-m4-power-02/README.md) | Preserved raw/reviewed evidence |
| [M4-POWER-03 build receipt — 2026-09-15](build/evidence/y2linux-m4-power-03/README.md) | Preserved raw/reviewed evidence |
| [M5-CONNECTIVITY-01 build evidence](build/evidence/y2linux-m5-connectivity-01/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-02 build evidence](build/evidence/y2linux-m5-connectivity-02/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-03 build evidence (crashing candidate)](build/evidence/y2linux-m5-connectivity-03/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-04 build evidence](build/evidence/y2linux-m5-connectivity-04/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-05 build and physical result](build/evidence/y2linux-m5-connectivity-05/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-06: build passed, factory provider blocks this boot](build/evidence/y2linux-m5-connectivity-06/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-07 retained build receipt and superseding physical result](build/evidence/y2linux-m5-connectivity-07/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-08 build and targeted validation](build/evidence/y2linux-m5-connectivity-08/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-09: permanent E2 HCI page correction](build/evidence/y2linux-m5-connectivity-09/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-10 validation receipts](build/evidence/y2linux-m5-connectivity-10/README.md) | Preserved raw/reviewed evidence |
| [Production Storage v1 Storage02 retry evidence](build/evidence/y2linux-production-v1-r2/README.md) | Preserved raw/reviewed evidence |
| [Storage03 offline validation](build/evidence/y2linux-production-v1-r3/README.md) | Preserved raw/reviewed evidence |
| [Storage05 integrated production candidate evidence](build/evidence/y2linux-production-v1-r5/README.md) | Preserved raw/reviewed evidence |
| [Production owner-key data initialization evidence](build/evidence/y2linux-production-v1-r6-owner-key/README.md) | Preserved raw/reviewed evidence |
| [Storage06 production candidate evidence](build/evidence/y2linux-production-v1-r6/README.md) | Preserved raw/reviewed evidence |
| [Production Storage v1 candidate evidence](build/evidence/y2linux-production-v1/README.md) | Preserved raw/reviewed evidence |
| [Storage02 DIAG01 offline evidence](build/evidence/y2linux-storage-diag-01/README.md) | Preserved raw/reviewed evidence |
| [M1 offline first-boot result](build/first-boot-result.md) | Historical build/deployment receipt |
| [Reproduce the offline first-boot artifact](build/first-boot.md) | Historical build/deployment receipt |
| [First-boot kernel and console policy](build/kernel-policy.md) | Historical build/deployment receipt |
| [M2-BASELINE-01 — integrated Linux 6.18 core development baseline](build/m2-baseline-01-result.md) | Historical build/deployment receipt |
| [M2-BASELINE-02 — correct live display PHY handoff](build/m2-baseline-02-result.md) | Historical build/deployment receipt |
| [M2-BASELINE-03 — keep the log reader ahead of display probing](build/m2-baseline-03-result.md) | Historical build/deployment receipt |
| [M2-CHRDET-01 — owner result received](build/m2-chrdet-01-result.md) | Historical build/deployment receipt |
| [M2-INPUT-01 — GPIO navigation through evdev and USB logs](build/m2-input-01-result.md) | Historical build/deployment receipt |
| [M2-PHYWAKE-01 — build and owner test handoff](build/m2-phywake-01-result.md) | Historical build/deployment receipt |
| [M2-PWRAP-01 — USB power prerequisite ready for device test](build/m2-pwrap-01-result.md) | Historical build/deployment receipt |
| [M2-USBACM-01 — ready for owner enumeration test](build/m2-usbacm-01-result.md) | Historical build/deployment receipt |
| [M2-USBACM-02 — retain the failed PMIC poll](build/m2-usbacm-02-result.md) | Historical build/deployment receipt |
| [M2-USBACM-03 — wait for PMIC synchronization before commands](build/m2-usbacm-03-result.md) | Historical build/deployment receipt |
| [M2-USBACM-04 — one bounded disconnect/reconnect trial](build/m2-usbacm-04-result.md) | Historical build/deployment receipt |
| [M2-USBCLK-01 — ready for owner device test](build/m2-usbclk-01-result.md) | Historical build/deployment receipt |
| [M2-USBGUARD-01 — ready for owner device test](build/m2-usbguard-01-result.md) | Historical build/deployment receipt |
| [M2-USBSTATE-01 — ready for owner device test](build/m2-usbstate-01-result.md) | Historical build/deployment receipt |
| [Platform v1 source reconstruction and release boundary](build/platform-v1-reconstruction.md) | Historical build/deployment receipt |
| [REBORN-BASELINE-01 root-only deployment candidate](build/reborn-baseline-01-deployment.md) | Historical build/deployment receipt |
| [Reborn radio scan UI correction](build/reborn-radio-ui-01-deployment.md) | Historical build/deployment receipt |
| [REBORN-SPLASH-01 startup update](build/reborn-splash-01-deployment.md) | Historical build/deployment receipt |
| [Risk-accepted first-boot diagnostic result](build/risk-diagnostic-result.md) | Historical build/deployment receipt |
| [Stock console capture runbook](build/stock-console-capture.md) | Historical build/deployment receipt |
| [Storage02 DIAG01 — manual BOOTIMG-only observation](build/storage-diagnostic-install.md) | Historical build/deployment receipt |
| [Storage05: corrected SPFT readback coordinates](build/storage05-readback.md) | Historical build/deployment receipt |
| [Capture Y2 CDC ACM logs](build/usb-log-capture.md) | Historical build/deployment receipt |
| [Y2B-245 — Offline on-screen diagnostic candidate](build/y2b245-result.md) | Historical build/deployment receipt |
| [Y2B-250 — ARM time32 nanosleep fix](build/y2b250-result.md) | Historical build/deployment receipt |
| [Y2LINUX-DEV-01 — owner manual deployment](build/y2linux-dev-01-deployment.md) | Historical build/deployment receipt |
| [DEV-01 SD preparation from the owner's Mac](build/y2linux-dev-01-mac-sd.md) | Historical build/deployment receipt |
| [Y2LINUX-DEV-01 integrated development candidate](build/y2linux-dev-01-result.md) | Historical build/deployment receipt |
| [DEV-02: BOOTIMG-only correction for the existing SD root](build/y2linux-dev-02-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-DEV-02 — root handoff ABI and wheel transaction corrections](build/y2linux-dev-02-result.md) | Historical build/deployment receipt |
| [Y2LINUX-GPU-01 — manual deployment receipt](build/y2linux-gpu-01-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-GPU-02 — targeted suspend correction](build/y2linux-gpu-02-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-M3-AUDIO-01 — owner deployment](build/y2linux-m3-audio-01-deployment.md) | Historical build/deployment receipt |
| [M3-AUDIO-01 integrated candidate — offline result](build/y2linux-m3-audio-01-result.md) | Historical build/deployment receipt |
| [M3-AUDIO-02 — corrected PCM notification candidate](build/y2linux-m3-audio-02-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-M4-01 — integrated power candidate, manual deployment pending](build/y2linux-m4-01-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-M4-ADC-01 — owner boot confirmed, charging inhibited](build/y2linux-m4-adc-01-deployment.md) | Historical build/deployment receipt |
| [Y2LINUX-M4-CHARGE-01: manual BOOTIMG handoff](build/y2linux-m4-charge-01-deployment.md) | Historical build/deployment receipt |
| [M4-POWER-02 integrated physical qualification](build/y2linux-m4-power-02-deployment.md) | Historical build/deployment receipt |
| [M4-POWER-03 — manual deployment and physical qualification](build/y2linux-m4-power-03-deployment.md) | Historical build/deployment receipt |
| [M5-CONNECTIVITY-01 — manual deployment handoff](build/y2linux-m5-connectivity-01-deployment.md) | Historical build/deployment receipt |
| [CONNECTIVITY-08 — targeted RF calibration protocol correction](build/y2linux-m5-connectivity-08-deployment.md) | Historical build/deployment receipt |
| [CONNECTIVITY-09 — permanent MT6582 E2 HCI page correction](build/y2linux-m5-connectivity-09-deployment.md) | Historical build/deployment receipt |
| [CONNECTIVITY-10 — installed Wi-Fi regulatory completion correction](build/y2linux-m5-connectivity-10-deployment.md) | Historical build/deployment receipt |
| [First manual Production Storage v1 deployment](build/y2linux-production-v1-deployment.md) | Historical build/deployment receipt |
| [Production Storage v1 — Storage02 manual retry](build/y2linux-production-v1-r2-deployment.md) | Historical build/deployment receipt |
| [Production Storage v1 — Storage03 manual correction candidate](build/y2linux-production-v1-r3-deployment.md) | Historical build/deployment receipt |
| [Storage05 production BOOTIMG-only correction](build/y2linux-production-v1-r5-deployment.md) | Historical build/deployment receipt |
| [Storage06 internal boot and owner SSH — accepted on hardware](build/y2linux-production-v1-r6-deployment.md) | Historical build/deployment receipt |
| [Storage02 DIAG01 deployment](build/y2linux-storage-diag-01-deployment.md) | Historical build/deployment receipt |

## Hardware Evidence

| Page | Scope |
| --- | --- |
| [Persistent audio tools — physical SD installation](hardware-evidence/2026-09-10-audio-tools-sd/README.md) | Preserved raw/reviewed evidence |
| [DEV-02 physical live evidence, 2026-09-10](hardware-evidence/2026-09-10-dev02-live/README.md) | Preserved raw/reviewed evidence |
| [M3-AUDIO-01 live entry and first playback](hardware-evidence/2026-09-10-m3-audio-01-live/README.md) | Preserved raw/reviewed evidence |
| [AUDIO-02 first controlled headphone playback](hardware-evidence/2026-09-10-m3-audio-02-playback/README.md) | Preserved raw/reviewed evidence |
| [M3 entry evidence, 2026-09-10](hardware-evidence/2026-09-10-m3-entry/README.md) | Preserved raw/reviewed evidence |
| [M4 entry, read-only live Storage06](hardware-evidence/2026-09-13-m4-baseline/README.md) | Preserved raw/reviewed evidence |
| [Production entry, 2026-09-13](hardware-evidence/2026-09-13-production-entry/README.md) | Preserved raw/reviewed evidence |
| [Owner first storage flash failure — 2026-09-13](hardware-evidence/2026-09-13-storage-flash-failure/README.md) | Preserved raw/reviewed evidence |
| [No-SD console photo](hardware-evidence/2026-09-13-storage02-owner-flash/no-sd-console.md) | Preserved raw/reviewed evidence |
| [Storage02 owner flash and power-on report — 2026-09-13](hardware-evidence/2026-09-13-storage02-owner-flash/README.md) | Preserved raw/reviewed evidence |
| [Storage02 rescue console with SD present](hardware-evidence/2026-09-13-storage02-owner-flash/rescue-console.md) | Preserved raw/reviewed evidence |
| [Storage03 owner result](hardware-evidence/2026-09-13-storage03-owner/README.md) | Preserved raw/reviewed evidence |
| [Storage04 host inspection — 2026-09-13](hardware-evidence/2026-09-13-storage04-live/README.md) | Preserved raw/reviewed evidence |
| [Storage05 owner boot result — failed internal discovery](hardware-evidence/2026-09-13-storage05-owner/README.md) | Preserved raw/reviewed evidence |
| [Storage06 owner boot result](hardware-evidence/2026-09-13-storage06-owner/README.md) | Preserved raw/reviewed evidence |
| [M4-POWER-02: post-reinstall charging fault](hardware-evidence/2026-09-14-m4-power02-charge-fault/README.md) | Preserved raw/reviewed evidence |
| [Fresh M5 entry: POWER-02 charging fault remains a prerequisite failure](hardware-evidence/2026-09-15-m5-entry/README.md) | Preserved raw/reviewed evidence |
| [POWER-02 USB reconnect workaround](hardware-evidence/2026-09-15-usb-reconnect/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-01: first inspection after owner installation](hardware-evidence/2026-09-16-m5-connectivity01/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-02: physical inspection after owner installation](hardware-evidence/2026-09-16-m5-connectivity02/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-03: MD completion followed by kernel crash](hardware-evidence/2026-09-16-m5-connectivity03/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-04: stable Linux, first WMT command times out](hardware-evidence/2026-09-16-m5-connectivity04/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-05: two-byte DMA tail stalls initial WMT command](hardware-evidence/2026-09-16-m5-connectivity05/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-08: RF result fixed; native BlueZ reached with scoped E2 quirk](hardware-evidence/2026-09-17-m5-connectivity08/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-09: standard BlueZ verified on normal boot](hardware-evidence/2026-09-17-m5-connectivity09/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-10: Wi-Fi registration and scanning verified](hardware-evidence/2026-09-17-m5-connectivity10/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-07: exact post-identification EPROTO](hardware-evidence/2026-09-17-m5-protocol/README.md) | Preserved raw/reviewed evidence |
| [CONNECTIVITY-09 Wi-Fi: regulatory OID completion failure](hardware-evidence/2026-09-17-m5-wifi/README.md) | Preserved raw/reviewed evidence |
| [GPU clock and stock-bin contract, before GPU-01](hardware-evidence/2026-09-18-gpu-contract/README.md) | Preserved raw/reviewed evidence |
| [GPU-01: accelerated graphics pass; suspend qualification blocked](hardware-evidence/2026-09-18-gpu01/README.md) | Preserved raw/reviewed evidence |

## Knowledge

| Page | Scope |
| --- | --- |
| [Audio and FM evidence](knowledge/audio-path.md) | Retained research / scoped evidence |
| [Persistent ALSA development tools on Y2ROOT](knowledge/audio-tools-sd.md) | Retained research / scoped evidence |
| [Stock boot structure and loader handoff](knowledge/boot-chain.md) | Retained research / scoped evidence |
| [Historical exact-backup runbook and current recovery route](knowledge/bootimg-recovery-runbook.md) | Retained research / scoped evidence |
| [Candidate and fallback identity index](knowledge/candidate-index.md) | Current artifact index |
| [DEV-01 practical RAM reconciliation](knowledge/development-memory.md) | Retained research / scoped evidence |
| [Display and input evidence](knowledge/display-input.md) | Retained research / scoped evidence |
| [Y2 donor audit and Linux 6.18 integration decision](knowledge/donor-audit.md) | Retained research / scoped evidence |
| [Evidence provenance and review](knowledge/evidence-index.md) | Retained research / scoped evidence |
| [First-device launch gates after the M1 offline wave](knowledge/first-boot-launch-gates.md) | Retained research / scoped evidence |
| [Y2B-240 authorized preflight — 2026-09-08](knowledge/first-experiment-preflight.md) | Retained research / scoped evidence |
| [Y2B-240 result — Linux PID1 evidence and Android recovery](knowledge/first-experiment-result.md) | Retained research / scoped evidence |
| [Y2B-240 — One risk-accepted BOOTIMG-only diagnostic experiment](knowledge/first-experiment.md) | Retained research / scoped evidence |
| [Y2 production GPU platform](knowledge/gpu-platform.md) | Retained research / scoped evidence |
| [Hardware Final: stock CPU DVFS admission and qualification](knowledge/hardware-final-dvfs.md) | Retained research / scoped evidence |
| [Hardware Final idle source boundary](knowledge/hardware-final-idle-evidence.md) | Retained research / scoped evidence |
| [Hardware Final power evidence and remaining physical gates](knowledge/hardware-final-power-evidence.md) | Retained research / scoped evidence |
| [Hardware Final: ECM request alignment and Inventra DMA](knowledge/hardware-final-usb-dma-alignment.md) | Retained research / scoped evidence |
| [Initial Linux RAM policy](knowledge/initial-ram-map.md) | Retained research / scoped evidence |
| [First BOOTIMG-only experiment: launch readiness](knowledge/launch-readiness.md) | Retained research / scoped evidence |
| [Linux 6.18 boot-fundamentals audit](knowledge/linux-6.18-support.md) | Retained research / scoped evidence |
| [M1 — Continued native PID1 execution on the physical Y2](knowledge/m1-runtime-hardware-result.md) | Retained research / scoped evidence |
| [M2 integrated baseline — physical results, 2026-09-10](knowledge/m2-baseline-hardware-result.md) | Retained research / scoped evidence |
| [M2-CHRDET-01 physical result](knowledge/m2-chrdet-hardware-result.md) | Retained research / scoped evidence |
| [M2-CHRDET-01 — PMIC charger-presence observation](knowledge/m2-chrdet-probe.md) | Retained research / scoped evidence |
| [M2-PHYWAKE-01 physical result](knowledge/m2-phy-wake-hardware-result.md) | Retained research / scoped evidence |
| [M2-PHYWAKE-01 — bounded suspend-force release](knowledge/m2-phy-wake-probe.md) | Retained research / scoped evidence |
| [M2-PWRAP-01 physical result](knowledge/m2-pwrap-hardware-result.md) | Retained research / scoped evidence |
| [M2 PWRAP/VUSB prerequisite — bounded read-only PMIC probe](knowledge/m2-pwrap-probe.md) | Retained research / scoped evidence |
| [M2-USBCLK-01 physical result](knowledge/m2-usb-clock-hardware-result.md) | Retained research / scoped evidence |
| [M2-USBCLK-01 — inherited USB clock state](knowledge/m2-usb-clock-probe.md) | Retained research / scoped evidence |
| [M2-USBACM-01 — guarded first controller ownership and logging](knowledge/m2-usb-enumeration.md) | Retained research / scoped evidence |
| [M2-USBGUARD-01 — expose the refused USB handoff](knowledge/m2-usb-guard-diagnostic.md) | Retained research / scoped evidence |
| [M2-USBSTATE-01 physical result](knowledge/m2-usb-state-hardware-result.md) | Retained research / scoped evidence |
| [M2-USBSTATE-01 — inherited USB state, read only](knowledge/m2-usb-state-probe.md) | Retained research / scoped evidence |
| [M2 USB ACM hardware results](knowledge/m2-usbacm-hardware-result.md) | Retained research / scoped evidence |
| [Native audio physical result — AUDIO-02 supersedes AUDIO-01](knowledge/m3-audio-01-live-result.md) | Retained research / scoped evidence |
| [M3 headphone-first architecture and source review](knowledge/m3-audio-architecture.md) | Retained research / scoped evidence |
| [M4 charging prerequisite: this Y2's battery sensor](knowledge/m4-battery-acquisition.md) | Retained research / scoped evidence |
| [M4 production charging contract](knowledge/m4-charging.md) | Retained research / scoped evidence |
| [M4 end-user power completion](knowledge/m4-end-user-power.md) | Retained research / scoped evidence |
| [M4 power platform](knowledge/m4-power-platform.md) | Retained research / scoped evidence |
| [M5 connectivity entry audit — 2026-09-15](knowledge/m5-connectivity-entry.md) | Retained research / scoped evidence |
| [M5 production connectivity candidate](knowledge/m5-connectivity-implementation.md) | Retained research / scoped evidence |
| [CONNECTIVITY-02: correction of the first startup failure](knowledge/m5-connectivity02-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-03: factory-path normalization](knowledge/m5-connectivity03-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-04: verify the real CONN EMI remap before startup](knowledge/m5-connectivity04-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-05: frame the first WMT command over BTIF](knowledge/m5-connectivity05-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-06: rearm APDMA TX completion for each transfer](knowledge/m5-connectivity06-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-07: identify factory storage through mounted root/data](knowledge/m5-connectivity07-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-08: consume the MT6582 E2 RF calibration result](knowledge/m5-connectivity08-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-09: MT6582 E2 local extended feature page limit](knowledge/m5-connectivity09-corrections.md) | Retained research / scoped evidence |
| [CONNECTIVITY-10: complete Wi-Fi regulatory TX power requests](knowledge/m5-connectivity10-corrections.md) | Retained research / scoped evidence |
| [Y2 first-boot observation path](knowledge/observation-path.md) | Retained research / scoped evidence |
| [D14 — Self-observable initramfs diagnostics](knowledge/on-screen-diagnostics.md) | Retained research / scoped evidence |
| [Unknowns and architecture decisions](knowledge/open-unknowns.md) | Retained research / scoped evidence |
| [Partition map and acquisition boundary](knowledge/partition-map.md) | Retained research / scoped evidence |
| [Platform v1 source and hardware boundaries](knowledge/platform-v1-hardware-gates.md) | Retained research / scoped evidence |
| [Power evidence and reporting discrepancy](knowledge/power.md) | Retained research / scoped evidence |
| [Recovery baseline](knowledge/recovery.md) | Retained research / scoped evidence |
| [Additional Y2 reverse-engineering evidence](knowledge/reverse-engineering-audit.md) | Retained research / scoped evidence |
| [D11 — Risk-accepted first diagnostic boot policy](knowledge/risk-accepted-diagnostic.md) | Retained research / scoped evidence |
| [Current non-root system baseline](knowledge/stock-system-baseline.md) | Retained research / scoped evidence |
| [Storage03 correction candidate](knowledge/storage03-corrections.md) | Retained research / scoped evidence |
| [Storage04: one production platform](knowledge/storage04-production-platform.md) | Retained research / scoped evidence |
| [Storage05 integrated production MMC correction](knowledge/storage05-mmc-correction.md) | Retained research / scoped evidence |
| [Production stock eMMC address compatibility](knowledge/storage06-addressing-correction.md) | Retained research / scoped evidence |
| [Hardware Final USB DMA observations](knowledge/usb-dma-observation.md) | Retained research / scoped evidence |
| [Host-readable USB logging — minimal path and controller gate](knowledge/usb-logging.md) | Retained research / scoped evidence |
| [Wi-Fi, Bluetooth and firmware evidence](knowledge/wifi-bluetooth.md) | Retained research / scoped evidence |
| [Y2 hardware evidence baseline](knowledge/y2-hardware.md) | Retained research / scoped evidence |
| [Y2B-245 — Real screen result and sleep failure](knowledge/y2b245-hardware-result.md) | Retained research / scoped evidence |
| [DEV-02 live platform qualification — 2026-09-10](knowledge/y2linux-dev02-live-qualification.md) | Retained research / scoped evidence |
| [Y2Linux development hardware results](knowledge/y2linux-development-hardware-result.md) | Retained research / scoped evidence |

## Planning

| Page | Scope |
| --- | --- |
| [Production Storage / Installation v1 — issue 33](planning/issues/production-storage-v1.md) | Historical issue specification |
| [Y2A-300 — M3 current state, 2026-09-10](planning/issues/Y2A-300.md) | Historical issue specification |
| [Y2B-201 — Pin and reproduce the ARMv7 build environment](planning/issues/Y2B-201.md) | Historical issue specification |
| [Y2B-205 — Enforce D08 layout before packaging](planning/issues/Y2B-205.md) | Historical issue specification |
| [Y2B-210 — Select the CPU0-only diagnostic kernel configuration](planning/issues/Y2B-210.md) | Historical issue specification |
| [Y2B-215 — Describe the researched CPU0 first-boot hardware](planning/issues/Y2B-215.md) | Historical issue specification |
| [Y2B-220 — Build a tiny diagnostic initramfs](planning/issues/Y2B-220.md) | Historical issue specification |
| [Y2B-225 — Build and validate zImage with appended DTB](planning/issues/Y2B-225.md) | Historical issue specification |
| [Y2B-230 — Package the legacy MTK BOOTIMG offline](planning/issues/Y2B-230.md) | Historical issue specification |
| [Y2B-235 — Prove offline reproducibility and publish the launch-gated result](planning/issues/Y2B-235.md) | Historical issue specification |
| [Y2B-240 — One risk-accepted BOOTIMG-only diagnostic experiment](planning/issues/Y2B-240.md) | Historical issue specification |
| [Y2B-245 — Useful on-screen Linux diagnostic environment](planning/issues/Y2B-245.md) | Historical issue specification |
| [Y2B-250 — Restore ARM time32 nanosleep for the diagnostic heartbeat](planning/issues/Y2B-250.md) | Historical issue specification |
| [Y2B-255 — Add bounded USB CDC ACM kernel and PID1 logging](planning/issues/Y2B-255.md) | Historical issue specification |
| [Y2E-101 — Inventory existing stock artifacts and establish evidence provenance](planning/issues/Y2E-101.md) | Historical issue specification |
| [Y2E-105 — Capture the current non-root ADB system and access baseline](planning/issues/Y2E-105.md) | Historical issue specification |
| [Y2E-110 — Reconcile stock scatter and runtime partition metadata](planning/issues/Y2E-110.md) | Historical issue specification |
| [Y2E-115 — Inspect stock boot structure and config/device-tree availability offline](planning/issues/Y2E-115.md) | Historical issue specification |
| [Y2E-120 — Assess recovery capability and propose the next safe acquisition boundary](planning/issues/Y2E-120.md) | Historical issue specification |
| [Y2E-125 — Stock loader handoff and Linux 6.18 strategy](planning/issues/Y2E-125.md) | Historical issue specification |
| [Y2E-130 — Establish the safe initial Linux RAM map](planning/issues/Y2E-130.md) | Historical issue specification |
| [Y2E-140 — Establish a reliable first-boot observation path](planning/issues/Y2E-140.md) | Historical issue specification |
| [Y2E-145 — Assess authentication, recovery and bounded handoff launch gates](planning/issues/Y2E-145.md) | Historical issue specification |
| [Y2E-155 — Specify one safe static RAM expansion step](planning/issues/Y2E-155.md) | Historical issue specification |
| [Y2E-160 — Establish the MT6323 power and supply prerequisite map](planning/issues/Y2E-160.md) | Historical issue specification |
| [Y2E-165 — Resolve the wheel and select-button event path](planning/issues/Y2E-165.md) | Historical issue specification |
| [Y2E-170 — Specify a safe display handoff and backlight ownership boundary](planning/issues/Y2E-170.md) | Historical issue specification |
| [Y2E-175 — Specify a read-only removable-SD controller proof](planning/issues/Y2E-175.md) | Historical issue specification |
| [Y2H-300 — M2 core platform foundations and boot stability](planning/issues/Y2H-300.md) | Historical issue specification |
| [Y2N-500 — M5 connectivity, firmware and conditional FM coverage](planning/issues/Y2N-500.md) | Historical issue specification |
| [Y2P-400 — M4 end-user production power platform](planning/issues/Y2P-400.md) | Historical issue specification |
| [Y2R-600 — M6 recoverable production userspace, updates and security](planning/issues/Y2R-600.md) | Historical issue specification |
| [M0 — Evidence & Recovery Baseline](planning/M0-evidence-and-recovery.md) | Decision log / historical roadmap |
| [M1 — Linux 6.18 First Boot](planning/M1-first-boot.md) | Decision log / historical roadmap |
| [Whole-platform evidence ledger — 2026-09-23](planning/platform-review-evidence-2026-09-23.md) | Decision log / historical roadmap |
| [Platform v1 software completion roadmap](planning/platform-v1-roadmap.md) | Decision log / historical roadmap |
| [Y2Linux roadmap and gap audit](planning/roadmap-gap-audit.md) | Decision log / historical roadmap |

## Validation

| Page | Scope |
| --- | --- |
| [Platform v1 capability report](validation/PLATFORM-V1-CAPABILITY-REPORT.md) | Scoped validation record |
| [Platform v1 completion ledger](validation/PLATFORM-V1-COMPLETION.md) | Scoped validation record |
| [Hardware 02 — coherent hardware ceiling candidate](validation/PLATFORM-V1-HARDWARE-02.md) | Scoped validation record |
| [Y2 hardware capability ceiling campaign](validation/PLATFORM-V1-HARDWARE-CEILING.md) | Scoped validation record |
| [Platform v1 owner qualification](validation/PLATFORM-V1-OWNER-QUALIFICATION.md) | Scoped validation record |
| [Platform v1 physical bring-up campaign](validation/PLATFORM-V1-PHYSICAL-BRINGUP.md) | Scoped validation record |
| [Platform v1 physical master issues](validation/PLATFORM-V1-PHYSICAL-ISSUES.md) | Scoped validation record |
| [Platform v1 physical qualification](validation/PLATFORM-V1-PHYSICAL-QUALIFICATION.md) | Scoped validation record |
| [Platform v1 telemetry repair — 2026-09-25](validation/PLATFORM-V1-TELEMETRY-01.md) | Scoped validation record |
| [CPU Final Fix 03 implementation and candidate receipt](validation/Y2-CPU-FINAL-FIX03.md) | Current software candidate receipt |
| [CPU Final Fix03 source and evidence ledger](validation/Y2-CPU-FINAL-FIX03-SOURCES.md) | Scoped validation record |
| [Y2 CPU Final Fix03 physical qualification — prepared, not run](validation/Y2-CPU-FINAL-FIX03-PHYSICAL-QUALIFICATION.md) | Prepared owner run; no hardware result |
| [CPU Final Fix 02 implementation and candidate receipt](validation/Y2-CPU-FINAL-FIX02.md) | Scoped validation record (flashed and physically qualified 2026-09-29) |
| [CPU Final Fix02 source and binary ledger](validation/Y2-CPU-FINAL-FIX02-SOURCES.md) | Scoped validation record |
| [Y2 CPU Final Fix02 physical qualification — 2026-09-29](validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md) | Latest CPU physical record: awake CPU qualified; SLIDLE, full suspend and loaded USB FAIL |
| [Y2 CPU Final Fix01 physical qualification — 2026-09-27](validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md) | Scoped validation record |
| [CPU Final Fix01 source and binary ledger](validation/Y2-CPU-FINAL-FIX01-SOURCES.md) | Scoped validation record |
| [CPU Final Fix 01 implementation and candidate receipt](validation/Y2-CPU-FINAL-FIX01.md) | Scoped validation record |
| [Y2 CPU Final physical qualification](validation/Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md) | Scoped validation record |
| [CPU Final source and register provenance](validation/Y2-CPU-FINAL-SOURCES.md) | Scoped validation record |
| [Y2 CPU Final](validation/Y2-CPU-FINAL.md) | Scoped validation record |
| [CPU Final Fix01 physical capability ledger](validation/Y2-HARDWARE-FINAL-CAPABILITIES.md) | Current observed ledger / historical inventories |
| [Hardware Final implementation-first continuation, 2026-09-27](validation/Y2-HARDWARE-FINAL-HANDOFF.md) | Scoped validation record |
| [Y2 Hardware Final physical qualification](validation/Y2-HARDWARE-FINAL-QUALIFICATION.md) | Scoped validation record |
| [Hardware Final radio and audio source work](validation/Y2-HARDWARE-FINAL-RADIO-AUDIO.md) | Scoped validation record |
| [Y2 Hardware Final implementation candidate](validation/Y2-HARDWARE-FINAL.md) | Scoped validation record |

## Tests

| Page | Scope |
| --- | --- |
| [Y2Linux host test profiles](../tests/README.md) | Test reference |
