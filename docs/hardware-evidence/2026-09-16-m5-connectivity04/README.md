# CONNECTIVITY-04: stable Linux, first WMT command times out

The owner installed CONNECTIVITY-04 and requested inspection. Authenticated SSH
confirms kernel `6.18.0-y2linux-m5-connectivity-04`, retained CONNECTIVITY-03 root
and calibration-helper SHA256 `098c12fbc028d7f3d7f02dc130e7c31c9a0db386df8a88dde81dcb6542ec02da`.
These are runtime markers/helper verification, not fresh full-partition hashes.

## Physical results

```text
[49.192827] MD1 calibration boot: stage=2 FS=841 restore=1 open=0 result=0 poweroff=0
[49.203830] CONN EMI remap verified; starting shared core
[49.212791] CONN chip=6582; starting BTIF transport
[53.282816] connectivity start failed: -110
```

- Linux remains responsive through uptime **813.75 seconds**, retaining the same
  boot ID. No panic, Oops, BUG or paging-fault signature appears in the capture.
  This is a bounded observation, not proof against every possible crash.
- MD completion reports 841 FS exchanges, restore success and successful
  power-off. A bounded `y2-radio wifi on-runtime` / `off-runtime` attempt at
  uptimes 272.70–278.79 increments recoveries from 1 to 2 without repeating MD
  calibration or rebooting. Both persistent radio preferences remain off.
- Every attempt reaches the verified remap and MMIO chip ID `6582`, then fails
  after the first WMT command's four-second timeout (`-110`). Shared status is
  `powered=0 functions=0x0 calibrated=1 error=-110 transport_errors=0`.
  `chip/hvr/fvr=0000` in status means WMT identification did not finish; it does
  not contradict the MMIO chip ID. Silicon stepping remains unqueried.
- BTIF, TX DMA, RX DMA and wake IRQ counters remain zero; MD IRQ count is 841.
  No `wlan0`, no `iw dev` result and no usable BlueZ adapter. Discovery,
  networking, pairing, audio, EDR/BLE and coexistence cannot yet be qualified.
- Internal p5 Y2ROOT and p7 Y2DATA remain mounted ext4 read/write; wired USB SSH
  works. At uptime 128, SDP allowance is 500 mA, charge setting 450 mA, CV 4.175 V
  and fault 0. CPU/PMIC readings are about 43.6/42.8 C; CPU is 598 MHz.
  At the final snapshot the charger reports not charging with those same
  current/CV settings. No calibrated pack telemetry or full charge-cycle pass
  is inferred from this inspection.
- No suspend, reboot, RTC setting, offline-charge, audio/input or storage stress
  series was run. Accepted M4 architecture remains intact in source; this short
  inspection does not replace the required later M5/M4 regression session.

## Next correction and privacy

The [CONNECTIVITY-05 correction](../../knowledge/m5-connectivity05-corrections.md)
frames the initial WMT command in stock BTIF mandatory STP mode. It follows
this physical timeout and source evidence, not a new entry audit. M5 stays open.

Raw inspection, live stream, recovery and final snapshot remain mode-0600 files
under `evidence-private/20260916-m5-connectivity04/`. No raw calibration, radio
address, pairing key or credential is published. The runtime test made no
protected write or persistent preference change. No assistant deployment occurred.
