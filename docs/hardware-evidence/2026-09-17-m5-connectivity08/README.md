# CONNECTIVITY-08: RF result fixed; native BlueZ reached with scoped E2 quirk

2026-09-17 UTC. The owner confirms manual installation; strictly pinned SSH
independently reports `6.18.0-y2linux-m5-connectivity-08`. Built source `8b18ca3`,
BOOTIMG identity in the [deployment receipt](../../build/y2linux-m5-connectivity-08-deployment.md).
No physical BOOTIMG readback hash is claimed. Observations below share one boot.
Raw HCI captures, device addresses and diagnostic source stay private under
`evidence-private/20260917-m5-protocol/`.

## Original EPROTO is physically cleared

The normal boot reports full STP mode at 51.953664 s, patch 1/reset at
52.060458 s, patch 2/reset at 52.254080 s, and successful RF calibration with
`event=630 bytes` at 52.563980 s. No startup EPROTO, transport error or recovery
occurs before function initialization. Vendor HCI setup proceeds at 53.063121 s.
Factory/MD, DMA/IRQ and identification research are not repeated.

At 53.143717 s HCI initialization fails later: `Opcode 0x1004 failed: -38`.
The hci0 object exists, but BlueZ has no adapter. This is a new feature-page
failure after shared startup, function enable and vendor HCI initialization.

## Exact next failure and controlled correction

A `btmon` trace of a retry shows:

| Request/result | Observed value |
| --- | --- |
| Read Local Version | Bluetooth 4.0 (`06`), MediaTek manufacturer 70 |
| Read Local Extended Features `0x1004`, page 1 | Success `00`, reported maximum page 2 |
| Read Local Extended Features `0x1004`, page 2 | Status `30`, parameter out of range |
| Linux errno for status `30` | `-ENOSYS` (`-38`) |

The existing donor `bt/stp_hci.c` documents this exact E2 defect and sets
`HCI_QUIRK_BROKEN_LOCAL_EXT_FEATURES_PAGE_2`. The native driver omitted it.
The installed Linux 6.18 handler uses that quirk to prevent increasing the page
limit, but it does not repair a maximum already cached during failed setup.

A small module built against the exact installed -08 kernel verifies the
controller triple `6582/8a01/8a00`, stopped transport/functions, zero core error,
and an idle HCI device still in SETUP. Under the HCI request and core lifecycle
locks, it can apply only this standard quirk and set the verified maximum to 1.
It queues the existing kernel HCI setup worker, then returns `-ECANCELED` so
there is no resident module or callback. No code patch, MMIO, factory write or
EDR/LE feature/command masking is used. Loading it taints this diagnostic boot
as an external module; the production candidate contains no such module.

First retry with no change reproduces page-2 status `30`. Setting the quirk
alone on the already-failed HCI object still fails because `max_page=2` is
cached. Applying the quirk with maximum 1 at 445.696 s succeeds:
`Bluetooth: MGMT ver 1.23` at 446.911397 s, and `bluetoothctl list/show` exposes
the standard adapter. The successful capture reads page 1 and never page 2.

The existing `y2-radio bluetooth on-runtime` then reports **Changing power on
succeeded**, BlueZ **Powered: yes**, core `powered=1 functions=0x1 error=0`.
This is actual standard HCI/BlueZ availability, beyond a sysfs object. BR/EDR and
LE capabilities remain reported without masking. Pairing, discovery, EDR data,
BLE interoperability and A2DP audio are not claimed as tested.

Bluetooth is returned off with its runtime-only control; the BlueZ adapter
remains visible. Both saved preferences stay 0. The temporary quirk remains in
this HCI instance until reboot; it is not yet in the installed -08 BOOTIMG.

## Wi-Fi and scope limits

One runtime-only Wi-Fi unblock reaches WMT function enable (`functions=0x8`)
and the fullmac log `send Wi-Fi Start command`, but no wlan0 is registered.
It triggers four EIO reports and three bounded automatic recoveries. Wi-Fi is
returned off; one idle-core recovery clears the latched error before the HCI
experiment. Counters stay `transport_errors=4 recoveries=4` throughout the
subsequent successful Bluetooth test. These counters include Wi-Fi errors and
do not establish renewed STP framing failure. The precise Wi-Fi post-start
failure remains open; no speculative Wi-Fi/DMA/global fix is added.

USB SSH and the same boot remain responsive, with no observed Oops/panic.
PC charging reports fault 0 in the captures. This is not a fresh complete M4
qualification. No flash, protected write, source/ROM audit, GPU/Reborn work or
milestone closure occurs. M5 #31 stays ACTIVE / BLUETOOTH ADAPTER REACHED /
WI-FI STARTUP FAILURE / BROADER RADIO QUALIFICATION OPEN.

[CONNECTIVITY-09](../../knowledge/m5-connectivity09-corrections.md) makes this
physically proven, controller-specific HCI correction permanent in setup.
