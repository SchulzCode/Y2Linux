# Stock boot structure and unresolved boot contract

Date: 2026-09-08. Task: [Y2E-115](https://github.com/SchulzCode/Y2Linux/issues/4). Capture: `20260908-boot`. Status: offline structure established; bootloader contract and recovery remain UNKNOWN.

## Exact package input analysis

Both original images were read without modification. The existing `tools/adb/build_adb_boot.py` parser was inspected and imported with Python bytecode writes disabled; no build/patch/pack entrypoint ran. Parser source SHA-256: `4ad50feb81ede3f83419bdcc8e395fda1b20cd45303b210403fc85ad39e54fc3`. Host-only scratch analysis checked component bounds and decompressed gzip/parsed cpio in memory with size limits. No extracted program was executed and no archive-controlled pathname was written to the filesystem.

| Field | Stock boot | Stock recovery |
| --- | --- | --- |
| Image bytes | 5,654,528 | 6,039,552 |
| Page size | 2,048 | 2,048 |
| Kernel offset / size | 2,048 / 4,942,168 | same |
| Ramdisk offset / size | 4,945,920 / 707,660 | 4,945,920 / 1,092,308 |
| Second stage / declared DT / trailing bytes | 0 / 0 / 0 | 0 / 0 / 0 |
| Header command line | empty | empty |
| Ramdisk MTK wrapper name | ROOTFS | RECOVERY |
| Cpio entries including trailer | 44 | 61 |

The wrapper magic is `0x58881688`. The kernel wrapper is 512 bytes and names KERNEL; its declared payload is 4,941,656 bytes. The boot ramdisk's declared gzip payload is 707,148 bytes; recovery's is 1,091,796. Lengths and gzip termination validate. The reference helper rejects RECOVERY by design; analysis therefore checked its actual wrapper magic/name/length separately and used only the existing cpio parser. This is not an image modification.

Both extracted wrapped kernels exactly match `OriginalFirmware/kernel` and `kernel_eastaeon82_wet_kk.bin`, SHA-256 `b026b2859e9bf977eca0b5f8506f67024143fbadb18e9c28c23f5e7b6a51118d`. Each ramdisk exactly matches its corresponding loose package file. Gzip begins at wrapped-kernel offset 19,172 and produces 10,544,388 bytes, SHA-256 `5f284fade5e9ab4f146ed9e7bd2464d1e1a4d04a8eafb43abb4a909738666355`. This exactly reproduces `reverse/ghidra/exports/y2-kernel-image.bin`, establishing that derivative's lineage from this stock kernel. The symbolized ELF's hash is verified separately; symbol recovery itself was not rerun.

## Address conflict: do not construct a boot artifact

Both headers record kernel load `0x10008000`, ramdisk load `0x11000000`, second-stage address `0x10f00000` despite zero second-stage size, and tags `0x10000100`. The current `/proc/iomem` instead reports System RAM at `0x80000000–0xbdffffff` plus `0xbf800000–0xbf9fffff`, kernel code at `0x80008000–0x80955fff`, and kernel data at `0x8098e000–0x80dc818f`.

These are distinct observed address domains, not interchangeable approved Linux load addresses. Whether the bootloader ignores/rewrites header fields, aliases memory, uses a vendor convention or takes another path remains UNKNOWN. The second reported RAM range and unreported gaps also require reservation analysis. Do not infer usable DRAM solely from MemTotal or copy header constants into a Linux image/DTS.

## Config and device-tree search limits

The exact whole boot/recovery images, wrapped kernels and independently decompressed kernel were searched for `IKCFG_ST`, `IKCFG_ED` and big-endian FDT magic `d00dfeed`: no occurrences were found in these scopes. The legacy parser's declared DT size is zero and there are no trailing bytes. Runtime exported config/DT paths are unavailable. `dtc` and `extract-ikconfig` are not on this host's PATH; no tools were installed and no DTS was authored.

Result: **no candidate found in the stated inputs/search**, not proof that a DT/config cannot exist in another artifact or vendor encoding. Bootloader-provided ATAGs and compiled board configuration remain hypotheses. Recover the bootloader handoff and board configuration as a later bounded research proof; never invent missing nodes or pin numbers.

## Ramdisk observations

Stock defaults set `ro.secure=1`, `ro.debuggable=0`, `ro.adb.secure=1`, and USB `mass_storage`. The current device reports `mass_storage,adb`; exact installed boot lineage is still unconfirmed. The package fstab describes `/emmc@usrdata` and writable protect filesystems. Init references WMT/connection loaders and `/system/etc/firmware/`. The `service console /system/bin/sh` entry is disabled and its start trigger requires `ro.debuggable=1`, unlike the observed 0. This does not establish a usable serial console, physical UART routing or safe access. The ttyMT2 connectivity comment is not a verified diagnostic console pinout.

## Review decision

Y2E-115 is complete as bounded structure/availability research. Exact stock kernel and ramdisk lineage is established; header/runtime address interpretation and missing config/DT evidence block Linux boot construction. Y2E-120 may now assess recovery capability using these findings. It may not execute a flasher, upload a download agent, change boot mode or read/write partition contents.
