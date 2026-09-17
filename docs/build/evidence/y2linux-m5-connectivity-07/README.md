# CONNECTIVITY-07 retained build receipt and superseding physical result

These existing build receipts were untracked at session entry on 2026-09-17
and are preserved here without rerunning source/ROM/factory qualification.
Source `359f7870859c546290d3a56dcdc601585f05d66f`, BOOTIMG 6150144 bytes,
SHA256 `eaf2360d3e29b77d860689c4f9b97f6729143e0cebcd8bef4e001b72cee39ea0`.
The owner installed this candidate. The mounted-eMMC provider now succeeds,
DMA/IRQs work, and the controller replies MT6582/HVR `8a01`/FVR `8a00`.

[The fresh physical capture](../../../hardware-evidence/2026-09-17-m5-protocol/README.md)
locates the remaining EPROTO at the extended RF calibration result, after both
patches and resets. No usable BlueZ/cfg80211 interface is claimed. The -07
BOOTIMG is the responsive Linux fallback for the -08 correction; M4 is retained.
