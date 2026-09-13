# Storage05 integrated production candidate evidence

Build source: `5401007791dc2d915c95baac31e91fd062c8edc0`. One kernel build,
reusing verified Storage04 rescue binaries and retaining both ext4 images.
No root/data rebuild, diagnostic firmware variant or assistant physical write.

**Offline PASS; physical internal boot FAILED.** The owner supplied the
[Storage05 rescue photograph](../../../hardware-evidence/2026-09-13-storage05-owner/README.md)
after packaging. CMD18 still returns sector0/signature0000, SBC8/00000900 and
R1=00000900. Explicit SBC sequencing did not solve the physical read/discovery
problem. `result.json` supersedes the frozen manifest's pending status.

## Retained checks

- `regression-tests.log`: 52 passing tests, including actual production MMC
  functions, switch context, bounded writes/protected rejects, the upstream parser
  with stock MBR/EBR fixtures, root/data resolver, actual ARM BusyBox handover and
  existing platform checks. Card/MMIO/DMA responses and mounts are mocked.
- `source-defects.log`: old code reproduces incorrect one-block CMD25 data type
  and falsely successful DMA faults; these do not prove the physical zero cause.
- `arm-acceptance.log`: actual ARM BusyBox reproduces fatal `set -i` exit2;
  production syntax and ARM ABI pass. The regression run also uses this ARM shell
  for successful/refused/rolled-back handover control flow.
- `artifact-validation.log`, `layout.json`: kernel/Image/zImage/DT, module ABI,
  rescue archive, memory/relocation and Android/MTK BOOTIMG bounds pass. Complete
  LK read end5193728 fits the16777216-byte allocation;5542 tail bytes are zero.
- `acceptance-final.log`: all71 DT nodes/reservations match Storage04 except
  calculated initrd-end; config differs only in LOCALVERSION. Eight isolated
  package mutations are refused even after refreshing generic checksums:
  protected MBR selection, shifted geometry, wrong UUID, extra data payload,
  wrong module release, damaged boot bytes, widened writes and wrong root source.
  Corresponding host-only scripts are retained alongside their logs.
- `userspace-source.json`, `userspace-reuse.log`: exact userspace/root/data
  identities. Both existing images passed host e2fsck -fn, host and actual ARM
  blkid, byte hash/size, root contract/layout and data schema checks. Packaging
  exited0 after base and new BOOTIMG-only validation.

Package construction's terminal result, summarized from the successful call:

```text
Y2ROOT.img: clean ext4, matching label/UUID; 769/32768 files, 11661/131072 blocks
Y2DATA.img: clean ext4, matching label/UUID; 23/51296 files, 7587/204800 blocks
PASS production BOOTIMG-only manifest, stock geometry, unchanged root/data, module ABI contract, bounds and hashes
```

`kernel-build.log.gz` retains the single build. Its EINT_MTK dependency warning
comes from unchanged platform Kconfig/select code; no build/compiler error.
`SHA256SUMS` is the frozen package inventory, not this evidence directory's
inventory. The later test-runner edit includes the already-passing handover suite
in ordinary production validation; it changes no firmware bytes.
