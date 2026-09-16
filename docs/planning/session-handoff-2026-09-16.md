# Latest state — CONNECTIVITY-06 installed; mounted-eMMC provider correction

SSH confirms CONNECTIVITY-06. Internal root/data are on mmcblk1; the provider
hardcodes absent mmcblk0 and fails before radio activation. The TX fix remains
physically untested. [Evidence](../build/evidence/y2linux-m5-connectivity-06/README.md).
CONNECTIVITY-07 resolves the validated eMMC parent of mounted root/data.
This needs BOOTIMG + Y2ROOT, preserving Y2DATA. Build and stop for manual installation.
M5 #31 stays open; accepted M4 is unchanged. Earlier receipts below are history.

# Latest state — CONNECTIVITY-05 installed, CONNECTIVITY-06 correction

CONNECTIVITY-05 is running after owner installation. MD calibration completes,
but the 26-byte first WMT command stops after 24 bytes; no TX IRQ is serviced.
Linux remains responsive with the same boot ID, storage and PC charging working.
[Physical evidence](../hardware-evidence/2026-09-16-m5-connectivity05/README.md).
CONNECTIVITY-06 corrects the missing per-transfer TX interrupt rearm and DMA
submission order. Build a BOOTIMG-only candidate retaining CONNECTIVITY-03 root
and Y2DATA; stop for manual installation. M5 remains open.

The earlier CONNECTIVITY-04 artifacts and fallbacks below remain historical reference.

# Session handoff — 2026-09-16, CONNECTIVITY-04 physically inspected

The owner installed CONNECTIVITY-04. SSH confirms Linux remains responsive
through 813 seconds with the same boot ID and no observed crash, including a
bounded radio recovery. MD calibration completes (`FS=841`, restore/shutdown
successful), the corrected EMI remap verifies, and MMIO chip ID is `6582`.
The first WMT command times out; Wi-Fi/BlueZ remain unavailable.
[Physical evidence](../hardware-evidence/2026-09-16-m5-connectivity04/README.md).
**M5 remains ACTIVE / WMT STARTUP TIMEOUT / PHYSICALLY UNQUALIFIED.**

## Installed artifacts

- Source: `ac833c887d81d883827a734700639fa9be7c9f1e`.
- Kernel: `6.18.0-y2linux-m5-connectivity-04`.
- Package: `out/y2linux-m5-connectivity-04/`, BOOTIMG-only, already installed.
- BOOTIMG: 6150144 bytes; SHA256 `31bb8597ed5a10fa089866664172de664b20fe00a2f7525d08cebc73dbb99d31`.
- Retained CONNECTIVITY-03 root: source `1c2b08563c85a3443f8983f59329a2772c767fb8`,
  536870912 bytes; SHA256 `1495cc1825ca3831fc4607cf25bc04969495f6bdfa7842d634d0917ecb42824e`.
- Installed calibration helper SHA256:
  `098c12fbc028d7f3d7f02dc130e7c31c9a0db386df8a88dde81dcb6542ec02da`.
- Y2DATA is preserved. Both saved radio preferences remain off after the runtime test.

[Build evidence](../build/evidence/y2linux-m5-connectivity-04/README.md): 85 tests,
ARM rescue checks, artifact validation and 14 package rejection cases passed.
CONNECTIVITY-03 fixed the repeated FS separator and installed the corrected helper;
its subsequent crash exposed the wrong CONN remap. CONNECTIVITY-04 corrects the
offset and requires readback before power-on.

## Next targeted correction

The native initial WMT command bypasses STP, but stock enables BTIF mandatory
framing before the chip-register query. Correct that send/receive boundary,
retain full-mode CRC/ACK/retries, and build a BOOTIMG-only update retaining root.
After manual installation, qualify startup and continue the
[existing M5 procedure](../build/y2linux-m5-connectivity-01-deployment.md).
Do not repeat the entry audit/inventory or protected data acquisition.

PC charging was observed with fault 0, SDP allowance 500 mA, charge setting
450 mA and CV 4.175 V. Internal mounts/USB SSH work. No full M4 regression,
suspend, reboot, RTC persistence, audio/input or offline-charge series occurred.
Raw logs and prior crash photographs remain private. No assistant flash or
protected write occurred. Owner-key SSH remains `root@10.42.0.1`.

## Fallbacks

CONNECTIVITY-04 with the current root is the bounded-stable Linux fallback for
the next BOOTIMG; its radios still fail. **CONNECTIVITY-03 BOOTIMG crashes**;
do not select it just because it is retained in the -04 fallback folder.

Earlier matched fallback pairs, preserving Y2DATA:

- `out/y2linux-m5-connectivity-03/fallback/`: CONNECTIVITY-02 BOOTIMG
  `72b17b90d5ab21c2c52f957056f483f3af9949f0607ecc98c4c87c89aa7765f9`,
  CONNECTIVITY-01 root `add6b375d33dbfa3f36e505c9784c934c3b31e2073041f514dcd2db524408e67`.
  Restore both together; Linux boots but MD calibration fails earlier.
- `out/y2linux-m5-connectivity-01/fallback/`: accepted POWER-03 BOOTIMG
  `66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010`,
  previous root `814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f`.

M4 #30 remains owner-accepted/closed. M5 #31 stays open. Firmware provenance
and standard userspace versions remain in the original deployment receipt.
Do not start older-board FM, GPU/lima, Y2PlayerNative or OTA implementation.
