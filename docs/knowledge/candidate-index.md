# Candidate and fallback identity index

Updated 2026-10-01. This is an index to retained manifests and physical reports,
not a new package validation or a live-device read. The exact owner-flashed
Fix02 pair was observed on 2026-09-29. Later documentation HEADs are not the
commits compiled into it.

| Role | Source/runtime identity | Evidence and limitation |
| --- | --- | --- |
| Latest storage ceiling candidate (unflashed) | Linux `540ae029b13940d4b2dfe21902bed71b89cb117a`; Reborn `b92d312cc2dc4b74f57a1a9a7b1407e34707137a` | kernel `6.18.0-y2linux-storage-ceiling`, root `2025.02.18-platform-v1.11`, release `1.0.0-storage-ceiling-candidate.1`; eMMC HS200 / SD SDR104 ceilings with fallback ladders; BOOTIMG `4332d4f2…`, Y2ROOT `72eab34a…`; software-validated, physical NOT_RUN; [receipt](../validation/Y2-STORAGE-CEILING.md) |
| Product UI v2 candidate (unflashed) | Linux `5cfe04cb2fe9e1177df99f3113b7a17ee7d442f5`; Reborn `bd8436dd2afcdbe4a45b9140aa98e4d20307482b` | kernel `6.18.0-y2linux-cpu-final-fix03` (unchanged), root `2025.02.18-platform-v1.10`, release `1.0.0-reborn-product-ui-v2-candidate.1`; BOOTIMG `8a974cb5…`, Y2ROOT `a5c2914e…`; software-validated, physical NOT_RUN; [receipt](../validation/Y2-REBORN-PRODUCT-UI-V2.md) |
| Fix03 candidate (unflashed) | Linux `76a5d41df683a1680b1127393deb0101aafe7697`; Reborn `77cf83e09a18f82a867040e72d35b6e70b26fa85` | kernel `6.18.0-y2linux-cpu-final-fix03`, root `2025.02.18-platform-v1.9`, release `1.0.0-cpu-final-fix03-candidate.1`; BOOTIMG `c55b2640…`, Y2ROOT `04cafd4b…`; software-validated, physical NOT_RUN |
| Installed Fix02 pair | Linux `1a1a6693dcd82f62daecd4c1491ef512823a86c5`; Reborn `77cf83e09a18f82a867040e72d35b6e70b26fa85` | kernel `6.18.0-y2linux-cpu-final-fix02`, root `2025.02.18-platform-v1.8`; BOOTIMG `a621e270…`, Y2ROOT `5010d839…`; physical acceptance FAIL (awake CPU qualified) |
| Fix01 candidate | Linux `0de6e951b438bf0f2d701e23e476d2b2405ba3c6`; Reborn `36db1869c6bab1ad2d00b7d8e7807ea6ef5b3803` | kernel `6.18.0-y2linux-cpu-final-fix01`, root `2025.02.18-platform-v1.7`, release `1.0.0-cpu-final-fix01-candidate.1`; physical acceptance FAIL |
| Prior CPU Final tested pair | Linux `ff586dfff2b5e7abd28af339c055375307308f1f`; Reborn `afcf9ffa1bc45073e97520d592c0284ce73fefcf` | kernel `6.18.0-y2linux-cpu-final`, root `2025.02.18-platform-v1.6`; older physical before-state |
| Retained known-good fallback | Hardware02 kernel `6.18.0-y2linux-hardware-02`, root `2025.02.18-platform-v1.4` | preserve exact BOOTIMG/Y2ROOT pair and Y2DATA; narrower earlier hardware evidence, not a Fix01 rollback test |
| Hardware Final software candidate | Linux `067003f8988042a832181e935128ea93e26070a7`; Reborn `5b5d23b89952d88faf3155cf680ccce918f1b099` | earlier software ledger; no Fix01 physical acceptance transfer |
| Platform v1 software candidate | Linux `d04b95aaff713edf943042d97a4c6134ca19fc24`; Reborn `6c8aa128550ec80addd08ef3145e9d5a846ddf2e` | earlier build/completion receipt; no Fix01 physical acceptance transfer |

Latest local package (Fix03, unflashed): `out/y2linux-cpu-final-fix03-candidate/`.
Its manifest selects only **BOOTIMG** and **ANDROID/Y2ROOT** in the preserve-data
scatter. It has no Y2DATA replacement and no protected/preloader/LK/NVRAM/
calibration payload. Exact manifest image hashes:

| File | SHA-256 |
| --- | --- |
| `BOOTIMG.img` | `c55b26407e46a436f2bfffe216feecfd18ced6b7bd404ed8ab6eaa013ba9cce6` |
| `Y2ROOT.img` | `04cafd4bddab65a01e0a35c056ee9a22a6b00ffa059cd863a95095af635d7257` |
| `fallback/BOOTIMG.img` | `b2a2c3bcb7cc7783828882e447e8b867453cb5b65846ce63577076b1ca4afeea` |
| `fallback/Y2ROOT.img` | `63dbd0a198cd86e847c10ed163fd14b2cbe269595fa1988c99ac8393160bb547` |

Hashes here are copied from the sealed candidate manifest, not a fresh build or
flash assertion. The [Fix03 software receipt](../validation/Y2-CPU-FINAL-FIX03.md)
records package integrity; the [Fix02 physical report](../validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md)
is the latest hardware outcome and before-state. Fix01 (BOOTIMG `82b38fd3…`)
and Fix02 (BOOTIMG `a621e270…`) receipts keep their own hashes. Never substitute a newer source HEAD or a different
image for an exact measured pair without another qualification.
