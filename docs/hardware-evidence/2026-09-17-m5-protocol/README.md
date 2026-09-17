# CONNECTIVITY-07: exact post-identification EPROTO

2026-09-17 UTC, source checkpoint `359f7870859c546290d3a56dcdc601585f05d66f`.
Strictly pinned owner-key SSH matches the retained September 14/15 host key.
Live kernel is `6.18.0-y2linux-m5-connectivity-07`. No BOOTIMG readback hash
is claimed. The initial observation is at uptime 326.11 seconds; the stopped
RX snapshot is at 326.568 seconds on the same boot.

Factory/MD completion and BTIF/AP_DMA operation are retained facts, not rerun
workstreams. Startup identifies `6582 / HVR 8a01 / FVR 8a00` at 68.308840 s,
then fails with `-71` at 68.935840 s. `hci0` exists in sysfs but BlueZ lists no
adapter; no `wlan0`/cfg80211 interface exists. This is not usable HCI registration.

## Exact wire boundary

The driver leaves its coherent RX history allocated after stopping transport.
A one-shot module compiled against the exact CONNECTIVITY-07 build reads only
that software buffer under the lifecycle mutex, requiring this controller,
zero functions, and power/transport/DMA all stopped. It performs no MMIO,
controller command, factory read or protected write. Initialization returns
`-ECANCELED` intentionally so no module/callback remains, including on this
kernel without module unload. Private source/header copies and raw captures
are in `evidence-private/20260917-m5-protocol/`. Loading an external diagnostic
may mark the running kernel tainted; it is not part of the candidate image.

[Sanitized frame metadata](rx-frames.json) records 125 frames / 1636 RX bytes:

- Three register replies have correct ordinary inner length 12.
- Default STP query, options set, and full-mode query all succeed.
- Both patch-address command pairs succeed. Patch 1 receives 18 successful
  fragment replies, patch 2 receives 34. Each ends with a successful reset.
- Command `01 14 01 00 01` then returns STP header `9b 42 76 53`, WMT prefix
  `02 14 72 02 00 01`: channel 4, seq/ACK 3, outer payload 630, inner length
  626, status zero, operation 1. It includes 624 additional RF-result bytes.
  Header checksum and full-payload CRC both validate.
- Native `stp.c:deliver()` rejects the 630-byte event against its 256-byte
  response buffer, before WMT completion. The six-byte expected-event check
  would also reject it if only the buffer limit were increased.

All captured full-mode header checksums/CRCs pass; the sequence crosses modulo-8
boundaries normally. This is a host response-shape rejection after successful
patch download/reset and after the controller reports RF calibration success.
It is before antenna/co-clock configuration, radio function enable, successful
HCI setup/BlueZ exposure and Wi-Fi bring-up.

The donor maps HVR `8a01` to E2 while retaining the E1 patch family. Its
`wmt_core_init_script()` explicitly skips comparison of opcode `0x14` RF
calibration events. This corroborates a variable RF-result event, but does
not justify ignoring error status or accepting arbitrary lengths/controllers.
No patch-family switch, reordering, global checksum relaxation, factory-data
change, DMA fix or M4 change follows from this observation.

M5 #31 remains ACTIVE / PROTOCOL CORRECTION / PHYSICALLY UNQUALIFIED.
The [CONNECTIVITY-08 correction](../../knowledge/m5-connectivity08-corrections.md)
needs a new BOOTIMG followed by live interface checks. No milestone boundary,
memory-layout expansion, GPU/Reborn work or broad qualification occurred.
