# Wi-Fi, Bluetooth and firmware evidence

> 2026-09-10: [additional research](reverse-engineering-audit.md) supplies native
> cold-start/A2DP records with complete modem calibration followed by MD1 off.
> It also records failed EDR remedies and a Basic Rate workaround. Validate our
> own revision and calibration; never adopt another player's fixture. M5 remains
> deferred, and FM reception is excluded for the owner's older board.

Date: 2026-09-08. Related tasks: Y2E-101/105/115. No radio scan, enable, pair, connect or firmware-loading action was performed.

CONFIRMED current metadata: `/system/etc/firmware` lists several WIFI_RAM_CODE variants, WMT_SOC.cfg, MT6572/82 patch blobs and an mt6627 directory. `/system/lib/modules/wlan.ko` is a symlink naming wlan_consys_mt6582.ko; target availability/loading is not established by that listing. `/proc/modules` is empty. Firmware presence does not prove which blob is selected, which transport is used, or whether a radio is populated and operational.

CONFIRMED static ramdisk evidence: WMT/connection loader declarations and STP-related node setup; a ttyMT2 connectivity comment. These identify a vendor ecosystem, not a standard Linux HCI transport contract or safe diagnostic UART. Current `ro.hardware=mt6582` is observed; reconcile it with older Bluetooth HAL-selection inferences instead of copying them.

The early `BLUETOOTH_STACK_INVESTIGATION.md` describes BlueAngel artifacts and originally lacked runtime evidence. Later archived route observations have different scope. Neither justifies promising BlueZ/PipeWire compatibility or specific negotiated codecs on native Linux.

UNKNOWN: exact controller revision, bus/transport, power/reset/clock sequence, required firmware/module hashes and order, calibration storage/ownership, MAC persistence and standard-kernel driver applicability. Keep addresses, bonds and network credentials private. Next proof is a bounded driver/firmware identity inventory with no radio state changes.
