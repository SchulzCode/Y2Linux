# CONNECTIVITY-10 — installed Wi-Fi regulatory completion correction

**The owner-installed image exposes stable wlan0/cfg80211 and scans successfully.**
Two scans return 23 and 12 BSS entries. Runtime Wi-Fi restart and standard BlueZ
power-on with Wi-Fi active succeed, with zero core/transport errors or recoveries.
[Physical verification and limits](../hardware-evidence/2026-09-17-m5-connectivity10/README.md).

The exact -09 failure was Set TX Power (`0x38`) completing transmission but not
its waiting OID. CONNECTIVITY-10 supplies the standard callbacks for that
command. Regulatory channels/power limits, firmware, transport and the working
Bluetooth corrections remain intact.

## Image and validation

Directory: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m5-connectivity-10/`.
Final source: `206f1da29d50a4a22cf01331a49b4519cd7a925f`.
Kernel: `6.18.0-y2linux-m5-connectivity-10`.
Retain installed CONNECTIVITY-07 Y2ROOT and all Y2DATA.

| File | Bytes | SHA256 |
| --- | ---: | --- |
| `BOOTIMG.img` | 6150144 | `d7fbb0808db953a7c8d26f3846815b7b5fbc67caa1b86248d7f992dba51f30a7` |
| `fallback/BOOTIMG-previous.img` | 6150144 | `200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a` |

[Validation receipts](evidence/y2linux-m5-connectivity-10/README.md): 16 targeted
tests against emitted -10 artifacts, old-source failure replay, production
build, config/DT/memory/BOOTIMG checks and preserving package/hash validation.
The final format-only diagnostic correction emits the same BOOTIMG bytes as
the first -10 candidate. The installed image needs no repeat installation.

## Retained manual installation and fallback procedure

Installation remains the owner's step under the existing deployment contract.
For any later reinstall, verify `sha256sum -c SHA256SUMS`, use this directory's
`MT6582_BOOTIMG_only_scatter.txt`, **Download Only**, **BOOTIMG.img only**.
Preserve Y2ROOT/Y2DATA, loaders, partition tables and protected partitions.
Fallback selects only `fallback/BOOTIMG-previous.img`: verified -09 Bluetooth,
with the earlier Wi-Fi regulatory completion failure. No assistant flash occurs.

The verification leaves both radios and saved preferences off. No network is
configured, so association/DHCP/data traffic remain untested. Bluetooth pairing,
audio and sustained coexistence remain unqualified. M4 stays intact and M5
open; GPU/Reborn are not started.
