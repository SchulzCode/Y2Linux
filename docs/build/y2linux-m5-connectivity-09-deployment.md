# CONNECTIVITY-09 — permanent MT6582 E2 HCI page correction

Bluetooth has reached standard hci0/BlueZ on the live -08 Y2 with the verified
runtime correction. **Normal BlueZ power-on succeeds.** The remaining HCI
initialization defect was page 2 of Read Local Extended Features: page 1
advertises maximum 2, but page 2 returns status `0x30`. Linux's existing quirk
and verified page limit 1 fix it without changing EDR/LE feature/command bits.
[Physical result and limits](../hardware-evidence/2026-09-17-m5-connectivity08/README.md).

This candidate makes the correction permanent, only for chip `6582`, HVR
`8a01`, FVR `8a00`. The -08 RF calibration correction, patches, factory provider,
DMA, M4 and memory/storage layout remain intact. Wi-Fi still fails after its
Start command; no Wi-Fi fix or full radio qualification is claimed.

## Artifacts and validation

Directory: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m5-connectivity-09/`.
Source: `f55fff48926c578fcfcd84ef8074d0693b3442e3`.
Kernel: `6.18.0-y2linux-m5-connectivity-09`.
Retain installed CONNECTIVITY-07 Y2ROOT and all Y2DATA.

| File | Bytes | SHA256 |
| --- | ---: | --- |
| `BOOTIMG.img` | 6150144 | `200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a` |
| `fallback/BOOTIMG-previous.img` | 6150144 | `5b8e333cdb9fff7bf314abf905ad11a7f26d23e8dab4fd4a4cf78a0bd58b0d78` |

[Validation receipts](evidence/y2linux-m5-connectivity-09/README.md): 15 targeted
connectivity tests, one clean build, emitted D08/DT/memory/BOOTIMG checks and
BOOTIMG-only package validation. The live HCI correction is verified; this
new -09 image still needs normal-boot confirmation.

## Owner installation and next observation

The existing deployment contract reserves installation for the owner. Verify
`sha256sum -c SHA256SUMS` in the package directory. Use the established SP Flash
Tool setup, `MT6582_BOOTIMG_only_scatter.txt`, **Download Only**, and **BOOTIMG →
BOOTIMG.img only**. Preserve ANDROID/Y2ROOT, USRDATA/Y2DATA, loaders, tables and
protected partitions. No Format/Firmware Upgrade or assistant flash.

Boot normal Linux and reconnect USB SSH. Verify -09 with `uname -r`, the WMT
630-byte calibration success, `bluetoothctl list/show`, and one standard
runtime-only power-on/off while retaining saved preferences. The expected
BlueZ adapter should appear without any diagnostic module or runtime patch.
No factory/DMA/identification re-investigation is needed.

Fallback selects only BOOTIMG, using `fallback/BOOTIMG-previous.img` (-08).
It keeps the RF fix but retains the unfixed HCI page-2 failure on a fresh boot;
the temporary live HCI quirk does not survive reboot. Accepted M4 fallback
artifacts remain documented in the original M5 handoff.

The current -08 session was returned to both radios off, with saved preferences
still 0 and the BlueZ adapter still visible. Pairing, A2DP, BLE/EDR data and
coexistence remain unqualified. M5 stays open; GPU/Reborn remain out of scope.
