# CONNECTIVITY-09 Wi-Fi: regulatory OID completion failure

2026-09-17 UTC, owner-installed `6.18.0-y2linux-m5-connectivity-09`, strictly
pinned USB SSH. Bluetooth normal-boot availability is already verified in the
[-09 receipt](../2026-09-17-m5-connectivity09/README.md). This session investigates
the remaining Wi-Fi failure without repeating factory/DMA/identification work.

## Exact failure

The driver's existing debug masks initially suppress all Wi-Fi diagnostics.
A one-shot module compiled against the exact installed image changes only
those masks, while the shared core/functions are stopped. It checks the kernel
release and known controller, uses the exact build's data-symbol address after
checking live text addresses, and leaves no resident module or callback.
It performs no MMIO, code patch or firmware/calibration write. The diagnostic
boot is tainted as an external module; this is separate from the clean -09
normal-boot verification. Masks are restored after two controlled runtime
unblocks and their bounded automatic recoveries.

The first detailed trace proves firmware download, Start, ready polling and
capability queries succeed. At 692.727721 s the ready bit is asserted; at
692.762736 s the firmware reports `RF CAL FAIL=(0), BB CAL FAIL=(0)`. The
compile-flags query also returns. The failure is later than those stages.

The second trace identifies the request:

| Event | Uptime (s) | Observation |
| --- | ---: | --- |
| Regulatory domain command | 850.102111 | Sequence 92, CID 19 (`0x13`) |
| TX power control command | 850.102132 | Sequence 93, CID 56 (`0x38`) |
| Command TX resource use | 850.102154 / 850.102179 | TC4 credits acquired for both commands |
| TX credit returns | 850.102343–850.102436 | Interrupt/worker path returns resources |
| OID timeout | 852.168991 | Pending OID released after about two seconds |
| Interface teardown | 852.249177 | Registered wireless device unregistered by recovery |

The Wi-Fi IRQ count reaches 29 in the captured interval. No lasting interface
is exposed by `iw dev`. The source chain is `y2_wifi_reg_notifier` → `apply` →
`wlanoidSetTxPower` → `wlanProcessCommandQueue`. The `0x38` command is a set
with no firmware response, marked as an OID but with a NULL done callback.
After a successful TX the command queue frees it without completing the OID.
Its timer wakes the waiter with failure, and the regulatory notifier requests
shared recovery. This explains the EIO counters; they do not indicate a failed
Wi-Fi Start command or new STP framing defect.

## Correction and retained state

[CONNECTIVITY-10](../../knowledge/m5-connectivity10-corrections.md) attaches the
standard success/timeout callbacks to this one command producer. Its wire
format, response expectation, regulatory channels and power bytes are unchanged.
The host regression uses the actual producer, command-queue consumer, queue
macros and callbacks. It reproduces the old missing completion and verifies
success, TX failure, exhausted credits/retry, timeout and non-OID behavior.
Substituting the unmodified -09 producer fails the success-completion assertion.
All 16 targeted connectivity/Wi-Fi checks pass. Corrected physical behavior
still requires owner installation and a new runtime enable check.

Both runtime radios and saved preferences are off after diagnosis. An idle
core recovery clears the latched error: `error=0 transport_errors=4 recoveries=4`.
Standard BlueZ power-on/off succeeds again with those counters unchanged.
The debug masks are restored and the temporary module file is removed. USB
SSH remains available; no full M4 regression or radio interoperability test
is claimed. M4, memory/layout, root/data and the existing Bluetooth fixes remain
unchanged. M5 stays open; GPU/Reborn remain out of scope.

Raw logs, addresses and diagnostic source remain private under
`evidence-private/20260917-m5-wifi/`: `connectivity09-wifi-debug-retry.txt`,
`connectivity09-wifi-command-retry.txt`, `debug-restore.txt`,
`restored-bluetooth-check.txt` and `wifi-debug-module.c`.
