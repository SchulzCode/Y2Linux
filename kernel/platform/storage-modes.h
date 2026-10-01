/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_STORAGE_MODES_H
#define Y2_STORAGE_MODES_H
/* Y2 MSDC bus-mode ceiling, negotiation ladder and fallback policy.
 *
 * Evidence (docs/validation/Y2-STORAGE-CEILING.md):
 * - Hash-identified stock Y2 kernel board data: msdc0 flags 0x3c0 and msdc1
 *   flags 0x3e3 both carry MSDC_HIGHSPEED|MSDC_UHS1|MSDC_DDR; msdc1 adds
 *   dedicated 1.8 V drive strengths. Stock source maps UHS1 to SDR50/SDR104
 *   and the HS200 capability, DDR to DDR50/1.8 V DDR.
 * - Stock msdc_sd_power_switch (0xc04f2b60) sets MT6323 VMC to 1800 mV for
 *   MSDC1, then Schmitt/RDSEL-TDSEL and the 1.8 V drive table. Stock eMMC
 *   power only owns VEMC_3V3 (card VCC); MSDC0 IO is the fixed VIO18 rail,
 *   so the stock voltage switch returns success without a change for eMMC.
 * - MSDC source: LK selector 1, MSDCPLL 400 MHz / 2 = 200 MHz. Undivided
 *   mode gives 200 MHz, /2 gives 100 MHz, DDR uses source/4 = 50 MHz.
 * - The installed eMMC reports EXT_CSD card type 0x57: HS26, HS52, DDR52
 *   (1.8/3 V), HS200 (1.8 V) and HS400 (1.8 V). MT6582 MSDC has no data
 *   strobe / EMMC50 block, so HS400 is a card-only capability.
 * Every level admits only modes the top level admits, never a faster clock
 * than the level above, except that SD tries SDR50 (100 MHz SDR) after the
 * core's preferred DDR50 (50 MHz DDR) fails. Fallback only moves down the
 * ladder; nothing here touches voltage, partitions or card contents. */
#define Y2_MSDC_DAT_RDDLY0 0xf0	/* MT6582 read-data delay (tuned) */

#define Y2_CAP_HS	(1U << 0)	/* SD High Speed / eMMC HS52 SDR */
#define Y2_CAP_DDR	(1U << 1)	/* SD DDR50 / eMMC DDR52 (1.8 V IO) */
#define Y2_CAP_SDR50	(1U << 2)	/* SD UHS-I SDR50 (100 MHz) */
#define Y2_CAP_TOP	(1U << 3)	/* SD SDR104 / eMMC HS200 (200 MHz, tuned) */
#define Y2_CAP_UHS	(1U << 4)	/* SD 1.8 V signalling (UHS-I) */

struct y2_storage_level {
	const char *name;
	unsigned caps;
	unsigned clock_hz;
};

#define Y2_EMMC_LEVELS 5
static const struct y2_storage_level y2_emmc_levels[Y2_EMMC_LEVELS] = {
	{ "HS200", Y2_CAP_HS | Y2_CAP_DDR | Y2_CAP_TOP, 200000000 },
	{ "DDR52", Y2_CAP_HS | Y2_CAP_DDR, 50000000 },
	{ "HS52", Y2_CAP_HS, 50000000 },
	{ "HS-25MHz", Y2_CAP_HS, 25000000 },
	{ "legacy-13MHz", Y2_CAP_HS, 13000000 },
};

/* SD core preference is SDR104 > DDR50 > SDR50 > SDR25; the ladder follows it
 * and leaves 1.8 V signalling entirely once every UHS mode has failed. */
#define Y2_SD_LEVELS 6
static const struct y2_storage_level y2_sd_levels[Y2_SD_LEVELS] = {
	{ "SDR104", Y2_CAP_HS | Y2_CAP_UHS | Y2_CAP_DDR | Y2_CAP_SDR50 | Y2_CAP_TOP, 200000000 },
	{ "DDR50", Y2_CAP_HS | Y2_CAP_UHS | Y2_CAP_DDR, 50000000 },
	{ "SDR50", Y2_CAP_HS | Y2_CAP_UHS | Y2_CAP_SDR50, 100000000 },
	{ "HS", Y2_CAP_HS, 50000000 },
	{ "HS-25MHz", Y2_CAP_HS, 25000000 },
	{ "legacy-13MHz", Y2_CAP_HS, 13000000 },
};

enum y2_storage_event {
	Y2_EVENT_CRC,
	Y2_EVENT_TIMEOUT,
	Y2_EVENT_CONTROLLER,
	Y2_EVENT_TUNING,
	Y2_EVENT_VOLTAGE,
	Y2_EVENT_VERIFY,
	Y2_EVENT_COUNT,
};
static const char *const y2_storage_event_names[Y2_EVENT_COUNT] = {
	"crc", "timeout", "controller", "tuning", "voltage", "verify",
};

static inline const struct y2_storage_level *y2_storage_ladder(int sd, unsigned *count)
{
	*count = sd ? Y2_SD_LEVELS : Y2_EMMC_LEVELS;
	return sd ? y2_sd_levels : y2_emmc_levels;
}

/* The next level after a failure at `level`. A failed 1.8 V switch makes
 * every remaining UHS level pointless, so SD skips straight to 3.3 V HS. */
static inline unsigned y2_storage_fallback(int sd, unsigned level, enum y2_storage_event event)
{
	unsigned count, next = level + 1;
	const struct y2_storage_level *ladder = y2_storage_ladder(sd, &count);
	if (level >= count - 1)
		return count - 1;
	if (sd && event == Y2_EVENT_VOLTAGE)
		while (next < count - 1 && (ladder[next].caps & Y2_CAP_UHS))
			next++;
	return next;
}

/* Boot ceiling names accepted on the kernel command line. Unknown names keep
 * the source-backed maximum; "safe" is handled by y2.mmc_safe. */
static inline int y2_storage_level_named(int sd, const char *name)
{
	unsigned i, count;
	const struct y2_storage_level *ladder = y2_storage_ladder(sd, &count);
	for (i = 0; i < count; i++) {
		const char *a = ladder[i].name, *b = name;
		while (*a && *a == *b) { a++; b++; }
		if (!*a && (!*b || *b == '\n'))
			return (int)i;
	}
	return -1;
}

/* Highest ladder level the installed eMMC itself supports (EXT_CSD[196]).
 * HS400 (bit 6) is reported separately; the MT6582 host cannot use it. */
static inline unsigned y2_emmc_card_level(unsigned char card_type)
{
	if (card_type & 0x10)		/* HS200 1.8 V */
		return 0;
	if (card_type & 0x04)		/* DDR52 1.8/3 V */
		return 1;
	if (card_type & 0x02)		/* HS52 */
		return 2;
	return 3;			/* HS26 or backward-compatible */
}

/* Highest ladder level an SD card supports: SD 3.0 switch-function bus modes
 * (bit0 SDR12, 1 SDR25, 2 SDR50, 3 SDR104, 4 DDR50) and its 1.8 V acceptance. */
static inline unsigned y2_sd_card_level(unsigned bus_modes, int accepts_1v8, int high_speed)
{
	if (accepts_1v8 && (bus_modes & 0x08))
		return 0;
	if (accepts_1v8 && (bus_modes & 0x10))
		return 1;
	if (accepts_1v8 && (bus_modes & 0x04))
		return 2;
	return high_speed ? 3 : 4;
}

/* EXT_CSD bytes the MMC core itself treats as read-only identity
 * (mmc_compare_ext_csds), plus SEC_COUNT. A readback in the negotiated mode
 * must return exactly the identification copy. */
static const unsigned short y2_emmc_identity_bytes[] = {
	160, 181, 192, 194, 196, 212, 213, 214, 215, 217, 221, 223, 224, 229, 230, 231, 232,
};
static inline int y2_emmc_identity_matches(const unsigned char *a, const unsigned char *b)
{
	unsigned i;
	for (i = 0; i < sizeof(y2_emmc_identity_bytes) / sizeof(y2_emmc_identity_bytes[0]); i++)
		if (a[y2_emmc_identity_bytes[i]] != b[y2_emmc_identity_bytes[i]])
			return 0;
	return 1;
}

/* Stock board pad drive (GPIO 0x10005000, bits 10:8 of each cell), from the
 * hash-identified stock msdc_hw records and msdc_set_driving (0xc04f28c0):
 * msdc0 clk/cmd/dat 4/2/2; msdc1 7/7/7 at 3.3 V and 4/4/4 at 1.8 V. */
#define Y2_MSDC_DRIVE_SHIFT 8
#define Y2_MSDC_DRIVE_MASK (7U << Y2_MSDC_DRIVE_SHIFT)
struct y2_msdc_drive {
	unsigned short offset[3];	/* clk, cmd, dat */
	unsigned char v33[3], v18[3];
};
static const struct y2_msdc_drive y2_msdc_drives[2] = {
	{ { 0xc00, 0xc10, 0xc20 }, { 4, 2, 2 }, { 4, 2, 2 } },
	{ { 0xc40, 0xc50, 0xc60 }, { 7, 7, 7 }, { 4, 4, 4 } },
};
static inline unsigned y2_msdc_drive_value(unsigned old, unsigned drive)
{
	return (old & ~Y2_MSDC_DRIVE_MASK) | ((drive & 7U) << Y2_MSDC_DRIVE_SHIFT);
}
#ifdef __KERNEL__
/* drivers/y2/pinctrl.c: the GPIO block owner programs the MSDC pad cells. */
int y2_msdc_pad_drive(unsigned id, bool v18, unsigned *before);
#endif
#endif
