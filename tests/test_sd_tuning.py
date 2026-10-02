"""SD SDR104 diagnostic candidate: pass-map windows, margin-based selection,
fault classification, pad readback and RXDLYSEL, exercised on the real
storage-tuning.h and on the real patched MSDC tuning/diagnostic code run
against a simulated controller."""
from pathlib import Path
import unittest

from tests.test_hardware_ceiling import source
from tests.test_storage_ceiling import STUBS, PLATFORM, between, run_c

ROOT = Path(__file__).resolve().parents[1]


def run_tuning_c(program):
    run_c('#include "storage-tuning.h"\n' + program)


class TuningWindows(unittest.TestCase):
    def test_pass_map_generation_and_window_detection(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 struct y2_tune_window w[Y2_TUNE_MAX_WINDOWS];
 assert(y2_tune_windows(0, 32, 0, w, 8) == 0);
 /* every tap passes: one window spanning the whole line */
 assert(y2_tune_windows(0xffffffffu, 32, 0, w, 8) == 1 && w[0].start == 0 && w[0].len == 32);
 /* one run in the middle */
 assert(y2_tune_windows(0x0001fff0u, 32, 0, w, 8) == 1 && w[0].start == 4 && w[0].len == 13);
 /* three islands, reported in tap order */
 unsigned n = y2_tune_windows(0x00f000cfu, 32, 0, w, 8);
 assert(n == 3 && w[0].start == 0 && w[0].len == 4 && w[1].start == 6 && w[1].len == 2 &&
        w[2].start == 20 && w[2].len == 4);
 /* more islands than storage: the count is still exact */
 assert(y2_tune_windows(0x55555555u, 32, 0, w, 3) == 16);
 return 0;
}''')

    def test_wrap_around_windows_exist_only_on_a_circular_line(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 struct y2_tune_window w[Y2_TUNE_MAX_WINDOWS];
 unsigned map = 0xf000000fu;          /* taps 28..31 and 0..3 */
 /* the MT6582 delay line is linear: tap 31 and tap 0 are not adjacent */
 assert(Y2_TUNE_CIRCULAR == 0);
 assert(y2_tune_windows(map, 32, 0, w, 8) == 2 && w[0].len == 4 && w[1].len == 4);
 /* a circular line would join them into one run of 8 starting at tap 28 */
 assert(y2_tune_windows(map, 32, 1, w, 8) == 1 && w[0].start == 28 && w[0].len == 8);
 const unsigned two[2] = { map, 0 };
 struct y2_tune_choice c = y2_tune_choose(two, 1, 32, 1, 4);
 assert(c.valid && c.start == 28 && c.len == 8 && c.delay == (28 + 3) % 32);
 c = y2_tune_choose(two, 1, 32, 0, 4);
 assert(c.valid && c.len == 4);
 return 0;
}''')

    def test_midpoint_selection_and_margins(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 unsigned map[2] = { 0x001fffe0u, 0 };           /* taps 5..20, rising only */
 struct y2_tune_choice c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.valid && c.edge == 0 && c.start == 5 && c.len == 16);
 assert(c.delay == 12 && c.margin_lo == 7 && c.margin_hi == 8);
 assert(c.delay - c.margin_lo == c.start && c.delay + c.margin_hi == c.start + c.len - 1);
 assert(!c.clipped_lo && !c.clipped_hi && !c.open);
 /* a window against the end of the line is a lower bound and says so */
 map[0] = 0xffffff00u;                               /* taps 8..31 */
 c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.valid && c.start == 8 && c.len == 24 && c.clipped_hi && !c.clipped_lo);
 /* the old algorithm would also take len/3 for a window starting at tap 0 */
 assert(y2_tune_legacy_phase(0xffffffffu, 32) == 10);
 assert(y2_tune_legacy_phase(0x0000ffffu, 32) == 5);
 assert(y2_tune_legacy_phase(0x00fff000u, 32) == 12 + 6);
 assert(y2_tune_legacy_phase(0, 32) == 0xff);
 return 0;
}''')

    def test_all_pass_sweep_is_reported_as_an_open_eye(self):
        # Candidate 2 landed on 21 = 64/3 for both command and data: a sweep
        # where no tap ever failed. The new selection names that condition.
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 const unsigned map[2] = { 0xffffffffu, 0xffffffffu };
 struct y2_tune_choice c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.valid && c.open && c.clipped_lo && c.clipped_hi && c.len == 32);
 assert(c.edge == 0 && c.delay == 15 && c.margin_lo == 15 && c.margin_hi == 16);
 return 0;
}''')

    def test_multiple_windows_choose_the_widest_and_narrow_eyes_are_rejected(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 unsigned map[2] = { 0x03fffc3cu, 0 };   /* taps 2..5 (4) and 10..25 (16) */
 struct y2_tune_choice c = y2_tune_choose(map, 1, 32, 0, 4);
 assert(c.valid && c.start == 10 && c.len == 16 && c.windows[0] == 2);
 map[0] = 0x00000070u;                   /* one 3-tap island */
 c = y2_tune_choose(map, 1, 32, 0, 4);
 assert(!c.valid);
 map[0] = 0x000000f0u;                   /* exactly the minimum */
 c = y2_tune_choose(map, 1, 32, 0, 4);
 assert(c.valid && c.len == 4 && c.delay == 5);
 map[0] = 0;
 assert(!y2_tune_choose(map, 3, 32, 0, 4).valid);
 return 0;
}''')

    def test_edge_selection_prefers_the_larger_eye_then_rising(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 unsigned map[2] = { 0x000003f0u, 0x01fff800u };   /* rise 6 taps, fall 14 taps */
 struct y2_tune_choice c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.valid && c.edge == 1 && c.len == 14 && c.start == 11);
 /* a scanned-edge mask is honoured */
 c = y2_tune_choose(map, 1, 32, 0, 4);
 assert(c.valid && c.edge == 0 && c.len == 6);
 /* equal eyes: rising wins */
 map[1] = map[0];
 c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.edge == 0);
 /* equal width: the better-centred margin wins */
 map[0] = 0x00000ff0u; map[1] = 0x00ff0000u;
 c = y2_tune_choose(map, 3, 32, 0, 4);
 assert(c.len == 8 && c.edge == 0);
 return 0;
}''')


class FaultsAndPads(unittest.TestCase):
    def test_crc_classification_separates_command_data_and_companions(self):
        run_tuning_c(r'''
#include <assert.h>
#include <errno.h>
int main(void) {
 assert(y2_fault_kind(0, 0, 0, 0) == Y2_FAULT_NONE);
 assert(y2_fault_kind(0, -EILSEQ, 0, 0) == Y2_FAULT_CMD_CRC);
 assert(y2_fault_kind(0, -ETIMEDOUT, 0, 0) == Y2_FAULT_CMD_TIMEOUT);
 assert(y2_fault_kind(0, 0, -EILSEQ, 0) == Y2_FAULT_DATA_CRC);
 assert(y2_fault_kind(0, 0, -ETIMEDOUT, 0) == Y2_FAULT_DATA_TIMEOUT);
 assert(y2_fault_kind(0, -EILSEQ, -EILSEQ, 0) == Y2_FAULT_CMD_CRC);   /* command first */
 assert(y2_fault_kind(-EILSEQ, 0, 0, 0) == Y2_FAULT_STOP);
 assert(y2_fault_kind(0, 0, 0, -ETIMEDOUT) == Y2_FAULT_STOP);
 assert(y2_fault_kind(0, -EIO, 0, 0) == Y2_FAULT_OTHER);
 assert(y2_fault_kind(0, 0, -EIO, 0) == Y2_FAULT_OTHER);
 assert(y2_dcrc_pos_lanes(0x0a05) == 0x05 && y2_dcrc_neg_lanes(0x0a05) == 0x0a);
 return 0;
}''')

    def test_fault_log_keeps_counters_lanes_first_and_last_failure(self):
        run_tuning_c(r'''
#include <assert.h>
#include <string.h>
int main(void) {
 struct y2_fault_log log; memset(&log, 0, sizeof(log));
 struct y2_fault_rec a = { .kind = Y2_FAULT_DATA_CRC, .opcode = 18, .dcrc = 0x0005, .arg = 100 };
 struct y2_fault_rec b = { .kind = Y2_FAULT_CMD_CRC, .opcode = 18, .arg = 200 };
 struct y2_fault_rec c = { .kind = Y2_FAULT_DATA_CRC, .opcode = 18, .dcrc = 0x0201, .arg = 300 };
 struct y2_fault_rec none = { .kind = Y2_FAULT_NONE };
 y2_fault_account(&log, &none);
 assert(log.total == 0 && !log.have_first);
 y2_fault_account(&log, &a); y2_fault_account(&log, &b); y2_fault_account(&log, &c);
 assert(log.total == 3 && log.count[Y2_FAULT_DATA_CRC] == 2 && log.count[Y2_FAULT_CMD_CRC] == 1);
 assert(log.first.arg == 100 && log.first.seq == 1 && log.last.arg == 300 && log.last.seq == 3);
 assert(log.lane_pos[0] == 2 && log.lane_pos[2] == 1 && log.lane_neg[1] == 1 && log.lane_neg[0] == 0);
 /* a command CRC never lands in the data-lane histogram */
 assert(log.lane_pos[1] == 0);
 return 0;
}''')

    def test_pad_cell_decoding_matches_the_stock_register_layout(self):
        run_tuning_c(r'''
#include <assert.h>
#include "storage-modes.h"
int main(void) {
 /* msdc_set_driving [10:8], msdc_set_sr [12], msdc_set_smt [13], rdtdsel [9:4]/[3:0] */
 assert(y2_pad_cell_drive(0x2400) == 4 && y2_pad_cell_smt(0x2400) == 1 && y2_pad_cell_slew(0x2400) == 0);
 assert(y2_pad_cell_slew(0x1100) == 1 && y2_pad_cell_drive(0x1100) == 1);
 assert(y2_pad_cell_drive(y2_msdc_drive_value(0xffffffffu, 4)) == 4);
 assert(y2_pad_cell_pull(0x12c5) == 0xc5);
 assert(y2_pad_rdsel(0x3f3) == 0x3f && y2_pad_tdsel(0x3f3) == 3);
 assert(y2_msdc_pad_offsets[1][0] == 0xc40 && y2_msdc_pad_offsets[1][3] == 0xc70);
 assert(y2_msdc_pad_offsets[0][3] == 0xc30);
 /* PAD_TUNE fields as read back from candidate 2: 0x09951500 = 21/21/6/clk 1 */
 assert(y2_pad_tune_datrd(0x09951500u) == 21 && y2_pad_tune_cmdrd(0x09951500u) == 21);
 assert(y2_pad_tune_cmdrr(0x09951500u) == 6 && y2_pad_tune_clktx(0x09951500u) == 1);
 assert(y2_pad_tune_datrd(0x02900800u) == 8 && y2_pad_tune_clktx(0x02900800u) == 0 + 0);
 return 0;
}''')

    PINS = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#include "storage-modes.h"
#include "storage-tuning.h"
#define EPROBE_DEFER 517
struct y2_pins { char *base; int lock; };
static struct y2_pins *y2_pins_owner;
#define READ_ONCE(x) (x)
#define spin_lock_irqsave(l, f) ((void)(f))
#define spin_unlock_irqrestore(l, f) ((void)(f))
static unsigned readl(const void *p) { unsigned v; memcpy(&v, p, 4); return v; }
static void writel(unsigned v, void *p) { memcpy(p, &v, 4); }
'''

    def test_pad_cells_read_back_and_fields_set_through_the_real_pinctrl_code(self):
        text = (PLATFORM / 'pinctrl.c').read_text()
        body = between(text, 'int y2_msdc_pad_cells(', 'static int pins_probe(')
        run_c(self.PINS + body + r'''
int main(void) {
 static char mmio[0x1000]; struct y2_pins pins = { mmio, 0 };
 unsigned cells[4];
 assert(y2_msdc_pad_cells(1, cells) == -ENODEV);          /* no pin owner yet */
 y2_pins_owner = &pins;
 writel(0x00000100, mmio + 0xc40); writel(0x00000100, mmio + 0xc50);
 writel(0x00000100, mmio + 0xc60); writel(0x00000033, mmio + 0xc70);
 assert(y2_msdc_pad_cells(1, cells) == 0 && cells[0] == 0x100 && cells[3] == 0x33);
 assert(y2_msdc_pad_cells(2, cells) == -EINVAL);
 /* each field keeps every other bit */
 writel(0xffffffff, mmio + 0xc50);
 assert(y2_msdc_pad_field(1, 1, Y2_PAD_DRIVE, 4) == 0 && readl(mmio + 0xc50) == 0xfffffcff);
 assert(y2_msdc_pad_field(1, 1, Y2_PAD_SLEW, 0) == 0 && readl(mmio + 0xc50) == 0xffffecff);
 assert(y2_msdc_pad_field(1, 1, Y2_PAD_SMT, 0) == 0 && readl(mmio + 0xc50) == 0xffffccff);
 assert(y2_msdc_pad_field(1, 1, Y2_PAD_PULL, 0x20) == 0 && readl(mmio + 0xc50) == 0xffffcc20);
 writel(0xffffffff, mmio + 0xc70);
 assert(y2_msdc_pad_field(1, 3, Y2_PAD_RDSEL, 0) == 0 && readl(mmio + 0xc70) == 0xfffffc0f);
 assert(y2_msdc_pad_field(1, 3, Y2_PAD_TDSEL, 5) == 0 && readl(mmio + 0xc70) == 0xfffffc05 + 0);
 /* range and line checks reject without touching the register */
 unsigned before = readl(mmio + 0xc70);
 assert(y2_msdc_pad_field(1, 3, Y2_PAD_RDSEL, 64) == -EINVAL && readl(mmio + 0xc70) == before);
 assert(y2_msdc_pad_field(1, 1, Y2_PAD_RDSEL, 1) == -EINVAL);    /* RDSEL lives in the PAD cell */
 assert(y2_msdc_pad_field(1, 3, Y2_PAD_DRIVE, 1) == -EINVAL);
 assert(y2_msdc_pad_field(1, 0, Y2_PAD_DRIVE, 8) == -EINVAL && y2_msdc_pad_field(2, 0, Y2_PAD_DRIVE, 1) == -EINVAL);
 return 0;
}''')

    def test_sd_pads_get_the_stock_schmitt_input_and_zero_rdsel_tdsel(self):
        text = (PLATFORM / 'pinctrl.c').read_text()
        body = between(text, 'int y2_msdc_pad_drive(', 'int y2_msdc_pad_cells(')
        run_c(self.PINS + body + r'''
int main(void) {
 static char mmio[0x1000]; struct y2_pins pins = { mmio, 0 }; unsigned before[3];
 y2_pins_owner = &pins;
 /* LK state candidate 3 found: drive 1, SMT 0, RDSEL 12, TDSEL 5 */
 for (int i = 0; i < 3; i++) writel(0x4010 | (1u << 8), mmio + y2_msdc_pad_offsets[1][i]);
 writel(0xc5, mmio + 0xc70);
 assert(y2_msdc_pad_drive(1, true, before) == 0 && before[0] == 1);
 for (int i = 0; i < 3; i++) {
  unsigned v = readl(mmio + y2_msdc_pad_offsets[1][i]);
  assert(y2_pad_cell_drive(v) == 4 && y2_pad_cell_smt(v) == 1 && (v & 0x4010) == 0x4010);
 }
 assert(readl(mmio + 0xc70) == 0);                        /* stock RDSEL = TDSEL = 0 */
 /* 3.3 V uses the 7/7/7 drive and the same input pad settings */
 assert(y2_msdc_pad_drive(1, false, NULL) == 0);
 assert(y2_pad_cell_drive(readl(mmio + 0xc50)) == 7 && y2_pad_cell_smt(readl(mmio + 0xc50)) == 1);
 /* the eMMC pad cells are not touched beyond their drive */
 writel(0xc5, mmio + 0xc30); writel(0x0, mmio + 0xc00);
 assert(y2_msdc_pad_drive(0, true, NULL) == 0);
 assert(readl(mmio + 0xc30) == 0xc5 && y2_pad_cell_smt(readl(mmio + 0xc00)) == 0);
 return 0;
}''')

    def test_pad_field_update_is_pure_and_bounded(self):
        run_tuning_c(r'''
#include <assert.h>
#include "storage-modes.h"
int main(void) {
 unsigned out;
 assert(y2_pad_field_update(0, 0, Y2_PAD_DRIVE, 7, &out) == 0 && out == 0x700);
 assert(y2_pad_field_update(0xffffffffu, 2, Y2_PAD_SMT, 0, &out) == 0 && out == 0xffffdfffu);
 assert(y2_pad_field_update(0, 3, Y2_PAD_RDSEL, 63, &out) == 0 && out == 0x3f0);
 assert(y2_pad_field_update(0, 3, Y2_PAD_TDSEL, 15, &out) == 0 && out == 0xf);
 assert(y2_pad_field_update(0, 3, Y2_PAD_TDSEL, 16, &out) == -1);
 assert(y2_pad_field_update(0, 4, Y2_PAD_DRIVE, 1, &out) == -1);
 assert(Y2_MSDC_PAD_RDTD_MASK == 0x3ff && Y2_MSDC_CELL_SMT == 0x2000);
 return 0;
}''')

    def test_rxdlysel_is_unsupported_when_the_bit_never_sticks(self):
        run_tuning_c(r'''
#include <assert.h>
int main(void) {
 /* writable-mask the probe read back on MT6582 PAD_TUNE: fields only */
 const unsigned fields = 0xfbc01f1fu;       /* DATWR, DATRD, CMDRD, CMDRR, CLKTX */
 assert(!y2_rxdlysel_supported(fields));
 assert(y2_rxdlysel_supported(fields | (1u << 15)));
 assert(Y2_MSDC_PAD_TUNE_RXDLYSEL == 0x8000);
 return 0;
}''')
        t = source('drivers/mmc/host/mtk-sd.c')
        init = between(t, 'static void y2_probe_tuning_registers(', 'static void msdc_init_hw(')
        self.assertIn('writel(0xffffffff, host->base + tune_reg)', init)
        self.assertIn('writel(0, host->base + tune_reg)', init)       # cleared again
        hw = between(t, 'static void msdc_init_hw(', 'static void msdc_deinit_hw(')
        self.assertIn('y2_probe_tuning_registers(host)', hw)
        self.assertIn('if (y2_rxdlysel_supported(host->y2_probe_pad_mask))', hw)
        self.assertIn('host->y2_rxdlysel_actual = !!(readl(host->base + tune_reg) &', hw)


class TuningDriver(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = source('drivers/mmc/host/mtk-sd.c')

    def tuning_code(self):
        return between(self.text, 'static void y2_set_cmd_delay(', '/* ---- Candidate 3 diagnostics')

    SIM = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "storage-tuning.h"
typedef unsigned int u32; typedef unsigned long long u64;
#define BIT(n) (1U << (n))
#define GENMASK(h, l) (((~0U) << (l)) & (~0U >> (31 - (h))))
#define MSDC_IOCON 0x04
#define MSDC_PATCH_BIT 0xb0
#define MSDC_PATCH_BIT2 0xb8
#define MSDC_PAD_TUNE 0xec
#define MSDC_IOCON_RSPL BIT(1)
#define MSDC_IOCON_DSPL BIT(2)
#define MSDC_IOCON_DDLSEL BIT(3)
#define MSDC_IOCON_W_DSPL BIT(8)
#define MSDC_PAD_TUNE_DATRRDLY GENMASK(12, 8)
#define MSDC_PAD_TUNE_CMDRDLY GENMASK(20, 16)
#define MSDC_PAD_TUNE_CMDRRDLY GENMASK(26, 22)
#define MSDC_INT_DAT_LATCH_CK_SEL GENMASK(9, 7)
#define Y2_MSDC_DAT_RDDLY0 0xf0
#define MMC_TIMING_UHS_SDR104 6
typedef char __iomem_t;
#define __iomem
static u32 regs[0x80];
static u32 readl(const void *p) { return *(const u32 *)p; }
static void writel(u32 v, void *p) { *(u32 *)p = v; }
static void sdr_set_bits(void *reg, u32 bs) { writel(readl(reg) | bs, reg); }
static void sdr_clr_bits(void *reg, u32 bs) { writel(readl(reg) & ~bs, reg); }
static void sdr_set_field(void *reg, u32 field, u32 val)
{ u32 tv = readl(reg); tv &= ~field; tv |= (val << (__builtin_ffs(field) - 1)); writel(tv, reg); }
struct dev_comp { u32 pad_tune_reg; };
struct msdc_host {
 char *base; struct dev_comp *dev_comp; u32 hs200_cmd_int_delay, latch_ck, y2_tune_count;
 struct y2_tune_log y2_tune_first, y2_tune_last; void *dev; };
struct mmc_ios { unsigned char timing; };
struct mmc_host { struct mmc_ios ios; struct msdc_host *host; };
static struct msdc_host *mmc_priv(struct mmc_host *m) { return m->host; }
#define dev_info(d, ...) do { (void)(d); } while (0)
static void msdc_set_data_sample_edge(struct msdc_host *host, bool rising)
{ u32 v = rising ? 0 : 1;
  sdr_set_field(host->base + MSDC_IOCON, MSDC_IOCON_DSPL, v);
  sdr_set_field(host->base + MSDC_IOCON, MSDC_IOCON_W_DSPL, v); }
/* The simulated card/controller: a tap passes iff it lies in the programmed eye. */
static unsigned eye_cmd[2], eye_data[2];     /* pass maps per edge */
static unsigned calls, flaky_mask, tap_calls[32];
static int mmc_send_tuning(struct mmc_host *m, u32 op, int *cmd_error)
{
 u32 pad = regs[MSDC_PAD_TUNE / 4], io = regs[MSDC_IOCON / 4];
 unsigned cmd_tap = (pad >> 16) & 31, data_tap = (pad >> 8) & 31;
 bool cmd_ok = eye_cmd[!!(io & MSDC_IOCON_RSPL)] & BIT(cmd_tap);
 bool data_ok = cmd_ok && (eye_data[!!(io & MSDC_IOCON_DSPL)] & BIT(data_tap));
 /* a flaky tap passes only every other time it is tried */
 if (data_ok && (flaky_mask & BIT(data_tap)) && (tap_calls[data_tap]++ & 1)) data_ok = false;
 calls++;
 if (cmd_error) *cmd_error = cmd_ok ? 0 : -EILSEQ;
 return data_ok ? 0 : -EILSEQ;
}
'''

    def run_sim(self, scenario):
        run_c(self.SIM + self.tuning_code() + r'''
static struct dev_comp comp = { MSDC_PAD_TUNE };
static struct msdc_host host; static struct mmc_host mmc;
static void reset(void) { memset(regs, 0, sizeof(regs)); memset(&host, 0, sizeof(host));
 host.base = (char *)regs; host.dev_comp = &comp; mmc.host = &host; mmc.ios.timing = MMC_TIMING_UHS_SDR104; calls = 0; flaky_mask = 0; memset(tap_calls, 0, sizeof(tap_calls)); }
static unsigned pad(void) { return regs[MSDC_PAD_TUNE / 4]; }
static unsigned io(void) { return regs[MSDC_IOCON / 4]; }
''' + scenario)

    def test_tuning_is_32_taps_and_writes_only_its_own_fields(self):
        code = self.tuning_code()
        self.assertNotIn('PAD_DELAY_FULL', code)
        self.assertNotIn('TUNING_REG2_FIXED_OFFEST', code)
        self.assertNotIn('msdc_set_cmd_delay(', code)    # mainline's helper writes DAT_RDDLY0
        self.assertNotIn('msdc_set_data_delay(', code)
        self.assertIn('tap & (Y2_TUNE_TAPS - 1)', code)  # sdr_set_field never masks its value
        self.assertIn('for (tap = 0; tap < Y2_TUNE_TAPS; tap++)', code)
        self.assertIn('for (edge = 0; edge < Y2_TUNE_EDGES; edge++)', code)
        probe = between(self.text, 'msdc_of_property_parse(pdev, host);',
                        'host->dev = &pdev->dev;')
        self.assertIn('host->tuning_step = Y2_TUNE_TAPS', probe)
        self.assertIn('host->y2_emmc || host->y2_sd', probe)

    def test_command_and_data_are_tuned_separately_on_the_better_edge(self):
        self.run_sim(r'''
int main(void) {
 reset();
 eye_cmd[0] = 0x001fff0u << 0;                 /* rising: taps 4..16 (13) */
 eye_cmd[0] = 0x0001fff0u;
 eye_cmd[1] = 0x00000f00u;                     /* falling: taps 8..11 (4) */
 eye_data[0] = 0x000001f8u;                    /* rising: taps 3..8 (6) */
 eye_data[1] = 0x07fff000u;                    /* falling: taps 12..26 (15) */
 int ret = y2_sd_execute_tuning(&mmc, 19);
 struct y2_tune_log *l = &host.y2_tune_last;
 assert(ret == 0 && l->result == 0 && l->valid);
 /* command: rising edge, centre of 4..16 */
 assert(l->cmd.pick.edge == 0 && l->cmd.pick.len == 13 && l->cmd.pick.delay == 10);
 assert(!(io() & MSDC_IOCON_RSPL) && ((pad() >> 16) & 31) == 10);
 /* data: the falling edge has the larger eye */
 assert(l->data.pick.edge == 1 && l->data.pick.len == 15 && l->data.pick.delay == 19);
 assert((io() & MSDC_IOCON_DSPL) && ((pad() >> 8) & 31) == 19);
 /* every pass map is recorded for both edges, nothing else is touched */
 assert(l->cmd.all[0] == 0x0001fff0u && l->cmd.all[1] == 0x00000f00u);
 assert(l->data.all[0] == 0x000001f8u && l->data.all[1] == 0x07fff000u);
 assert(l->cmd.scanned == 3 && l->data.scanned == 3);
 assert(regs[0xf0 / 4] == 0 && ((pad() >> 27) & 31) == 0);        /* no stray CLKTXDLY */
 assert(host.y2_tune_count == 1 && host.y2_tune_first.valid);
 /* the command-internal sweep passed everywhere: no information, stock value stays */
 assert(l->cmd_int.pick.open && ((pad() >> 22) & 31) == 0);
 return 0;
}''')

    def test_a_tap_must_pass_every_try_and_flaky_taps_are_visible(self):
        self.run_sim(r'''
int main(void) {
 reset();
 eye_cmd[0] = eye_cmd[1] = 0xffffffffu; eye_data[0] = eye_data[1] = 0xffffffffu;
 struct y2_tune_phase ph; memset(&ph, 0, sizeof(ph));
 y2_tune_sweep(&host, &mmc, 19, Y2_SWEEP_DATA, 0, Y2_TUNE_TRIES, &ph);
 assert(ph.all[0] == 0xffffffffu && ph.any[0] == 0xffffffffu && ph.scanned == 1);
 /* tap 4 never passes: neither "all" nor "any" */
 eye_data[0] = 0xffffffefu;
 memset(&ph, 0, sizeof(ph));
 y2_tune_sweep(&host, &mmc, 19, Y2_SWEEP_DATA, 0, Y2_TUNE_TRIES, &ph);
 assert(ph.all[0] == 0xffffffefu && ph.any[0] == 0xffffffefu);
 /* tap 9 passes only some of its tries: "any" but not "all", so it cannot widen an eye */
 eye_data[0] = 0xffffffffu; flaky_mask = BIT(9); memset(tap_calls, 0, sizeof(tap_calls));
 memset(&ph, 0, sizeof(ph));
 y2_tune_sweep(&host, &mmc, 19, Y2_SWEEP_DATA, 0, Y2_TUNE_TRIES, &ph);
 assert(ph.any[0] == 0xffffffffu && ph.all[0] == (0xffffffffu & ~BIT(9)));
 /* the pick therefore keeps the flaky tap out of the window */
 y2_tune_phase_choose(&ph, Y2_TUNE_TAPS);
 assert(ph.pick.valid && ph.pick.windows[0] == 2 && ph.pick.start == 10 && ph.pick.len == 22);
 return 0;
}''')

    def test_no_command_eye_fails_tuning_and_restores_the_registers(self):
        self.run_sim(r'''
int main(void) {
 reset();
 regs[MSDC_PAD_TUNE / 4] = 0x00001300u; regs[MSDC_IOCON / 4] = 0x4;
 eye_cmd[0] = 0x00000007u; eye_cmd[1] = 0x0u;   /* only 3 taps: below the minimum */
 eye_data[0] = eye_data[1] = 0xffffffffu;
 assert(y2_sd_execute_tuning(&mmc, 19) == -EIO);
 assert(!host.y2_tune_last.cmd.pick.valid && host.y2_tune_last.result == -EIO);
 /* the half-swept delays are put back for the next, lower level */
 assert(pad() == 0x00001300u && io() == 0x4);
 /* a data stage without an eye fails the same way */
 reset(); eye_cmd[0] = 0xffffffffu; eye_cmd[1] = 0xffffffffu; eye_data[0] = 0; eye_data[1] = 0x3;
 assert(y2_sd_execute_tuning(&mmc, 19) == -EIO && !host.y2_tune_last.data.pick.valid);
 assert(pad() == 0 && io() == 0);
 return 0;
}''')

    def test_all_pass_sweeps_pick_the_middle_and_flag_the_open_eye(self):
        self.run_sim(r'''
int main(void) {
 reset();
 eye_cmd[0] = eye_cmd[1] = eye_data[0] = eye_data[1] = 0xffffffffu;
 assert(y2_sd_execute_tuning(&mmc, 19) == 0);
 struct y2_tune_log *l = &host.y2_tune_last;
 assert(l->cmd.pick.open && l->data.pick.open && l->cmd.pick.delay == 15 && l->data.pick.delay == 15);
 /* mainline's len/3 rule would have said 10 on this map (64 taps: 21) */
 assert(l->cmd.legacy[0] == 10 && l->data.legacy[0] == 10);
 return 0;
}''')

    def test_diagnostic_output_lists_every_map_window_and_pick(self):
        diag = between(self.text, 'static const char *const y2_edge_name', 'static ssize_t y2_tune_diag_show(')
        run_c(self.SIM.replace('#include <stdio.h>', '#include <stdio.h>\n#include <stdarg.h>') + r'''
#define PAGE 4096
static int sysfs_emit_at(char *buf, int at, const char *fmt, ...)
{ va_list ap; va_start(ap, fmt); int n = vsnprintf(buf + at, PAGE - at, fmt, ap); va_end(ap); return n; }
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
const char *y2_timing_name(unsigned char t) { return t == 6 ? "sdr104" : "other"; }
''' + diag + r'''
int main(void) {
 static char buf[PAGE]; struct y2_tune_log log; memset(&log, 0, sizeof(log));
 log.valid = 1; log.timing = 6; log.pad_tune = 0x09951500u; log.cmd.scanned = 3;
 log.cmd.all[0] = 0x0001fff0u; log.cmd.any[0] = 0x0003fff8u; log.cmd.all[1] = 0;
 y2_tune_phase_choose(&log.cmd, Y2_TUNE_TAPS);
 int n = y2_diag_log(buf, 0, "last", &log);
 assert(n > 0 && n < PAGE);
 assert(strstr(buf, "tune.last: result=0 timing=sdr104"));
 assert(strstr(buf, "cmdrdly=21 cmdrrdly=6 datrrdly=21 datwrdly=0 clktxdly=1"));
 assert(strstr(buf, "tune.last.cmd.pick: valid=1 edge=rise delay=10 window=4-16 eye=13 margin=6/6"));
 assert(strstr(buf, "tune.last.cmd.rise: all=0001fff0 any=0003fff8 windows=4-16"));
 assert(strstr(buf, "tune.last.cmd.fall: all=00000000 any=00000000 windows=none"));
 assert(strstr(buf, "legacy=10/255") || strstr(buf, "legacy=10/") != NULL);
 /* worst case: both logs, every phase scanned on both edges, maximally fragmented maps */
 struct y2_tune_log full; memset(&full, 0, sizeof(full)); full.valid = 1; full.timing = 6;
 struct y2_tune_phase *ph[3] = { &full.cmd, &full.cmd_int, &full.data };
 for (int i = 0; i < 3; i++) { ph[i]->scanned = 3;
  ph[i]->all[0] = ph[i]->all[1] = 0x55555555u; ph[i]->any[0] = ph[i]->any[1] = 0xffffffffu;
  y2_tune_phase_choose(ph[i], Y2_TUNE_TAPS); }
 int total = y2_diag_log(buf, 0, "first", &full);
 total = y2_diag_log(buf, total, "last", &full);
 assert(total > 1000 && total < PAGE - 200);   /* a sysfs page is 4096 bytes */
 return 0;
}''')

    def test_fallback_records_the_reason_and_previous_mode_and_the_clock_steps_down(self):
        t = self.text
        ladder = (between(t, '#define Y2_MANAGED_CAPS', 'static int y2_msdc_clock_ready(')
                  + between(t, 'static void y2_downgrade(', '/* Classify a completed data request.'))
        run_c(STUBS + ladder + r'''
int main(void) {
 static struct mmc_card card;
 struct msdc_host s = {0}; s.y2_sd = true; s.mmc.f_max = 200000000;
 s.y2_dt_caps = MMC_CAP_SD_HIGHSPEED | MMC_CAP_UHS | MMC_CAP_UHS_SDR104;
 s.mmc.card = &card; s.mmc.ios.power_mode = MMC_POWER_ON; s.mmc.ios.timing = MMC_TIMING_UHS_SDR104;
 s.y2_tuning_runs = 4;
 y2_apply_level(&s, 0);
 assert(s.y2_clock_limit == 200000000);
 y2_downgrade(&s, Y2_EVENT_CRC, true);          /* SDR104 -> DDR50 */
 assert(s.y2_level == 1 && s.y2_clock_limit == 50000000);
 assert(s.y2_fb_from == 0 && s.y2_fb_event == Y2_EVENT_CRC && s.y2_fb_tuning_runs == 4);
 s.y2_tuning_runs = 6;
 s.mmc.ios.timing = MMC_TIMING_UHS_DDR50;
 y2_downgrade(&s, Y2_EVENT_TIMEOUT, true);      /* DDR50 -> SDR50, newest cause wins */
 assert(s.y2_level == 2 && s.y2_fb_from == 1 && s.y2_fb_event == Y2_EVENT_TIMEOUT);
 assert(s.y2_fb_tuning_runs == 6 && s.y2_clock_fallbacks == 2);
 /* the bottom of the ladder never rewrites the recorded cause */
 s.y2_level = 5; unsigned from = s.y2_fb_from;
 y2_downgrade(&s, Y2_EVENT_CRC, true);
 assert(s.y2_level == 5 && s.y2_fb_from == from);
 return 0;
}''')

    def test_fault_capture_is_wired_into_the_interrupt_and_request_paths(self):
        t = self.text
        irq = between(t, 'static irqreturn_t msdc_irq(', 'static void msdc_init_hw')
        self.assertIn('MSDC_INT_DATCRCERR', irq)
        self.assertIn('host->y2_irq_dcrc = readl(host->base + Y2_MSDC_DATCRC_STS)', irq)
        # the lane status is read before the data path resets the controller
        self.assertLess(irq.index('Y2_MSDC_DATCRC_STS'), irq.index('msdc_data_xfer_done(host'))
        done = between(t, 'static void msdc_request_done(', 'static void msdc_set_buswidth(')
        self.assertIn('y2_record_fault(host, mrq)', done)
        record = between(t, 'static void y2_record_fault(', 'static inline u32 msdc_cmd_find_resp(')
        self.assertIn('mmc_op_tuning(mrq->cmd->opcode)', record)     # tuning sweeps are not faults
        self.assertIn('!host->y2_fault.have_first', record)          # only the first is logged
        self.assertEqual(record.count('dev_warn('), 1)

    def test_card_driver_type_defaults_to_b_and_follows_the_lab_request(self):
        code = between(self.text, 'static int y2_select_drive_strength(', 'static int y2_msdc_execute_tuning(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#define SD_DRIVER_TYPE_B 0x01
#define SD_DRIVER_TYPE_A 0x02
#define SD_DRIVER_TYPE_C 0x04
#define SD_DRIVER_TYPE_D 0x08
#define READ_ONCE(x) (x)
struct msdc_host { bool y2_sd; unsigned y2_drv_type; };
struct mmc_host { struct msdc_host *h; };
struct mmc_card { struct mmc_host *host; };
static struct msdc_host *mmc_priv(struct mmc_host *m) { return m->h; }
''' + code + r'''
int main(void) {
 struct msdc_host h = { .y2_sd = true }; struct mmc_host m = { &h }; struct mmc_card c = { &m };
 int type = 9, all = 0xf;
 assert(y2_select_drive_strength(&c, 208000000, all, all, &type) == 0 && type == 0);   /* B */
 h.y2_drv_type = 1; assert(y2_select_drive_strength(&c, 0, all, all, &type) == 1 && type == 1);   /* A */
 h.y2_drv_type = 2; assert(y2_select_drive_strength(&c, 0, all, all, &type) == 2 && type == 2);   /* C */
 h.y2_drv_type = 3; assert(y2_select_drive_strength(&c, 0, all, all, &type) == 3 && type == 3);   /* D */
 /* a type the card does not offer falls back to B */
 assert(y2_select_drive_strength(&c, 0, all, SD_DRIVER_TYPE_B | SD_DRIVER_TYPE_A, &type) == 0 && type == 0);
 /* a type the host does not advertise falls back to B */
 assert(y2_select_drive_strength(&c, 0, SD_DRIVER_TYPE_B, all, &type) == 0);
 h.y2_drv_type = 9; assert(y2_select_drive_strength(&c, 0, all, all, &type) == 0);
 h.y2_drv_type = 1; h.y2_sd = false; assert(y2_select_drive_strength(&c, 0, all, all, &type) == 0);   /* eMMC */
 return 0;
}''')
        self.assertIn('.select_drive_strength = y2_select_drive_strength', self.text)
        self.assertIn('MMC_CAP_DRIVER_TYPE_A | MMC_CAP_DRIVER_TYPE_C | MMC_CAP_DRIVER_TYPE_D', self.text)

    def test_candidate_diagnostics_do_not_change_the_one_line_status_contract(self):
        t = self.text
        show = between(t, 'static ssize_t y2_storage_show(', 'static DEVICE_ATTR_RO(y2_storage);')
        for name in ('y2_tune_first', 'y2_tune_last', 'y2_fault', 'rxdlysel', 'pad.', 'y2_probe'):
            self.assertNotIn(name, show)
        self.assertIn('static DEVICE_ATTR_RO(y2_storage_diag);', t)
        self.assertIn('static DEVICE_ATTR_RO(y2_tune_diag);', t)
        # the characterization interface is root-only and compiled out of production
        self.assertIn('static DEVICE_ATTR(y2_lab, 0600, y2_lab_show, y2_lab_store);', t)
        self.assertIn('#if Y2_MSDC_LAB', t)
        header = (PLATFORM / 'storage-tuning.h').read_text()
        self.assertIn('#define Y2_MSDC_LAB 1', header)


if __name__ == '__main__':
    unittest.main()
