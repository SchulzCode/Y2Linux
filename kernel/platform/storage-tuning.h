/* SPDX-License-Identifier: GPL-2.0-only */
/* MT6582 MSDC tuning geometry and the pure parts of SD tuning: pass-map
 * windows, margin-based selection, fault classification and pad-cell decoding.
 * No kernel dependency, so the real header is compiled by tests/.
 *
 * Hardware facts (MT6582 vendor mt6582/sd.c register dump and stock
 * msdc_set_sr/_smt/_rdtdsel/_driving/_pin_pud disassembly, retained under
 * evidence-private/):
 * - MSDC_PAD_TUNE (0xec): DATWRDLY[4:0] DATRRDLY[12:8] CMDRDLY[20:16]
 *   CMDRRDLY[26:22] CLKTXDLY[31:27]. Every delay is a 5-bit, 32-tap, LINEAR
 *   delay line: tap 31 is not adjacent to tap 0. Stock never touches bits 13,
 *   15 or 21 (no RXDLYSEL/RD_SEL/CMD_SEL on this controller).
 * - Offset 0xf0/0xf4 is MSDC_DAT_RDDLY0/1 (per-lane read delay, selected by
 *   IOCON.DDLSEL). It is NOT a second PAD_TUNE register.
 * - SDC_DATCRC_STS (0x60): bits 7:0 rising-edge lane CRC, bits 15:8 falling.
 * - MSDC1 pad cells at GPIO 0x10005000: CLK 0xc40, CMD 0xc50, DAT 0xc60, PAD
 *   0xc70. Cell: drive[10:8] SR[12] SMT[13]; PAD cell: RDSEL[9:4] TDSEL[3:0].
 */
#ifndef Y2_STORAGE_TUNING_H
#define Y2_STORAGE_TUNING_H

#define Y2_TUNE_TAPS 32U
#define Y2_TUNE_TRIES 3U		/* a tap passes only if every try passes */
#define Y2_TUNE_MIN_WIDTH 4U		/* narrower than this is not an eye */
#define Y2_TUNE_CIRCULAR 0		/* linear delay line */
#define Y2_TUNE_EDGES 2U		/* 0 rising, 1 falling */
#define Y2_TUNE_MAX_WINDOWS 8U

/* Characterization interface (sysfs y2_lab on the SD host). Diagnostic
 * candidates only: production images build with 0. */
#ifndef Y2_MSDC_LAB
#define Y2_MSDC_LAB 0
#endif

#define Y2_MSDC_DATCRC_STS 0x60U
#define Y2_MSDC_MAIN_VER 0x100U
#define Y2_MSDC_ECO_VER 0x104U
#define Y2_MSDC_PATCH_BIT0_REG 0xb0U
#define Y2_MSDC_PATCH_BIT1_REG 0xb4U
#define Y2_MSDC_PAD_TUNE_RXDLYSEL (1U << 15)

struct y2_tune_window {
	unsigned char start;
	unsigned char len;
};

/* All contiguous passing runs of `map` (bit i = tap i passed), in tap order.
 * A circular line merges a run touching the last tap with one touching tap 0
 * (reported at the high start). Returns the number of windows found, which
 * may exceed `max`; only `max` are stored. */
static inline unsigned y2_tune_windows(unsigned map, unsigned taps, int circular,
				       struct y2_tune_window *out, unsigned max)
{
	unsigned count = 0, tap = 0, merge_lo = 0;
	unsigned long long full = taps >= 32 ? 0xffffffffULL : ((1ULL << taps) - 1);

	map &= (unsigned)full;
	if (!map)
		return 0;
	if (map == (unsigned)full) {
		if (max) {
			out[0].start = 0;
			out[0].len = (unsigned char)taps;
		}
		return 1;
	}
	if (circular && (map & 1U) && (map & (1U << (taps - 1)))) {
		while (merge_lo < taps && (map & (1U << merge_lo)))
			merge_lo++;
	}
	while (tap < taps) {
		unsigned start, len;

		if (!(map & (1U << tap))) {
			tap++;
			continue;
		}
		start = tap;
		while (tap < taps && (map & (1U << tap)))
			tap++;
		len = tap - start;
		if (circular && start == 0 && merge_lo && merge_lo < taps && (map & (1U << (taps - 1))))
			continue;		/* folded into the high-end run */
		if (circular && tap == taps && merge_lo && start != 0)
			len += merge_lo;
		if (count < max) {
			out[count].start = (unsigned char)start;
			out[count].len = (unsigned char)len;
		}
		count++;
	}
	return count;
}

struct y2_tune_choice {
	unsigned char valid;
	unsigned char edge;
	unsigned char delay;
	unsigned char start;
	unsigned char len;
	unsigned char margin_lo;	/* passing taps below the chosen delay */
	unsigned char margin_hi;	/* passing taps above it */
	unsigned char clipped_lo;	/* window touches tap 0: true eye may be wider */
	unsigned char clipped_hi;	/* window touches the last tap */
	unsigned char open;		/* every tap passed: this sweep proves nothing */
	unsigned char windows[Y2_TUNE_EDGES];
};

/* Widest window of one edge's map, or len 0. */
static inline struct y2_tune_window y2_tune_widest(unsigned map, unsigned taps, int circular,
						   unsigned *windows)
{
	/* A 32-tap map can have 16 islands. Diagnostic output is capped at
	 * eight, but that output budget must never truncate the selection. */
	struct y2_tune_window list[Y2_TUNE_TAPS / 2], best = { 0, 0 };
	unsigned capacity = sizeof(list) / sizeof(list[0]);
	unsigned found = y2_tune_windows(map, taps, circular, list, capacity);
	unsigned i, stored = found < capacity ? found : capacity;

	for (i = 0; i < stored; i++)
		if (list[i].len > best.len)
			best = list[i];
	if (windows)
		*windows = found;
	return best;
}

/* Pick the delay with the largest timing eye over the scanned edges.
 *
 *   widest passing window per edge -> reject eyes below `min_width`
 *   -> prefer the wider window, then the larger worst-side margin, then the
 *   rising edge -> centre of that window.
 *
 * The centre of a run of n taps starting at s is s + (n - 1) / 2, so the
 * margins differ by at most one tap. A window clipped by the end of the line
 * is a lower bound on the real eye; an `open` sweep (all taps pass) selects
 * the middle of the line and is reported as carrying no information. */
static inline struct y2_tune_choice y2_tune_choose(const unsigned map[Y2_TUNE_EDGES],
						   unsigned edge_mask, unsigned taps,
						   int circular, unsigned min_width)
{
	struct y2_tune_choice best = { 0 };
	unsigned edge, best_score = 0;

	for (edge = 0; edge < Y2_TUNE_EDGES; edge++) {
		struct y2_tune_window w;
		unsigned windows = 0, lo, hi, score;

		if (!(edge_mask & (1U << edge)))
			continue;
		w = y2_tune_widest(map[edge], taps, circular, &windows);
		best.windows[edge] = (unsigned char)(windows > 255 ? 255 : windows);
		if (w.len < min_width)
			continue;
		lo = (w.len - 1) / 2;
		hi = w.len - 1 - lo;
		/* width dominates, then the worse margin */
		score = ((unsigned)w.len << 8) | (lo < hi ? lo : hi);
		if (score > best_score) {
			best_score = score;
			best.valid = 1;
			best.edge = (unsigned char)edge;
			best.start = w.start;
			best.len = w.len;
			best.margin_lo = (unsigned char)lo;
			best.margin_hi = (unsigned char)hi;
			best.delay = (unsigned char)((w.start + lo) % taps);
			best.clipped_lo = w.start == 0;
			best.clipped_hi = (unsigned)w.start + w.len >= taps;
			best.open = w.len >= taps;
		}
	}
	return best;
}

/* What mainline get_best_delay() would have picked from one map: the middle
 * of the first-longest run, or len/3 when the run starts at tap 0 (it biases
 * toward the smallest delay). Kept only to compare the old algorithm against
 * the recorded map. 0xff = no passing tap. */
static inline unsigned y2_tune_legacy_phase(unsigned map, unsigned taps)
{
	struct y2_tune_window w = y2_tune_widest(map, taps, 0, 0);

	if (!w.len)
		return 0xff;
	return w.start == 0 ? w.len / 3 : (w.start + w.len / 2) % taps;
}

/* ---- Fault classification ------------------------------------------------ */

enum y2_fault_kind {
	Y2_FAULT_NONE,
	Y2_FAULT_CMD_CRC,
	Y2_FAULT_CMD_TIMEOUT,
	Y2_FAULT_DATA_CRC,
	Y2_FAULT_DATA_TIMEOUT,
	Y2_FAULT_STOP,		/* CMD12/CMD23 style companion command failed */
	Y2_FAULT_OTHER,
	Y2_FAULT_KINDS,
};
static const char *const y2_fault_kind_names[Y2_FAULT_KINDS] = {
	"none", "cmd_crc", "cmd_timeout", "data_crc", "data_timeout", "stop", "other",
};

/* Which part of one request failed first. The command is judged before its
 * data stage, then the stop/sbc companions. -EILSEQ is a CRC error and
 * -ETIMEDOUT a timeout; anything else non-zero is "other". */
static inline enum y2_fault_kind y2_fault_kind(int sbc_error, int cmd_error,
					       int data_error, int stop_error)
{
	if (cmd_error == -84)			/* -EILSEQ */
		return Y2_FAULT_CMD_CRC;
	if (cmd_error == -110)			/* -ETIMEDOUT */
		return Y2_FAULT_CMD_TIMEOUT;
	if (data_error == -84)
		return Y2_FAULT_DATA_CRC;
	if (data_error == -110)
		return Y2_FAULT_DATA_TIMEOUT;
	if (sbc_error || stop_error)
		return Y2_FAULT_STOP;
	if (cmd_error || data_error)
		return Y2_FAULT_OTHER;
	return Y2_FAULT_NONE;
}

/* Cumulative fault counters plus the first and the most recent failure. */
struct y2_fault_rec {
	unsigned seq;
	unsigned long long uptime_ms;
	unsigned char kind, opcode, dir, level;
	unsigned blksz, blocks, arg;
	unsigned irq_events, dcrc, timing;
	unsigned pad_tune, iocon, patch0;
};

struct y2_fault_log {
	unsigned count[Y2_FAULT_KINDS];
	unsigned lane_pos[8], lane_neg[8];
	unsigned total;
	unsigned char have_first;
	struct y2_fault_rec first, last;
};

/* SDC_DATCRC_STS lanes: rising-edge bits 7:0, falling-edge bits 15:8. */
static inline unsigned y2_dcrc_pos_lanes(unsigned sts)
{
	return sts & 0xffU;
}
static inline unsigned y2_dcrc_neg_lanes(unsigned sts)
{
	return (sts >> 8) & 0xffU;
}

static inline void y2_fault_account(struct y2_fault_log *log, const struct y2_fault_rec *rec)
{
	unsigned lane;

	if (rec->kind == Y2_FAULT_NONE || rec->kind >= Y2_FAULT_KINDS)
		return;
	log->count[rec->kind]++;
	log->total++;
	if (rec->kind == Y2_FAULT_DATA_CRC) {
		for (lane = 0; lane < 8; lane++) {
			if (y2_dcrc_pos_lanes(rec->dcrc) & (1U << lane))
				log->lane_pos[lane]++;
			if (y2_dcrc_neg_lanes(rec->dcrc) & (1U << lane))
				log->lane_neg[lane]++;
		}
	}
	if (!log->have_first) {
		log->first = *rec;
		log->first.seq = log->total;
		log->have_first = 1;
	}
	log->last = *rec;
	log->last.seq = log->total;
}

/* ---- Pad cell decoding ---------------------------------------------------- */

static inline unsigned y2_pad_cell_drive(unsigned v) { return (v >> 8) & 7U; }
static inline unsigned y2_pad_cell_slew(unsigned v) { return (v >> 12) & 1U; }
static inline unsigned y2_pad_cell_smt(unsigned v) { return (v >> 13) & 1U; }
static inline unsigned y2_pad_cell_pull(unsigned v) { return v & 0xffU; }
static inline unsigned y2_pad_rdsel(unsigned v) { return (v >> 4) & 0x3fU; }
static inline unsigned y2_pad_tdsel(unsigned v) { return v & 0xfU; }

/* RXDLYSEL (PAD_TUNE bit 15) exists only if a write of 1 reads back as 1. */
static inline int y2_rxdlysel_supported(unsigned writable_mask)
{
	return !!(writable_mask & Y2_MSDC_PAD_TUNE_RXDLYSEL);
}

/* Fields of one PAD_TUNE value, for status and tests. */
static inline unsigned y2_pad_tune_datwr(unsigned v) { return v & 0x1fU; }
static inline unsigned y2_pad_tune_datrd(unsigned v) { return (v >> 8) & 0x1fU; }
static inline unsigned y2_pad_tune_cmdrd(unsigned v) { return (v >> 16) & 0x1fU; }
static inline unsigned y2_pad_tune_cmdrr(unsigned v) { return (v >> 22) & 0x1fU; }
static inline unsigned y2_pad_tune_clktx(unsigned v) { return (v >> 27) & 0x1fU; }

/* PATCH_BIT0 latch/clock-generator fields the vendor tuning sweeps above
 * 100 MHz: INT_DAT_LATCH_CK_SEL[9:7] and CKGEN_MSDC_DLY_SEL[14:10]. */
static inline unsigned y2_patch0_latch_ck(unsigned v) { return (v >> 7) & 7U; }
static inline unsigned y2_patch0_ckgen_dly(unsigned v) { return (v >> 10) & 0x1fU; }
/* PATCH_BIT1: WRDAT_CRCS[2:0], CMD_RSP[5:3]. */
static inline unsigned y2_patch1_wrdat_crcs(unsigned v) { return v & 7U; }
static inline unsigned y2_patch1_cmd_rsp(unsigned v) { return (v >> 3) & 7U; }

/* ---- Recorded tuning sweep ------------------------------------------------ */

struct y2_tune_phase {
	unsigned all[Y2_TUNE_EDGES];	/* tap passed on every try */
	unsigned any[Y2_TUNE_EDGES];	/* tap passed on at least one try */
	unsigned scanned;		/* bit e: edge e swept */
	struct y2_tune_choice pick;
	unsigned char legacy[Y2_TUNE_EDGES];
};

struct y2_tune_log {
	unsigned char valid;
	int result;
	struct y2_tune_phase cmd, cmd_int, data;
	unsigned iocon, pad_tune, rddly0, rddly1, patch0, patch1, patch2;
	unsigned timing;
};

/* Last characterization scan (sysfs y2_lab): every tap, both edges. */
struct y2_lab_scan {
	struct y2_tune_phase cmd, data;
	unsigned latch, ckgen, tries;
	unsigned char valid;
};

static inline void y2_tune_phase_choose(struct y2_tune_phase *p, unsigned taps)
{
	unsigned e;

	p->pick = y2_tune_choose(p->all, p->scanned, taps, Y2_TUNE_CIRCULAR, Y2_TUNE_MIN_WIDTH);
	for (e = 0; e < Y2_TUNE_EDGES; e++)
		p->legacy[e] = (unsigned char)y2_tune_legacy_phase(p->all[e], taps);
}

#endif
