# Storage05 owner boot result — failed internal discovery

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
