/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_STORAGE_LAYOUT_H
#define Y2_STORAGE_LAYOUT_H

/* Stock FM mmcblk0 is a window in native EMMC_USER. Verified by the own
 * stock kernel and owner SPFT readback; see storage06-addressing-correction.md.
 * The legacy DA concatenates 8.5 MiB of non-user hardware areas before USER;
 * its MBR at 20 MiB is therefore at native USER 11.5 MiB, not 20 MiB.
 * Keep real EXT_CSD untouched. These constants describe this board/layout only.
 */
#define Y2_EMMC_RAW_SECTORS 15269888U
#define Y2_EMMC_USER_OFFSET 23552U
#define Y2_EMMC_TAIL_SECTORS 43008U
#define Y2_EMMC_DISK_SECTORS (Y2_EMMC_RAW_SECTORS - Y2_EMMC_USER_OFFSET - Y2_EMMC_TAIL_SECTORS)

static inline unsigned y2_emmc_raw_sectors(const unsigned char *ext)
{
	return (unsigned)ext[212] | (unsigned)ext[213] << 8 |
	       (unsigned)ext[214] << 16 | (unsigned)ext[215] << 24;
}

static inline int y2_emmc_layout_valid(const unsigned char *ext)
{
	unsigned i;

	/* BOOT_SIZE_MULT=32: 4 MiB each; RPMB_SIZE_MULT=4: 512 KiB.
	 * No GP partitions: any other geometry needs independent qualification.
	 */
	if (y2_emmc_raw_sectors(ext) != Y2_EMMC_RAW_SECTORS ||
	    ext[226] != 32 || ext[168] != 4)
		return 0;
	for (i = 143; i < 155; i++)
		if (ext[i])
			return 0;
	return 1;
}

/* Only after the original logical request passed storage-policy.h. Do not
 * mutate cmd->arg: the MMC core may retry it, and SBC/STOP are not addresses.
 */
static inline unsigned y2_emmc_wire_arg(unsigned opcode, unsigned arg)
{
	switch (opcode) {
	case 17: case 18: case 24: case 25:
		return arg + Y2_EMMC_USER_OFFSET;
	default:
		return arg;
	}
}
#endif
