# CONNECTIVITY-01: first inspection after owner installation

The owner reported flashing CONNECTIVITY-01 and requested a quick SSH inspection.
Authenticated, read-only SSH succeeded. No assistant deployment, service restart,
radio toggle, time change, suspend or protected-partition write was performed.
Raw captures remain private in `evidence-private/20260916-m5-connectivity01/`.

## Observed result

- Running kernel `6.18.0-y2linux-m5-connectivity-01`, root marker
  `Y2LINUX-M5-CONNECTIVITY-01`, root version `2025.02.17-connectivity.1`, source
  `022c701010c467904ab6025cd98535d3b861c771`. These are running markers, not a
  fresh full BOOTIMG/rootfs hash readback.
- Uptime advanced from about 340 to 428 seconds. No panic/Oops was seen in the
  captured boot log. Internal p5 Y2ROOT and p7 Y2DATA were mounted ext4 read/write;
  wired USB SSH worked.
- The factory provider reported successful private-record preparation through
  read-only factory partitions. Calibration service, wpa_supplicant, bluetoothd,
  BlueALSA and reconnect service were running. Both saved radio preferences
  were off.
- **Radio initialization failed:** two boot-time MD attempts reported
  `stage=1 FS=0 restore=0 open=0 result=-71 poweroff=0`.
  `-71` is `EPROTO`. The core reported `activated=1 powered=0 functions=0x0
  calibrated=0 chip=0000 hvr=0000 fvr=0000 error=-71 transport_errors=0 recoveries=1`.
  MD shutdown reported success. The exact rejected message is not captured by
  this candidate's aggregate log; do not infer a bad factory record or invent
  the missing silicon identity.
- `hci0` existed in sysfs, but `bluetoothctl list` returned no usable adapter.
  No `wlan0` or wireless interface existed. Interface registration is not a
  Bluetooth pass; Wi-Fi/pairing/audio/coexistence qualification cannot proceed
  through this startup failure.
- `regulatory.db` loading failed with `ENOENT` at uptime 0.56 seconds, before
  the production root was mounted. Both database and signature were present
  in `/lib/firmware` after handover. `iw reg get` showed the built-in world
  domain. Its listed bands do not establish this radio's hardware capabilities.
- The system clock was 2022-08-01; the RTC supplied that date at boot. The clock
  needs setting and persistence qualification; it was not modified here.
- Charging reported no fault, a 450 mA configured limit with 500 mA USB allowance,
  and a 4.175 V CV setting. The log showed voltage hold and subsequent charging
  resumption. This short snapshot is not full charge-termination qualification.
- Current CPU/PMIC temperatures were about 46/45.1 degrees C; schedutil was at
  598 MHz. Deep mode was selected, but no suspend/wake test was performed.

## Next concrete work

Target the observed MD handshake/protocol rejection and early regulatory-data
availability. Preserve bounded failure handling, own factory data, MD shutdown
and the accepted M4 platform. Do not repeat the entry audit or private partition
acquisition. After startup is corrected, continue the coherent physical M5
qualification. **M5 remains OPEN and physically unqualified.**
