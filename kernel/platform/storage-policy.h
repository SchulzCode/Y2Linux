/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_STORAGE_POLICY_H
#define Y2_STORAGE_POLICY_H
/* Stock EMMC_USER sectors, reconciled against scatter + physical Android sysfs.
 * This guard executes BEFORE DMA. BOOTIMG is intentionally not writable by the
 * running v1 system; a future rescue updater requires a separate reviewed guard.
 */
static inline int y2_emmc_write_range(unsigned sector, unsigned blocks)
{
	unsigned end;
	if (!blocks || blocks > 0xffffffffU - sector)
		return 0;
	end = sector + blocks;
	return (sector >= 166912 && end <= 1846272) ||
	       (sector >= 2104320 && end <= 3742720);
}
static inline int y2_emmc_command_allowed(unsigned op, unsigned arg,
		unsigned write, unsigned blocks, unsigned blksz,
		unsigned sbc, unsigned user_card)
{
	unsigned index, value;
	if (sbc)
		return 0;
	if (write)
		return user_card && (op == 24 || op == 25) && blksz == 512 &&
		       y2_emmc_write_range(arg, blocks);
	if (op == 6) {
		/* WRITE_BYTE, normal command set; only reviewed volatile controls.
		 * PARTITION_CONFIG, boot setup, RPMB, sanitize, erase, write protect,
		 * vendor/general commands and firmware update are never allowed.
		 * LK must leave EMMC_USER selected; otherwise initialization fails.
		 */
		if ((arg & 0xff0000ffU) != 0x03000001U)
			return 0;
		index = (arg >> 16) & 255;
		value = (arg >> 8) & 255;
		switch (index) {
		case 32: return value == 1; /* FLUSH_CACHE */
		case 33: return value <= 1; /* CACHE_CTRL */
		case 34: return value >= 1 && value <= 3; /* power notification */
		case 161: return value == 1; /* HPI enable */
		case 175: return value == 1; /* ERASE_GROUP_DEF, not an erase */
		case 183: return value == 0; /* one-bit legacy bus */
		case 185: return value == 0; /* legacy timing only */
		default: return 0;
		}
	}
	switch (op) {
	case 0: case 8: return arg == 0;
	case 16: return arg == 512;
	case 1: case 2: case 3: case 7:
	case 9: case 10: case 12: case 13: case 17: case 18:
		return 1;
	default:
		return 0;
	}
}
#endif
