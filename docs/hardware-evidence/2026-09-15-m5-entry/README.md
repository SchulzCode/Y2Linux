# Fresh M5 entry: POWER-02 charging fault remains a prerequisite failure

2026-09-15; entry repository `bbbbbf2` with the existing USB/evidence changes,
subsequently preserved as `ef285f1`. Read-only inspection; no radio activation,
power-policy change, suspend/reboot, build or flash.

[Structured result](result.json) · [Selected charging events](power-events.txt) ·
[Own retained stock radio fields](stock-radio-fields.txt) ·
[Connectivity audit and firmware inventory](../../knowledge/m5-connectivity-entry.md).

## Current physical identity and power result

The current USB descriptor and authenticated `uname -r` identify
`6.18.0-y2linux-m4-power-02`. A bounded image-length BOOTIMG read at stock
logical offset `0x1d80000`, length **5378048**, hashes to
`1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9`.
This matches the retained package; unused partition-tail bytes were not read.

The fresh inventory starts at uptime 1363.10 s. The retained kernel log extends
the earlier USB-reconnect event history and shows the subsequent charger stop:

| Uptime | Observation |
| --- | --- |
| 719.328249 s | PC SDP charging active, programmed limit 450 mA, battery reading 4.195239 V; 622 watchdog services. |
| 747.328296 s | Battery reading 4.200073 V, status changes to Not charging, `fault=0x8`; 646 watchdog services. |
| 807.808680 s | Still inhibited with the same fault; reading 4.110424 V. |
| 1347.652830 s | Still inhibited, reading 4.069116 V; watchdog count remains 646. |
| Fresh sysfs | `phase=FAULT active=0 fault=0x8 full=0 timeout=0`, fresh voltage 4.066699 V. |

Source `kernel/platform/mt6323-charger.c` defines bit 3 as `Y2_FAULT_VOLTAGE`
and latches it when a sample is at least 4200000 µV. This explains this stop's
software classification. It does not establish analog overshoot, ADC accuracy,
measured battery current or a safe change to the protection threshold.
It is distinct from September 14's unresolved hardware-OVP fault `0x10`.
No fault was cleared or protection setting changed.

The earlier 12.96-second wall-source result remains a short no-fault observation;
it does not supersede this later sustained-PC failure. Full/offline charging,
termination/recharge and low-battery recovery remain unqualified.

## Preserved platform and limits

- Internal p5 root and p7 data are ext4 read-write at the expected stock
  geometry. The checked disk is MMC on host `11230000`, 15203328 logical sectors.
  No SD block device is listed. No filesystem repair, write or integrity test.
- MUSB is `on/active`; installed `S20y2-usb` SHA256
  `3647799ffb982afee5301739654a0d918b41f332b6b942605639de824fab5578`
  matches the preserved source. No new cable reconnect was requested.
- `s2idle [deep]` is available. SPM state at uptime 1406.35 s has
  `broken=0 entries=0 resumes=0 aborts=0`. No new deep-sleep result exists.
- RTC reports 2082-07-31. CPU thermal reports about 14.7°C versus PMIC 46.9°C;
  calibration accuracy remains unresolved. Four CPUs online, 598-MHz readback.
- `Y2Audio` is present; playback/display/input/wake behavior was not requalified.
  `lo` and `usb0` are the only network interfaces; no Bluetooth HCI device.

## Calibration acquisition and SSH trust

Strict SSH initially rejected the stale default host entry. The presented key
then matched the **exact fingerprint retained from the September 14 reinstall
inspection**. That match was checked before using a private temporary
`known_hosts` file with strict checking. The default file/account/client identity
was not changed. This is continuity with that retained pin, not an independently
owner-verified fingerprint. The identifying key/fingerprint stays private.

After release, host/type, logical capacity, p5/p7 bounds and BOOTIMG identity
checks, bounded `dd if=/dev/mmcblk0` reads obtained own NVRAM (5 MiB) and
PROTECT_F/S (10 MiB each). PROTECT p2/p3 start and length were checked as well.
All addresses were stock logical coordinates; the existing kernel translates
to native EMMC_USER. No device `of=`, mount, journal replay or protected write.

Host `debugfs` without `-w` inspected regular-file copies. PROTECT_F/S have
small `/md` record sets; the binary NVRAM container is nonempty. Current Y2DATA
does not contain Android `/data/nvram`. Record decoding/calibration ownership,
controller stepping and valid persistent radio addresses are still unproved.
Do not infer missing/corrupt calibration from the absence of plaintext filenames.

Raw partitions, full logs, host pin, private receipts/hashes and extracted vendor
firmware stay in ignored `evidence-private/20260915-m5-entry/`. Public output
contains only reviewed non-identifying facts. This local copy is not an
independent backup and does not repeat or close the wider recovery gate.

**M4 #30 remains ACTIVE/PARTIAL. M5 #31 has a completed entry audit but cannot
proceed to an integrated deployment on the asserted finished-M4 basis.**
The current charger fault and missing acceptance must be resolved in M4; own
silicon/firmware/calibration mapping remains M5's next identity dependency.
