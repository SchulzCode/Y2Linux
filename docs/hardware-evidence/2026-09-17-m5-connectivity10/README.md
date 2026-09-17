# CONNECTIVITY-10: Wi-Fi registration and scanning verified

2026-09-17 UTC. The owner reports installation; strictly pinned SSH confirms
`6.18.0-y2linux-m5-connectivity-10`. Build identities are in the
[deployment receipt](../../build/y2linux-m5-connectivity-10-deployment.md).
No physical BOOTIMG readback hash is claimed. All observations share one normal
boot, with kernel taint 0 and no diagnostic module or runtime correction.

## Wi-Fi result

`y2-radio wifi on-runtime` creates **wlan0 through cfg80211**, with registration
logged at 105.696688 s. The normal userspace service attaches wpa_supplicant,
which reports `DISCONNECTED` because no networks are configured. After eight
seconds the interface remains present with core `functions=0x8 error=0
transport_errors=0 recoveries=0`. The previous regulatory OID timeout is gone.

`iw dev wlan0 scan` exits successfully and returns **23 BSS entries** in
2412–2462 MHz. A runtime disable removes wlan0 and powers down the shared core
cleanly. Re-enabling registers wlan0 again at 200.297193 s and the interface
remains available without recovery.

Standard BlueZ power-on succeeds while Wi-Fi is active, with both adapters
visible and core `functions=0x9`. A second Wi-Fi scan succeeds and returns
**12 BSS entries**. Core error, transport-error and recovery counts remain 0
throughout. This is an interface/power/scan check, not sustained radio/audio
coexistence qualification.

Bluetooth is then disabled through BlueZ; Wi-Fi remains available with
`functions=0x8`. Wi-Fi is disabled last, leaving core `powered=0 functions=0x0
error=0 transport_errors=0 recoveries=0`. The BlueZ adapter remains visible
and powered off. Both saved preferences remain 0. USB SSH and the same boot
remain responsive, with no observed Oops/panic.

## Fix and limits

The [exact -09 failure](../2026-09-17-m5-wifi/README.md) was the missing
completion callback on Set TX Power (`0x38`) during cfg80211 regulatory setup.
The standard callback now completes the no-response OID after successful
transmission. Firmware Start, calibration, channel/power bytes and the existing
Bluetooth fixes are preserved. The installed result confirms the correction.

No network blocks are saved in wpa_supplicant's configuration. Association,
authentication, DHCP, DNS and data transfer are therefore untested. Pairing,
A2DP, BLE/EDR data and sustained coexistence also remain open. M4 code, root/data
and memory/storage layout are unchanged; no broad M4 requalification occurs.
M5 #31 remains ACTIVE / WI-FI AND BLUETOOTH ADAPTERS VERIFIED / CONNECTION AND
RADIO QUALIFICATION OPEN. GPU/Reborn remain out of scope.

[Sanitized machine-readable observations](adapter-result.json) are checked
against six private interface/scan captures plus BlueZ control/final-state
captures under `evidence-private/20260917-m5-wifi/`. Raw SSIDs, BSSIDs and device
addresses remain private.
