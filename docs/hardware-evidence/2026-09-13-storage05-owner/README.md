# Storage05 owner boot result — failed internal discovery

**2026-09-13 update:** Corrected readback at21:10:43 CEST matches all six samples. Tables are intact; flashed ext4 prefixes are correct. Actual stock FM code proves the missing23552-sector translation. [Verified result](readback-corrected-analysis.json), [production correction](../../knowledge/storage06-addressing-correction.md).

## Latest: SPFT readback completed in a different address mode

At 21:00:03 CEST the owner completed the six Readback entries. All 13824 bytes
arrived and SPFT reported `S_DONE(0x0)`. The retained commands selected
`EMMC_PART_UNKNOWN (0x00)` with `NUTL_ADDR_LOGICAL`; the supplied physical
offsets were interpreted in the legacy global DA address space. This exposes an
error in the assistant's instructions, not an on-device partition-table result.

All six intended-prefix comparisons fail. The file named MBR has an `EMMC_BOOT`
header. The file named Y2ROOT exactly equals the stock SEC_RO first 4096 bytes
at global address `0x05180000`. This independently corroborates the address
mode. BOOTIMG is all zero at the incorrectly selected location; the identity
gate fails, so no intended MBR/EBR corruption or ext4 damage is established.
[Analysis](readback-analysis.json), [selected log lines](readback-log-excerpt.txt).
Raw samples and complete SPFT logs stay in
`evidence-private/20260913-storage05-readback-210003/` because the unintended
locations include boot/security data. The source files in `samples/` are also
left in place until the owner's next acquisition.

The [corrected six-row plan](../../build/storage05-readback.md) uses the same
observed legacy mode and stock scatter/global addresses. It remains a 13.5-KiB
manual read, with the known BOOTIMG control required before media conclusions.
This is not a kernel address-translation finding. Firmware, DT, protected write
ranges, partition geometry and ext4 images are unchanged. No new firmware,
physical operation or GUI change was initiated by the assistant in this analysis.
#33 remains ACTIVE; internal boot remains failed. No milestone status changes.

## Earlier: read-only ACM capture succeeds

After the owner connected Storage05 and granted this ACM node's host access, a
20-second LOG1 request returned a valid `Y2LOG1 Y2LINUX-PLATFORM` header and
178779 bytes. A 40-second continuation drained 399874 bytes without sending
another command or rewinding the log. Combined, 578653 bytes contain all 2736
kernel records from sequence 0 through 2735 with no gap, covering startup through
171.833982 kernel seconds. [Initial capture](acm/capture.json),
[continuation](acm-followup/capture.json), [analysis](acm-analysis.json).
These include retained startup records and snapshots, not only capture-time state.

At 0.436047 seconds the production host reports:

```text
eMMC context: sector-addressed=1 sectors=15269888 PART_CONFIG=48
```

`48` is hexadecimal. The access field is `0x48 & 7 = 0`: user area, not a boot
hardware region. The remaining bits describe preserved boot-enable/ack settings.
At 0.442893 seconds CMD18 still returns 4096 bytes, signature0000, SBC8/00000900
and R1=00000900. No eMMC guard rejection or internal command/data transport
error is present in the complete retained interval. Repeated CMD8/CMD55 errors
belong to removable host 11240000. At 40.853244 seconds rescue reports missing/
invalid internal root/data. The retained mount/partition snapshot contains RAM
and virtual filesystems plus whole mmcblk0 only; no switch_root message appears.

This narrows the uncertainty: the captured context and normal command statuses
do not support an unselected user area or a rejected SBC explanation. They do
not independently prove the returned sector data correct. Actual MBR corruption
remains unproved. LOG1 supplies no raw-read/shell interface, so a
[single independent readback batch](../../build/storage05-readback.md) is prepared
but not performed. No reboot, block read/write, firmware build or flash was
initiated by the assistant. Source/parser/geometry and protected ranges stay
unchanged; storage #33 remains ACTIVE with internal boot failed.

## Earlier owner photograph

The owner supplied a photograph during final evidence retention and reported:
“still this is how far i come with the new bootimg”. The screen identifies
`Y2Linux STORAGE05 RESCUE` and missing/invalid internal Y2ROOT or Y2DATA.
The photo is retained privately; [identity and limitations](observation.json).
Its association with the local image is owner/build-text evidence, not an
independent whole-image device readback.

Clearly visible storage fields, with the wrapped line normalized:

```text
mmcblk0 MMC sectors=15269888
mmcblk0: mmc0:0001 FNX2MB 7.28 GiB
partition read CMD18 sector=0 bytes=4096 signature=0000 SBC=8/00000900 R1=00000900
```

No mmcblk0pN rows appear. Rescue reports missing/invalid root/data and presents
a shell prompt. USB is shown CONFIGURED with rc=0; that is device-side status,
not a new host log-transfer observation. SD absence is not established by the photo.

Linux R1=0x900 means ready-for-data and TRAN state, without reported status error
bits. SBC/CMD23 and CMD18 completed without a reported command error. The
strengthened data path reports4096 transferred bytes but still zero signature.
Explicit SBC sequencing alone therefore **did not fix the physical boot blocker**.
This does not prove MBR erasure, rule out all incorrect-data/DMA faults or validate
on-device ext4 contents. The fixed write and switch_root paths are not exercised.

Post-photo host inspection found no ttyACM0/ttyACM1, no Y2 in lsusb and no USB
network interface. No live read, shell command, reboot or block write ran. The
owner was asked to connect the running rescue for read-only LOG1 capture. LOG1
exports retained logs only; it cannot accept block reads or shell commands.
EXT_CSD/address-context logs may help, but cannot replace a sector-byte readback.
A subsequent bounded 20-second wait found no matching Storage05 ACM device;
no device was opened and no LOG1 request was sent.

The remaining distinction needs an independent read-only sector-zero observation
with verified EMMC_USER address semantics and a known-content control. Factory
tables and local ext4 files cannot substitute for that evidence. No table rewrite,
root/data rewrite or second speculative BOOTIMG is justified by this photograph.
Storage #33 remains ACTIVE with internal boot failed. Frozen r5 package metadata
predates this result; this record supersedes its pending-acceptance statement.
