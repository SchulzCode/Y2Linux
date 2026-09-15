# CONNECTIVITY-02: physical inspection after owner installation

The owner reported flashing the new image and requested inspection. Authenticated,
read-only SSH succeeded. No assistant deployment, radio restart, protected write,
service restart, time setting, suspend or reboot occurred. Raw output remains in
`evidence-private/20260916-m5-connectivity02/inspection.txt` (mode 0600).

## Results

- Running kernel is `6.18.0-y2linux-m5-connectivity-02`; the installed root remains
  CONNECTIVITY-01. These are runtime markers, not fresh image readback hashes.
- **The previous framing barrier is passed:** both startup attempts completed
  53 filesystem exchanges, compared with zero on CONNECTIVITY-01. No FS-framing
  rejection appeared. Factory provider preparation succeeded read-only.
- Both attempts then received control channel 0, message ID 4, check ID
  `0x45584350`, at stage 1. This is the MediaTek `MD_EX` exception notification,
  not successful firmware readiness. The logger calls the shared header field
  `bytes`, but on this control channel its value 4 is the message ID; similarly
  `buffer` contains the exception check ID. The protocol interpretation is
  supported by [MediaTek's control-channel definitions](https://android.googlesource.com/kernel/mediatek/+/android-mtk-3.18/drivers/misc/mediatek/include/mt-plat/mt_ccci_common.h)
  and [the older dual-CCCI exception handler](https://android.googlesource.com/kernel/mediatek/+/android-mediatek-sprout-3.4-kitkat-mr2/drivers/misc/mediatek/dual_ccci/ccci_md_main.c).
- Kernel result is `stage=1 FS=53 restore=0 open=0 result=-71 poweroff=0`.
  Shared-core status is `powered=0 functions=0x0 calibrated=0 error=-71`, with
  one recovery. MD shutdown reports success. The exception cause and last FS
  operation are not exposed by this build; do not guess the offending record.
- No `wlan0`; `iw dev` is empty. `hci0` exists in sysfs, but `bluetoothctl list`
  is empty. Both saved radio preferences are off. Wi-Fi/BT/audio/coexistence
  qualification cannot proceed through this firmware exception.
- The early `regulatory.db` ENOENT is gone. Trusted certificates load, no database
  or signature failure appears, and the world regulatory domain is available.
  This is not proof of physical Wi-Fi capability.
- Internal p5 Y2ROOT and p7 Y2DATA are mounted ext4 read/write. USB SSH works;
  uptime advances through roughly 55–162 seconds. No panic/Oops/BUG was found.
- PC source is SDP, 500 mA USB allowance, 450 mA charge setting, CV 4.175 V,
  fault 0. Charging entered voltage hold near 4.175 V and resumed at about
  4.091 V (uptime 155.9 seconds). This is a short observation, not full M4
  requalification. CPU/PMIC readings were about 46.6/45.7 C.
- Deep mode remains selected but was not exercised. The system clock remains
  unset at 2022-08-01; setting/reboot-persistence remains to be checked.

## Next targeted work

Decode the modem exception and identify the last calibration filesystem operation
and response using bounded, privacy-safe diagnostics. Preserve the successful
framing correction, own factory data and fail-closed MD shutdown. Do not bypass
calibration readiness or acknowledge an exception as boot-ready. Useful retained
stock routines are already under `out/m4-finish-reference/stock/`; no new entry
audit or protected-partition acquisition is needed.

**M5 remains open: calibration progresses but firmware initialization fails.**
