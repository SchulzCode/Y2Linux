# CONNECTIVITY-09: standard BlueZ verified on normal boot

2026-09-17 UTC. The owner confirms manual installation. Strictly pinned SSH
reports `6.18.0-y2linux-m5-connectivity-09`; source `f55fff4` and BOOTIMG identity
are recorded in the [deployment receipt](../../build/y2linux-m5-connectivity-09-deployment.md).
No physical BOOTIMG readback hash is claimed. These observations share one
normal boot, through uptime 262.67 seconds. No diagnostic module or runtime
correction is applied; kernel taint is 0 and the only loaded module is
`mediatek_drm`.

## Normal startup

| Stage | Uptime (s) | Result |
| --- | ---: | --- |
| Full STP mode | 52.773595 | Verified |
| Patch 1 and reset | 52.880290 | Acknowledged |
| Patch 2 and reset | 53.073883 | Acknowledged |
| RF calibration | 53.383731 | Successful 630-byte event |
| HCI extended-feature handling | 53.939164 | Existing page-2 quirk active |
| Bluetooth management | 53.966589 | MGMT 1.23 |

`/sys/class/bluetooth/hci0` and the adapter in `bluetoothctl list/show` appear
through normal initialization. The `broken local ext features page 2` log
identifies the active Linux quirk; there is no failed `0x1004` command. Core
status reports `calibrated=1 chip=6582 hvr=8a01 fvr=8a00 error=0
transport_errors=0 recoveries=0`. The original RF-result EPROTO and the later
HCI page-2 initialization failure are both cleared in the installed image.

## Standard power cycle

`y2-radio bluetooth on-runtime` reports **Changing power on succeeded**.
BlueZ reports **Powered: yes**, and core status is `powered=1 functions=0x1
error=0 transport_errors=0 recoveries=0`. This power-up repeats both patch
downloads, RF calibration and the scoped HCI correction successfully.

`y2-radio bluetooth off-runtime` then reports **Changing power off succeeded**.
The adapter remains visible with **Powered: no**; core status returns to
`powered=0 functions=0x0 error=0 transport_errors=0 recoveries=0`. Both saved
radio preferences remain 0. Wi-Fi stays soft blocked and no wlan0 exists.
The same boot remains responsive over USB SSH, with no observed Oops/panic.

## Scope and next checkpoint

This verifies standard Bluetooth adapter registration and power control on the
permanent production image. Pairing, discovery, BLE/EDR data, A2DP audio and
coexistence are not tested here. Wi-Fi's last active test remains the
[-08 post-Start EIO failure](../2026-09-17-m5-connectivity08/README.md); it is not
retried during this Bluetooth verification. Its exact next failure remains open.

Factory/DMA/identification research and full M4 qualification are not repeated.
Accepted M4 code, root/data, saved preferences and memory/storage layout remain
unchanged. M5 #31 stays ACTIVE / BLUETOOTH ADAPTER VERIFIED / WI-FI STARTUP
FAILURE / BROADER RADIO QUALIFICATION OPEN. No milestone closure or GPU/Reborn
work occurs.

Sanitized observations are in [adapter-result.json](adapter-result.json).
Raw logs and addresses remain private in `evidence-private/20260917-m5-protocol/`:
`connectivity09-212735.txt`, `connectivity09-boot-status.txt`,
`connectivity09-bluez-powered.txt` and `connectivity09-bluez-restored.txt`.
