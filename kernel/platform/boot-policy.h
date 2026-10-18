/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_BOOT_POLICY_H
#define Y2_BOOT_POLICY_H
/* The stock Y2 LK passes ATAG_BOOT, plus boot_reason in ATAG_CMDLINE.
 * Parse only this metadata. Never import loader memory, initrd or command-line
 * policy. The input is the already reserved first 16 KiB of board RAM.
 */
/* panel: from LK's lcm=<digit>-<name> (fm-lk 0x81e0af60, list [gc9503v,
 * st7701]): the name is the first driver whose lcm_compare_id matched, else the
 * list's last one (st7701); the digit is a separate DCS 0x0A read. GC9503V's
 * compare (0x81e1eb60) matches only DCS 0x04 = xx 95 03/04, so its name is a
 * match with either digit; st7701 is trusted only with 1, as before (#45).
 * 0 unknown, 1 GC9503V (gc9503v_hvga_dsi_vdo_hsd), 2 ST7701S
 * (st7701_hvga_dsi_vdo_boe), 3 another name. Informational: it never affects
 * validity. */
struct y2_boot_info { int mode, reason, valid, panel; char lcm[48]; };
#define Y2_PANEL_UNKNOWN 0
#define Y2_PANEL_GC9503V 1
#define Y2_PANEL_ST7701S 2
#define Y2_PANEL_OTHER 3

static inline int y2_boot_prefix(const unsigned char *p, unsigned at, unsigned end, const char *word)
{
	unsigned k;
	for (k = 0; word[k]; k++)
		if (at + k >= end || p[at + k] != (unsigned char)word[k]) return 0;
	return 1;
}

static inline unsigned y2_boot_word(const unsigned char *p)
{
	return p[0] | (unsigned)p[1] << 8 | (unsigned)p[2] << 16 | (unsigned)p[3] << 24;
}

static inline struct y2_boot_info y2_boot_parse(const unsigned char *p, unsigned bytes)
{
	struct y2_boot_info info = { -1, -1, 0, Y2_PANEL_UNKNOWN, "" };
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
				/* The raw lcm= token (bounded, printable), for diagnostics. */
				if (y2_boot_prefix(p, i, n, "lcm=")) {
					unsigned k;
					for (k = 0; k < sizeof(info.lcm) - 1 && i + k < n && p[i + k] != ' '; k++)
						info.lcm[k] = p[i + k] >= 0x20 && p[i + k] < 0x7f ? p[i + k] : '?';
					info.lcm[k] = 0;
				}
				/* See struct y2_boot_info: gc9503v with either digit, the
				 * rest only with lcm=1-. */
				if (y2_boot_prefix(p, i, n, "lcm=0-gc9503v") ||
				    y2_boot_prefix(p, i, n, "lcm=1-gc9503v"))
					info.panel = Y2_PANEL_GC9503V;
				else if (y2_boot_prefix(p, i, n, "lcm=1-"))
					info.panel = y2_boot_prefix(p, i + 6, n, "st7701") ? Y2_PANEL_ST7701S :
						     Y2_PANEL_OTHER;
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
