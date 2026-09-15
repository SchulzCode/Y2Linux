# CONNECTIVITY-05 build and physical result

Source `894ac7b5f625c2e0e2c38a42368d50edea3c110d`; package
`out/y2linux-m5-connectivity-05/`. BOOTIMG is 6150144 bytes, SHA256
`505a83986b062bedf99163f4f013e96f5ef02c3d6a3ac79e8f56857ea8a6a096`.
86 production/M4/connectivity tests (56 + 30), ARM rescue checks, emitted artifact
validation and 14 package rejection cases passed. The STP regression rejects
the old bare-WMT implementation and checks fragmented replies/full-mode transition.
Kernel configuration changes only LOCALVERSION; Buildroot configuration is identical.

The owner installed this image. MD calibration completes, but the correctly framed
26-byte first command stalls with two bytes left in TX DMA. No usable Wi-Fi or
BlueZ adapter exists. [Physical result](../../../hardware-evidence/2026-09-16-m5-connectivity05/README.md).
The BOOTIMG remains unqualified for M5 connectivity.

Y2ROOT remains CONNECTIVITY-03, SHA256
`1495cc1825ca3831fc4607cf25bc04969495f6bdfa7842d634d0917ecb42824e`;
Y2DATA is preserved. Fallback BOOTIMG is CONNECTIVITY-04, SHA256
`31bb8597ed5a10fa089866664172de664b20fe00a2f7525d08cebc73dbb99d31`,
with the same root. It has bounded Linux stability but unavailable radios.
