# Storage05: one independent read-only storage check

The [ACM capture](../hardware-evidence/2026-09-13-storage05-owner/README.md)
confirms sector addressing, user-area access (`PART_CONFIG=0x48`), successful
SBC/CMD18 status and a zero sector-zero signature. It cannot export arbitrary
sector bytes. An independent SPFT Readback can distinguish incorrect Linux data
from on-device metadata without another kernel or any storage rewrite.

This is a prepared manual operation; it has not been executed. Preserve the
current Storage05 installation. Use the existing SP Flash Tool v5.2032 and DA,
the **Readback** tab, and these six entries in one batch. Each entry uses region
**EMMC_USER** and a hexadecimal **physical byte address**, not a scatter-linear
address. Save the files together in `out/storage05-readback/samples/`.

| Output file | Start address | Length | Purpose |
| --- | --- | --- | --- |
| BOOTIMG-prefix.bin | `0x01d80000` | `0x1000` | Known Storage05 bytes to check device/address selection |
| MBR.bin | `0x00000000` | `0x200` | Sector zero and its actual signature/table |
| EBR1.bin | `0x00080000` | `0x200` | First extended table |
| EBR2.bin | `0x04700000` | `0x200` | Second extended table |
| Y2ROOT-prefix.bin | `0x05180000` | `0x1000` | Existing ANDROID ext4 superblock/identity |
| Y2DATA-prefix.bin | `0x40380000` | `0x1000` | Existing USRDATA ext4 superblock/identity |

Total: **13824 bytes (13.5 KiB)**. These lengths are samples, not full partition
backups. No Download, Format, Firmware Upgrade or Write Memory operation is part
of this check. No boot hardware region, NVRAM or calibration range is sampled.
SPFT requires the existing manual preloader connection procedure rather than the
running Linux ACM connection. Keep its operation log and region/address choices.

The prepared `out/storage05-readback/plan.json` records exact coordinates and
expected local-source prefix hashes. `expected-prefix.sha256` compares only these
small files; mismatches are evidence to investigate, not a repair instruction.
The existing ROM/recovery provenance was not repeated to prepare this plan.

Before drawing a media conclusion, the BOOTIMG sample must match the known
Storage05 prefix and the DA/device/address selection must be established. A
matching MBR/EBR set with Linux still returning zero points to the Linux read
path. An independently invalid MBR with valid controls establishes invalid bytes
at that observed location; it does not authorize rewriting them or identify when
they changed. Different controls leave address/device/image identity unresolved.
The root/data samples identify location and ext4 superblocks, not full filesystem
integrity. Full-image reflashing is not justified by the current ACM log.
