# Latest state — CONNECTIVITY-10 installed; Wi-Fi scans verified (2026-09-17)

Strictly pinned SSH confirms `6.18.0-y2linux-m5-connectivity-10`. **wlan0/cfg80211
stays registered and scans succeed**, returning 23 then 12 BSS entries. Runtime
Wi-Fi restart works; standard BlueZ powers on with Wi-Fi active. Core/transport
errors and recoveries remain 0, kernel taint is 0, and no diagnostic module or
runtime correction is used. [Physical result](../hardware-evidence/2026-09-17-m5-connectivity10/README.md).

Both radios and saved preferences are returned off. No saved network exists,
so association/authentication/DHCP/data remain untested. Bluetooth pairing,
audio and sustained coexistence remain open. M4 stays accepted, M5 stays open,
and GPU/Reborn remain out of scope. The next useful Wi-Fi step is an owner
network connection check, not renewed firmware/factory/DMA investigation.

Final source `206f1da`, BOOTIMG 6150144 bytes, SHA256
`d7fbb0808db953a7c8d26f3846815b7b5fbc67caa1b86248d7f992dba51f30a7`.
All 16 targeted checks, artifact/package checks and package SHA256 checks pass.
Retain -07 root/data and verified -09 fallback. [Installed image receipt](../build/y2linux-m5-connectivity-10-deployment.md).
Earlier installation-pending text below is history; no repeat flash is required.

# Wi-Fi regulatory completion fixed in source (2026-09-17)

The installed -09 Wi-Fi firmware starts successfully. The exact later failure
is regulatory TX power command `0x38`: no firmware reply is expected, and the
producer's missing completion callback leaves its OID waiting until timeout
and shared recovery. [Physical trace and causal source path](../hardware-evidence/2026-09-17-m5-wifi/README.md).

CONNECTIVITY-10 adds the standard done/timeout callbacks to this one command
producer, plus small registration/regulatory failure logs. All 16 targeted
checks pass; replaying the -09 producer fails the regression. Build BOOTIMG-only
and retain -07 root/data and -09 fallback, then verify stable wlan0/cfg80211
after manual owner installation. No physical -10 success is claimed yet.
Debug masks are restored; both radios and saved preferences are off. Idle
recovery clears the diagnostic EIO, and BlueZ power-on/off still succeeds.
M4 remains accepted, M5 open, and GPU/Reborn out of scope.

# CONNECTIVITY-09 installed; permanent BlueZ verified (2026-09-17)

Strictly pinned SSH confirms owner-installed `6.18.0-y2linux-m5-connectivity-09`.
**Standard hci0/BlueZ appears on normal boot and runtime power-on/off succeeds.**
Both patches/resets, RF calibration with a 630-byte result, and the scoped HCI
page correction succeed. Core errors, transport errors and recoveries remain
0; kernel taint is 0. No diagnostic module or runtime correction is used.
[Complete physical result](../hardware-evidence/2026-09-17-m5-connectivity09/README.md).

The original RF-result EPROTO and subsequent `0x1004` page-2/status-`0x30`
failure are resolved in the installed production image. Both radios and saved
preferences are returned off. Wi-Fi's last active test remains the -08 failure
after Wi-Fi Start without wlan0; its next exact failure still needs diagnosis.
Pairing, BLE/EDR data, audio and coexistence remain unqualified. M5 stays open;
M4 remains accepted. Do not repeat factory/DMA/identification work or start
GPU/Reborn. Earlier installation-pending descriptions below are history.

CONNECTIVITY-09 source `f55fff4`, BOOTIMG 6150144 bytes, SHA256
`200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a`.
Fifteen targeted checks and emitted artifact/package checks pass.
[Installed BOOTIMG receipt](../build/y2linux-m5-connectivity-09-deployment.md).
Retain -07 root/data and -08 fallback. The normal-boot SSH verification is
complete; another installation is not needed for this checkpoint.

# Latest state — CONNECTIVITY-07 protocol failure located (2026-09-17)

The provider, DMA/IRQs and controller identification work. The own live capture
proves both patches/reset replies succeed; native STP rejects a successful
630-byte RF calibration event as EPROTO. See the
[physical protocol evidence](../hardware-evidence/2026-09-17-m5-protocol/README.md)
and [CONNECTIVITY-08 correction](../knowledge/m5-connectivity08-corrections.md).
Build BOOTIMG-only, preserve current CONNECTIVITY-07 root/data, and continue
live bring-up after manual owner installation. M5 remains open, M4 intact.
Earlier provider/TX-blocker text below is historical.

CONNECTIVITY-08 is ready: [manual BOOTIMG-only installation](../build/y2linux-m5-connectivity-08-deployment.md),
source `8b18ca3`, SHA256
`5b8e333cdb9fff7bf314abf905ad11a7f26d23e8dab4fd4a4cf78a0bd58b0d78`.
All 14 targeted connectivity checks and emitted artifact/package checks pass.
The new kernel has not yet been physically tested; continue SSH after installation.

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
