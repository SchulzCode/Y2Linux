# Session handoff — 2026-09-16, M5 candidate ready

**Latest update: the owner installed CONNECTIVITY-01.** Read-only SSH confirms
the running kernel/root markers and working internal root/data/USB SSH. Native
radio startup fails in the MD calibration handshake (`stage=1`, `FS=0`,
`EPROTO`); BlueZ has no usable adapter and Wi-Fi is absent. See the
[first physical inspection](../hardware-evidence/2026-09-16-m5-connectivity01/README.md).
The next work is a targeted startup correction, followed by the physical matrix.
The original deployment handoff below is retained as history, not a request to
flash the same candidate again. M5 remains open.

**Stop for owner manual BOOTIMG + Y2ROOT installation. M5 remains OPEN.**
Use the [complete 26-field deployment receipt](../build/y2linux-m5-connectivity-01-deployment.md).
No assistant flash/deployment or protected write occurred. M4 POWER-03 was
explicitly accepted by the owner before authorizing this M5 implementation.
That acceptance does not imply a new agent-run M4 measurement series.

## Exact built state

- Source: `022c701010c467904ab6025cd98535d3b861c771`.
- Kernel: `6.18.0-y2linux-m5-connectivity-01`.
- Buildroot/root version: `2025.02.17-connectivity.1`.
- Package: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m5-connectivity-01/`.
- BOOTIMG: 6144000 bytes, SHA256
  `a24b257a795b8ffea198e57344f20a514f4fa5e7148572ac8331afed9e996a96`.
- Y2ROOT: 536870912 bytes, SHA256
  `add6b375d33dbfa3f36e505c9784c934c3b31e2073041f514dcd2db524408e67`.
- Fallback BOOTIMG: accepted POWER-03, SHA256
  `66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010`.
- Fallback Y2ROOT: previous production root, SHA256
  `814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f`.
- Exactly BOOTIMG/ANDROID selected by the preserving scatter; no Y2DATA image,
  data reset, changed storage coordinates or protected payload mapping.

[Evidence](../build/evidence/y2linux-m5-connectivity-01/README.md): 81 tests,
eight isolated ARM userspace checks, 16 package rejection cases and final
artifact/rootfs checks passed. These are host results, not native radio passes.

## Implementation and remaining physical work

The [implementation contract](../knowledge/m5-connectivity-implementation.md)
describes the shared CONSYS owner, BTIF/AP_DMA/STP/HCI, AHB cfg80211 fullmac,
hash-checked owner firmware and read-only factory provider. A bounded native
MD calibration/filesystem lifecycle uses only this unit's retained factory
records; its actual completion gate and radio result remain to be proved.
No external player's calibration is used. No firmware blobs or identities
are committed; the owner-local image is not approved for redistribution.

Production userspace includes wpa_supplicant 2.12, BlueZ 5.79 and BlueALSA 4.3.1
with SBC. Credentials, bonds, radio identities and preferred audio peer persist
on Y2DATA. Standard APIs, a bounded A2DP/AVRCP fixture, reconnect policy and
radio-off full-suspend/activity-lease policy are implemented.

After the owner reports this candidate running, use existing authenticated SSH
at `root@10.42.0.1`. Perform the receipt's coherent session: identify silicon and
calibration, Wi-Fi WPA2/DHCP/DNS/reconnect, multiple Bluetooth audio categories,
SBC/AVRCP/reconnect, actual EDR/LE reporting, sustained coexistence, repeated radio
restart/recovery, screen-off work, deep Power/RTC wake and M4 regression. Keep
raw captures, addresses, credentials and bonds private. Make only targeted fixes
for concrete failures. Do not claim a physical pass from interface presence.

The completed entry audit is `67cbe8f`; do not repeat it or reacquire protected
partitions. Older-board FM, GPU/lima, Y2PlayerNative and OTA implementation remain
out of scope. Close #31 only after physical end-user acceptance.
