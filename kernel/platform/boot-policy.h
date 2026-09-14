/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_BOOT_POLICY_H
#define Y2_BOOT_POLICY_H
/* The stock Y2 LK passes ATAG_BOOT, plus boot_reason in ATAG_CMDLINE.
 * Parse only this metadata. Never import loader memory, initrd or command-line
 * policy. The input is the already reserved first 16 KiB of board RAM.
 */
struct y2_boot_info { int mode, reason, valid; };

static inline unsigned y2_boot_word(const unsigned char *p)
{
	return p[0] | (unsigned)p[1] << 8 | (unsigned)p[2] << 16 | (unsigned)p[3] << 24;
}

static inline struct y2_boot_info y2_boot_parse(const unsigned char *p, unsigned bytes)
{
	struct y2_boot_info info = { -1, -1, 0 };
	unsigned words, tag, size, n, i;
	int command_seen = 0, end = 0;
	const char key[] = "boot_reason=";
	/* LK 0x81e17ff0 emits the empty two-word CORE form, not five words. */
	if (bytes < 8 || y2_boot_word(p) != 2 || y2_boot_word(p + 4) != 0x54410001)
		return info;
	while (bytes >= 8) {
		words = y2_boot_word(p); tag = y2_boot_word(p + 4);
		if (!words && !tag) { end = 1; break; }
		if (words < 2 || words > bytes / 4) return info;
		size = words * 4;
		if (tag == 0x41000802) {
			if (size != 12 || info.mode != -1 || y2_boot_word(p + 8) > 9) return info;
			info.mode = y2_boot_word(p + 8);
		} else if (tag == 0x54410009) {
			if (command_seen++) return info;
			/* NUL must be inside the tag; no read beyond the supplied area. */
			for (n = 8; n < size && p[n]; n++) { }
			if (n == size) return info;
			for (i = 8; i < n; i++) {
				unsigned j;
				if (i != 8 && p[i - 1] != ' ') continue;
				for (j = 0; j < sizeof(key) - 1 && i + j < n && p[i + j] == key[j]; j++) { }
				if (j != sizeof(key) - 1) continue;
				if (info.reason != -1 || i + j >= n || p[i + j] < '0' || p[i + j] > '7' ||
				    (i + j + 1 < n && p[i + j + 1] != ' ')) return info;
				info.reason = p[i + j] - '0';
			}
		}
		p += size; bytes -= size;
	}
	info.valid = end && info.mode >= 0 && info.reason >= 0;
	return info;
}

static inline int y2_boot_offline(const struct y2_boot_info *info)
{
	return info->valid && (info->mode == 8 || info->mode == 9);
}
#endif
