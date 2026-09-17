# CONNECTIVITY-10: complete Wi-Fi regulatory TX power requests

The own Y2 trace places the remaining -09 Wi-Fi failure after firmware ready,
successful RF/baseband capability queries and the interface registration path.
The regulatory notifier sends domain `0x13` and TX power `0x38` commands; TX
credits return, but the power-setting OID times out and triggers recovery.
[Physical evidence](../hardware-evidence/2026-09-17-m5-wifi/README.md).

`wlanoidSetTxPower` marks `0x38` as an OID set without a firmware response and
provides no done callback. `wlanProcessCommandQueue` completes successful
no-response commands through that callback, then frees them. Consequently the
waiting `kalIoctl` never receives success, although transmission succeeded.

Attach existing `nicCmdEventSetCommon` and `nicOidCmdTimeoutCommon` callbacks
only to this producer. This repairs the driver completion contract; it adds no
silicon workaround and requests no unsupported firmware acknowledgement.
The payload, cfg80211 channel/power policy, command queue, transport and error
checks stay unchanged. Two small lifecycle logs expose interface registration
and regulatory failure without enabling the verbose legacy debug masks.

The new regression executes the actual producer, queue consumer, queue macros
and callbacks, reproduces the missing completion, and checks successful TX,
TX error, credit starvation/retry, timeout and non-OID behavior. Replaying the
unmodified -09 producer fails its successful-completion assertion. All 16
targeted Wi-Fi/connectivity checks pass using the current emitted -09 DT.

Build a normal CONNECTIVITY-10 BOOTIMG retaining CONNECTIVITY-07 root/data and
the verified CONNECTIVITY-09 Bluetooth-capable BOOTIMG as fallback. Installation
remains manual. Verify a stable wlan0/cfg80211 interface and regulatory setup
after installation, then continue bounded scan/connect testing as available.
This source fix is not a claim of physical -10 success or M5 qualification.
