# POWER-02 USB reconnect workaround

2026-09-15, repository `bbbbbf2` plus the scoped startup/recorder changes.
The installed release reports `6.18.0-y2linux-m4-power-02`; no new BOOTIMG
readback, kernel build or flash was performed. Earlier September 14 charging
evidence and unrelated working-tree edits are preserved.

**Result: a tested runtime workaround, not a kernel fix or #27 closure.**
Keeping the existing MUSB child at `power/control=on` avoids the reproduced
failure in two physical reconnects on one boot, including PC → wall charger → PC.
The startup script applies this setting only to POWER-02's Y2 USB controller.

[Selected device records](events.txt), [structured result and raw hashes](result.json).
Full captures remain in ignored `evidence-private/20260915-usb-reconnect/`.

## Reproduced failure with the original policy

A bounded recorder saved cached USB status, charging state and kernel logs to
`/data/logs/usb-charge.Ckk3ce`. The owner removed/reconnected the cable while
keeping the Y2 powered on. The PC logged removal but no new USB device.

| Device uptime | Observed event |
| --- | --- |
| 541.816480 s | USB enters DETACHED, error 0. |
| 547.327276 s | Source detection and USB re-entry succeed; awaiting host configuration. |
| 547.666738 s | USB stops permanently with error -75, IRQ count 1167. |

`-75` is `-EOVERFLOW`, set here by the adapter's guard against more than 512
interrupts in one jiffy. Re-entry reached READY before the interrupt storm;
the PHY admission checks did not reject this cycle. The first offending IRQ
status is not retained, so its exact electrical/register cause remains unknown.
The saved files survived the owner restart and were retrieved through SSH.

## Workaround and physical checks

On the next boot, the MUSB child's policy was `auto`. At uptime 133.65 s the
standard runtime-PM control was changed to `on`, and read back as `on/active`.
No charger limits or protection settings were changed.

| Cycle | Detach | Re-entry | Configured again | Result |
| --- | --- | --- | --- | --- |
| PC → unplug → PC | 207.293995 s | 211.574714 s | 212.094006 s | Same-boot SSH at 212.56 s; USB error 0. |
| PC → wall → PC | 332.994893 s | 359.256015 s | 359.775161 s | Same-boot SSH at 405.50 s; USB error 0. |

The wall source was classified as DCP (`source=4`). Charging was active from
341.565435 s until removal at 354.525292 s, approximately **12.96 seconds**,
with a programmed 650000-uA limit. All twelve retained DCP samples show
`active=1 fault=0x0`; their voltage readings span 4.096142–4.195239 V. Samples
contain sequential reads, not atomic simultaneous measurements. This is a brief
no-fault result, not sustained-gain, full-charge or offline-charge qualification.
The earlier OVP fault `0x10` remains unresolved.

At final readback, uptime 504.93 s, USB remained configured with error 0 and PC
charging remained active with fault 0. The recorder was deliberately stopped
with SIGTERM after the test; its `END exit=1` records that stop, not a driver fault.

## Installed changes and limits

* [Canonical startup script](../../../buildroot/board/y2/overlay/etc/init.d/S20y2-usb)
  now applies the workaround to the Y2 MUSB child only when the release is
  exactly `6.18.0-y2linux-m4-power-02`. It is installed at
  `/etc/init.d/S20y2-usb`; its SHA256 matches the repository file:
  `3647799ffb982afee5301739654a0d918b41f332b6b942605639de824fab5578`.
  The original is backed up at `/data/logs/usb-charge.qbEwnO/S20y2-usb.before`.
* Host and target shell syntax checks passed. Running the installed startup
  path preserved SSH and reported `configured`, `on`, `active`; both physical
  reconnects passed. A fresh boot after installing the startup change was not
  tested, and no broader kernel qualification was run.
* [Bounded recorder](../../../tools/development/record-usb-charge.sh) is explicit,
  accepts an observation window of 1–600 seconds, and retains diagnostics on
  normal production Y2DATA. It never writes charging or USB policy. Run through
  `nohup` when testing removal; it is not an automatic boot service.
* Holding runtime PM active can increase idle power consumption; no power
  measurement was made. System suspend/resume and offline initramfs behavior
  were not newly qualified. The startup workaround acts in normal Buildroot.
* The source's detach/re-entry paths access MUSB state without taking a runtime
  PM reference. The successful `on` experiment makes this interaction a strong
  lead, not proof of the exact interrupt source. A proper kernel correction must
  coordinate that lifecycle with runtime suspend/resume and pass reconnect with
  `power/control=auto`. Do not remove the interrupt-storm guard to hide failure.

The quick workaround unblocks M4 charging observation. #27/#28 reconnect and
#30 power remain partial; no milestone is closed or new hardware phase activated.
