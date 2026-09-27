# Candidate and fallback identity index

Updated 2026-09-28. This is an index to retained manifests and physical reports,
not a new package validation or a live-device read. The exact owner-flashed
Fix01 pair was observed on 2026-09-27. Later documentation HEADs are not the
commits compiled into it.

| Role | Source/runtime identity | Evidence and limitation |
| --- | --- | --- |
| Latest Fix01 candidate | Linux `0de6e951b438bf0f2d701e23e476d2b2405ba3c6`; Reborn `36db1869c6bab1ad2d00b7d8e7807ea6ef5b3803` | kernel `6.18.0-y2linux-cpu-final-fix01`, root `2025.02.18-platform-v1.7`, release `1.0.0-cpu-final-fix01-candidate.1`; physical acceptance FAIL |
| Prior CPU Final tested pair | Linux `ff586dfff2b5e7abd28af339c055375307308f1f`; Reborn `afcf9ffa1bc45073e97520d592c0284ce73fefcf` | kernel `6.18.0-y2linux-cpu-final`, root `2025.02.18-platform-v1.6`; older physical before-state |
| Retained known-good fallback | Hardware02 kernel `6.18.0-y2linux-hardware-02`, root `2025.02.18-platform-v1.4` | preserve exact BOOTIMG/Y2ROOT pair and Y2DATA; narrower earlier hardware evidence, not a Fix01 rollback test |
| Hardware Final software candidate | Linux `067003f8988042a832181e935128ea93e26070a7`; Reborn `5b5d23b89952d88faf3155cf680ccce918f1b099` | earlier software ledger; no Fix01 physical acceptance transfer |
| Platform v1 software candidate | Linux `d04b95aaff713edf943042d97a4c6134ca19fc24`; Reborn `6c8aa128550ec80addd08ef3145e9d5a846ddf2e` | earlier build/completion receipt; no Fix01 physical acceptance transfer |

Latest Fix01 local package: `out/y2linux-cpu-final-fix01-candidate/`.
Its manifest selects only **BOOTIMG** and **ANDROID/Y2ROOT** in the preserve-data
scatter. It has no Y2DATA replacement and no protected/preloader/LK/NVRAM/
calibration payload. Exact manifest image hashes:

| File | SHA-256 |
| --- | --- |
| `BOOTIMG.img` | `82b38fd31549f6a2aa84c10c11e4cc42015a225fddc7ded7d40feb3145a0ab25` |
| `Y2ROOT.img` | `26aa01ce84cc012e08309269a464e609e4d9dc1a9a837528eecbbe9e99a1d1f5` |
| `fallback/BOOTIMG.img` | `b2a2c3bcb7cc7783828882e447e8b867453cb5b65846ce63577076b1ca4afeea` |
| `fallback/Y2ROOT.img` | `63dbd0a198cd86e847c10ed163fd14b2cbe269595fa1988c99ac8393160bb547` |

Hashes here are copied from the sealed candidate manifest, not a fresh build or
flash assertion. The authoritative [Fix01 software receipt](../validation/Y2-CPU-FINAL-FIX01.md)
and [physical report](../validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md)
distinguish package integrity from hardware outcomes. The
[prior CPU Final report](../validation/Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md)
supplies the before-state. Never substitute a newer source HEAD or a different
image for an exact measured pair without another qualification.
