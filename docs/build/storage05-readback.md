# Storage05: corrected SPFT readback coordinates

## Completed batch: address mode mismatch

The owner completed all six reads at 21:00:03 CEST on 2026-09-13. SPFT returned
`S_DONE(0x0)` and all 13824 bytes, but its actual MT6582 path used
`NUTL_ADDR_LOGICAL` and `EMMC_PART_UNKNOWN (0x00)`: legacy global DA addresses.
The earlier instructions supplied user-area physical offsets without accounting
for this mode. That was an instruction error, not evidence of erased partitions.

The file named `MBR.bin` begins with `EMMC_BOOT`. The file named
`Y2ROOT-prefix.bin` exactly matches all 4096 bytes of the stock SEC_RO prefix,
whose scatter/global address is `0x05180000`. The BOOTIMG control is all zero,
so the intended-location identity gate fails. These samples establish the
readback address mismatch; they cannot establish the intended table or ext4
contents. [Analysis](../hardware-evidence/2026-09-13-storage05-owner/readback-analysis.json)
and [selected SPFT log lines](../hardware-evidence/2026-09-13-storage05-owner/readback-log-excerpt.txt).
Original samples and complete logs are retained under
`evidence-private/20260913-storage05-readback-210003/`; they are not public raw
partition evidence. No firmware was built, device write initiated or GUI changed
by the assistant during this analysis.

## Correct the existing six rows

In the **same SPFT Readback mode that just completed**, change only each row's
hexadecimal Start Address to the value below. Keep filenames and lengths. The
existing sample files may be replaced because this completed batch has been
retained separately. Save to `out/storage05-readback/samples/` again.

| Output file | Start address in current legacy mode | Length |
| --- | --- | --- |
| BOOTIMG-prefix.bin | `0x03180000` | `0x1000` |
| MBR.bin | `0x01400000` | `0x200` |
| EBR1.bin | `0x01480000` | `0x200` |
| EBR2.bin | `0x05b00000` | `0x200` |
| Y2ROOT-prefix.bin | `0x06580000` | `0x1000` |
| Y2DATA-prefix.bin | `0x41780000` | `0x1000` |

These values match the stock scatter's **linear** addresses and the tool's
retained PMT location report. The active mode is already corroborated by the
exact SEC_RO prefix match. This is a correction to host acquisition coordinates,
not a Linux LBA offset or a change to the partition layout. Do not use these
global addresses with a different backend explicitly selecting `EMMC_USER`.

Enable only these six Readback rows and repeat the same manual powered-off
preloader connection procedure. Use **Read Back** only. There is no Download,
Format, Firmware Upgrade or Write Memory operation. The corrected batch still
samples only 13824 bytes at the intended BOOTIMG, table and ext4 locations.
No new kernel or filesystem image is needed for it.

The prepared `out/storage05-readback/corrected-plan.json` and its
[retained copy](../hardware-evidence/2026-09-13-storage05-owner/corrected-readback-plan.json)
record this exact mode and coordinates. `expected-prefix.sha256` is unchanged:
the expected contents did not change. Check the next run's logged addresses and
mode and require the known Storage05 BOOTIMG prefix to match before interpreting
MBR/EBR differences. Ext4 prefixes establish superblock identity only, not full
filesystem integrity. No table or root/data rewrite follows from this batch.

## Historical preparation

The [original physical-offset plan](../hardware-evidence/2026-09-13-storage05-owner/proposed-readback-plan.json)
and `out/storage05-readback/plan.json` remain unchanged as provenance for the
completed batch. They are superseded for this observed SPFT mode. The Linux
partition geometry and existing firmware manifests are not changed by this
host-tool coordinate correction.
