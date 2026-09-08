# Linux 6.18 research boundary

Date: 2026-09-08. Status: chosen upstream baseline, board-support audit not yet completed. Related tasks: Y2E-115/120.

Linux 6.18 is listed as longterm, with projected end of life December 2028, on the [official release page](https://www.kernel.org/category/releases.html), retrieved this date. That designation does not imply complete MT6582 or Y2 support. No kernel source revision, toolchain, configuration, DTS or patch set is selected by this document.

The pinned [Linux v6.18 ARM boot protocol](https://github.com/torvalds/linux/blob/v6.18/Documentation/arch/arm/booting.rst) requires an explicit loader handoff, including initialized memory and the physical location of boot data. The stock header/runtime address discrepancy in [boot-chain.md](boot-chain.md) must be explained before choosing image load addresses, memory reservations or a DT handoff. ATAG versus DT is an evidence question here, not a reason to manufacture a tree.

The [v6.18 stable block sysfs ABI](https://github.com/torvalds/linux/blob/v6.18/Documentation/ABI/stable/sysfs-block) distinguishes logical/physical block sizes and documents sector-count units for interfaces such as chunk_sectors. Our partition report retains raw counts and byte conversions and independently cross-checks the vendor kernel's `/proc/emmc` output. Upstream conventions do not erase this vendor's special preloader/EBR/FAT/BMTPOOL cases.

The web connector returned an expired-token error; official references were retrieved directly on the host and hashed privately. The initially tried testing-ABI URL returned 404; the stable ABI reference is the retrieved source. No unsupported technical claim is based on that failed fetch.

Deferred support audit: CPU/timer/GIC/memory/clock/reset/pinctrl, storage/USB/console, DRM/panel/input, ASoC/DMA/codec, power and connectivity. Match exact v6.18 source and bindings to established device evidence before defining a minimal delta. Existing modern driver names or sibling-SoC support will not be treated as a working board implementation.
