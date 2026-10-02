"""Storage ceiling: capability parsing, negotiation ladder and every fallback,
exercised on the real policy header and the real patched MSDC driver code."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

from tests.test_hardware_ceiling import source

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'


def run_c(program):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory)
        (path / 'test.c').write_text(program)
        subprocess.run([shutil.which('cc'), '-std=gnu11', '-Wall', '-Werror', '-Wno-unused-function',
                        '-I' + str(PLATFORM), str(path / 'test.c'), '-o', str(path / 'test')],
                       check=True)
        subprocess.run([str(path / 'test')], check=True)


def between(text, start, end):
    body = text[text.index(start):]
    return body[:body.index(end)]


# Minimal kernel/MMC environment for compiling extracted driver bodies.
STUBS = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "storage-modes.h"
typedef unsigned int u32; typedef unsigned char u8;
#define min(a,b) ((a)<(b)?(a):(b))
#define WRITE_ONCE(x,v) ((x)=(v))
#define READ_ONCE(x) (x)
#define dev_warn(d, ...) do { (void)(d); if (0) printf(__VA_ARGS__); } while (0)
#define dev_info(d, ...) do { (void)(d); if (0) printf(__VA_ARGS__); } while (0)
#define dev_dbg(d, ...) do { (void)(d); if (0) printf(__VA_ARGS__); } while (0)
#define MMC_CAP_SD_HIGHSPEED (1u<<0)
#define MMC_CAP_MMC_HIGHSPEED (1u<<1)
#define MMC_CAP_UHS_SDR12 (1u<<2)
#define MMC_CAP_UHS_SDR25 (1u<<3)
#define MMC_CAP_UHS_SDR50 (1u<<4)
#define MMC_CAP_UHS_SDR104 (1u<<5)
#define MMC_CAP_UHS_DDR50 (1u<<6)
#define MMC_CAP_UHS (MMC_CAP_UHS_SDR12|MMC_CAP_UHS_SDR25|MMC_CAP_UHS_SDR50|MMC_CAP_UHS_SDR104|MMC_CAP_UHS_DDR50)
#define MMC_CAP_1_8V_DDR (1u<<7)
#define MMC_CAP_3_3V_DDR (1u<<8)
#define MMC_CAP_8_BIT_DATA (1u<<9)
#define MMC_CAP2_HS200_1_8V_SDR (1u<<0)
#define MMC_CAP2_HS200_1_2V_SDR (1u<<1)
#define MMC_CAP2_HS200 (MMC_CAP2_HS200_1_8V_SDR|MMC_CAP2_HS200_1_2V_SDR)
#define MMC_CAP2_HS400 (1u<<2)
#define MMC_CAP2_HS400_ES (1u<<3)
#define MMC_CAP2_NO_SD (1u<<4)
enum { MMC_TIMING_LEGACY, MMC_TIMING_MMC_HS, MMC_TIMING_SD_HS, MMC_TIMING_UHS_SDR12,
 MMC_TIMING_UHS_SDR25, MMC_TIMING_UHS_SDR50, MMC_TIMING_UHS_SDR104, MMC_TIMING_UHS_DDR50,
 MMC_TIMING_MMC_DDR52, MMC_TIMING_MMC_HS200, MMC_TIMING_MMC_HS400 };
#define MMC_POWER_ON 2
#define MMC_SIGNAL_VOLTAGE_330 0
#define MMC_SIGNAL_VOLTAGE_180 1
struct mmc_card { int removed; };
struct mmc_ios { unsigned char timing, power_mode, signal_voltage; unsigned clock; };
struct mmc_host { u32 caps, caps2, f_max, actual_clock; struct mmc_card *card; struct mmc_ios ios; };
struct msdc_host {
 bool y2_emmc, y2_sd; unsigned y2_level, y2_ceiling; u32 y2_dt_caps, y2_dt_caps2;
 u32 y2_clock_limit, y2_level_strikes, y2_clock_fallbacks, y2_resets; int y2_reset_error;
 int y2_clock_error, y2_verify_result; u32 y2_events[Y2_EVENT_COUNT];
 unsigned y2_fb_from; int y2_fb_event; u32 y2_fb_tuning_runs, y2_tuning_runs;
 struct mmc_host mmc; void *dev; };
static struct mmc_host *mmc_from_priv(struct msdc_host *h) { return &h->mmc; }
static int mmc_card_removed(struct mmc_card *c) { return c->removed; }
static int resets, reset_failures, clocks;
static int mmc_hw_reset(struct mmc_card *c) { resets++; return reset_failures-- > 0 ? -EIO : 0; }
static int msdc_set_mclk(struct msdc_host *h, unsigned char t, unsigned hz) { clocks++; return 0; }
'''


class StoragePolicy(unittest.TestCase):
    def test_ladders_only_ever_move_down_and_subset_caps(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "storage-modes.h"
int main(void) {
 for (int sd = 0; sd < 2; sd++) {
  unsigned count; const struct y2_storage_level *l = y2_storage_ladder(sd, &count);
  for (unsigned i = 1; i < count; i++) {
   assert((l[i].caps & ~l[0].caps) == 0);             /* nothing beyond the top */
   assert(!(l[i].caps & Y2_CAP_TOP));                  /* the top mode is never retried */
   assert(l[i].clock_hz <= l[i-1].clock_hz || (sd && i == 2)); /* SDR50 after DDR50 */
   if (!(l[i-1].caps & Y2_CAP_UHS)) assert(!(l[i].caps & Y2_CAP_UHS)); /* 1.8 V never returns */
  }
  assert(l[count-1].clock_hz == 13000000);
  for (unsigned i = 0; i < count; i++)
   for (int e = 0; e < Y2_EVENT_COUNT; e++) {
    unsigned next = y2_storage_fallback(sd, i, e);
    assert(i == count-1 ? next == i : next > i && next < count);
   }
 }
 /* eMMC: HS200 200 MHz is the ceiling; HS400 is not on the ladder. */
 assert(!strcmp(y2_emmc_levels[0].name, "HS200") && y2_emmc_levels[0].clock_hz == 200000000);
 assert(!strcmp(y2_emmc_levels[1].name, "DDR52") && y2_emmc_levels[1].clock_hz == 50000000);
 assert(!strcmp(y2_emmc_levels[2].name, "HS52"));
 /* SD: SDR104 200 MHz, DDR50, SDR50 100 MHz; only 1.8 V levels carry UHS. */
 assert(!strcmp(y2_sd_levels[0].name, "SDR104") && y2_sd_levels[0].clock_hz == 200000000);
 assert(y2_sd_levels[2].clock_hz == 100000000 && (y2_sd_levels[2].caps & Y2_CAP_UHS));
 assert(!(y2_sd_levels[3].caps & Y2_CAP_UHS) && y2_sd_levels[3].clock_hz == 50000000);
 /* A failed 1.8 V switch skips every remaining UHS level. */
 assert(y2_storage_fallback(1, 0, Y2_EVENT_VOLTAGE) == 3);
 assert(y2_storage_fallback(1, 1, Y2_EVENT_VOLTAGE) == 3);
 assert(y2_storage_fallback(1, 0, Y2_EVENT_TUNING) == 1);
 assert(y2_storage_fallback(0, 0, Y2_EVENT_TUNING) == 1);
 assert(y2_storage_level_named(0, "DDR52") == 1 && y2_storage_level_named(1, "HS\n") == 3);
 assert(y2_storage_level_named(0, "HS400") == -1 && y2_storage_level_named(1, "") == -1);
 return 0;
}''')

    def test_card_capability_parsing(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "storage-modes.h"
int main(void) {
 /* Installed eMMC: EXT_CSD card type 0x57. */
 assert(y2_emmc_card_level(0x57) == 0);
 assert(y2_emmc_card_level(0x07) == 1 && y2_emmc_card_level(0x03) == 2 && y2_emmc_card_level(0x01) == 3);
 /* SD 3.0 bus modes: SDR104 | DDR50 | SDR50 | SDR25 | SDR12 = 0x1f. */
 assert(y2_sd_card_level(0x1f, 1, 1) == 0);
 assert(y2_sd_card_level(0x17, 1, 1) == 1);   /* no SDR104 */
 assert(y2_sd_card_level(0x07, 1, 1) == 2);   /* SDR50 only */
 assert(y2_sd_card_level(0x1f, 0, 1) == 3);   /* refused 1.8 V: High Speed */
 assert(y2_sd_card_level(0, 0, 0) == 4);
 unsigned char a[512] = {0}, b[512] = {0};
 a[196] = b[196] = 0x57; a[212] = b[212] = 0x80;
 assert(y2_emmc_identity_matches(a, b));
 b[268] = 9;                       /* life time estimate: not identity */
 assert(y2_emmc_identity_matches(a, b));
 b[212] = 0x81;                    /* SEC_COUNT differs: corrupted readback */
 assert(!y2_emmc_identity_matches(a, b));
 return 0;
}''')

    def test_stock_pad_drive_table(self):
        run_c(r'''
#include <assert.h>
#include "storage-modes.h"
int main(void) {
 /* msdc_set_driving 0xc04f28c0: GPIO+0xc00.. bits 10:8; stock msdc_hw values. */
 assert(y2_msdc_drives[0].offset[0] == 0xc00 && y2_msdc_drives[0].offset[2] == 0xc20);
 assert(y2_msdc_drives[0].v33[0] == 4 && y2_msdc_drives[0].v33[1] == 2 && y2_msdc_drives[0].v33[2] == 2);
 assert(y2_msdc_drives[1].offset[0] == 0xc40 && y2_msdc_drives[1].offset[2] == 0xc60);
 for (int i = 0; i < 3; i++)
  assert(y2_msdc_drives[1].v33[i] == 7 && y2_msdc_drives[1].v18[i] == 4);
 assert(y2_msdc_drive_value(0xffffffffu, 4) == 0xfffffcffu);
 assert(y2_msdc_drive_value(0, 7) == 0x700 && y2_msdc_drive_value(0x12345678, 0) == 0x12345078);
 return 0;
}''')


class StorageDriver(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = source('drivers/mmc/host/mtk-sd.c')

    def ladder_code(self):
        t = self.text
        return (between(t, '#define Y2_MANAGED_CAPS', 'static int y2_msdc_clock_ready(')
                + between(t, 'static void y2_downgrade(', '/* Classify a completed data request.'))

    def test_levels_never_exceed_device_tree_and_safe_mode_has_no_high_modes(self):
        run_c(STUBS + self.ladder_code() + r'''
int main(void) {
 struct msdc_host h = {0}; struct mmc_host *m = &h.mmc;
 h.y2_sd = true; m->f_max = 200000000;
 h.y2_dt_caps = MMC_CAP_SD_HIGHSPEED | MMC_CAP_UHS | MMC_CAP_8_BIT_DATA;
 m->caps = h.y2_dt_caps;
 y2_apply_level(&h, 0);
 assert((m->caps & MMC_CAP_UHS) == MMC_CAP_UHS && h.y2_clock_limit == 200000000);
 assert(m->caps & MMC_CAP_8_BIT_DATA);           /* unmanaged caps untouched */
 y2_apply_level(&h, 1);                           /* DDR50 */
 assert(m->caps & MMC_CAP_UHS_DDR50 && !(m->caps & (MMC_CAP_UHS_SDR104 | MMC_CAP_UHS_SDR50)));
 assert(h.y2_clock_limit == 50000000);
 y2_apply_level(&h, 3);                           /* 3.3 V High Speed */
 assert(!(m->caps & MMC_CAP_UHS) && m->caps & MMC_CAP_SD_HIGHSPEED);
 /* DT without SDR104 can never gain it. */
 h.y2_dt_caps &= ~MMC_CAP_UHS_SDR104; y2_apply_level(&h, 0);
 assert(!(m->caps & MMC_CAP_UHS_SDR104));
 /* eMMC */
 struct msdc_host e = {0}; e.y2_emmc = true; e.mmc.f_max = 200000000;
 e.y2_dt_caps = MMC_CAP_MMC_HIGHSPEED | MMC_CAP_1_8V_DDR; e.y2_dt_caps2 = MMC_CAP2_HS200_1_8V_SDR | MMC_CAP2_HS400;
 y2_apply_level(&e, 0);
 assert(e.mmc.caps2 & MMC_CAP2_HS200_1_8V_SDR && !(e.mmc.caps2 & MMC_CAP2_HS400));
 assert(e.mmc.caps & MMC_CAP_1_8V_DDR && e.y2_clock_limit == 200000000);
 y2_apply_level(&e, 1);
 assert(!(e.mmc.caps2 & MMC_CAP2_HS200) && e.mmc.caps & MMC_CAP_1_8V_DDR);
 /* Safe crystal source: probe clears every managed DT cap and f_max 13 MHz. */
 e.y2_dt_caps &= ~Y2_MANAGED_CAPS; e.y2_dt_caps2 &= ~Y2_MANAGED_CAPS2; e.mmc.f_max = 13000000;
 y2_apply_level(&e, 0);
 assert(!(e.mmc.caps & Y2_MANAGED_CAPS) && !(e.mmc.caps2 & Y2_MANAGED_CAPS2) && e.y2_clock_limit == 13000000);
 return 0;
}''')

    def test_fallback_renegotiates_a_rejected_mode_and_keeps_stepping_on_failure(self):
        run_c(STUBS + self.ladder_code() + r'''
static struct msdc_host emmc(void) {
 struct msdc_host e = {0}; e.y2_emmc = true; e.mmc.f_max = 200000000;
 e.y2_dt_caps = MMC_CAP_MMC_HIGHSPEED | MMC_CAP_1_8V_DDR; e.y2_dt_caps2 = MMC_CAP2_HS200_1_8V_SDR;
 y2_apply_level(&e, 0); return e;
}
int main(void) {
 static struct mmc_card card;
 /* Card in HS200: CRC fallback admits DDR52 and renegotiates at once. */
 struct msdc_host e = emmc(); e.mmc.card = &card; e.mmc.ios.power_mode = MMC_POWER_ON;
 e.mmc.ios.timing = MMC_TIMING_MMC_HS200;
 y2_downgrade(&e, Y2_EVENT_CRC, true);
 assert(e.y2_level == 1 && resets == 1 && e.y2_resets == 1 && e.y2_reset_error == 0);
 /* Renegotiation fails in DDR52 too: continue to HS52 and reset again. */
 e = emmc(); e.mmc.card = &card; e.mmc.ios.power_mode = MMC_POWER_ON; e.mmc.ios.timing = MMC_TIMING_MMC_HS200;
 resets = 0; reset_failures = 1;
 y2_downgrade(&e, Y2_EVENT_TIMEOUT, true);
 assert(e.y2_level == 2 && resets == 2 && e.y2_reset_error == 0);
 /* A timing still admitted only lowers the clock (HS52 -> 25 MHz). */
 e.mmc.ios.timing = MMC_TIMING_MMC_HS; resets = 0; clocks = 0;
 y2_downgrade(&e, Y2_EVENT_CRC, true);
 assert(e.y2_level == 3 && resets == 0 && clocks == 1 && e.y2_clock_limit == 25000000);
 /* Inside the core's own initialisation: caps change, no reset. */
 e = emmc(); e.mmc.card = &card; e.mmc.ios.power_mode = MMC_POWER_ON; e.mmc.ios.timing = MMC_TIMING_MMC_HS200;
 resets = 0; y2_downgrade(&e, Y2_EVENT_TUNING, false);
 assert(e.y2_level == 1 && resets == 0 && !(e.mmc.caps2 & MMC_CAP2_HS200));
 /* No card bound: only the admitted modes change. */
 e = emmc(); resets = 0; y2_downgrade(&e, Y2_EVENT_CRC, true);
 assert(e.y2_level == 1 && resets == 0);
 /* Bottom of the ladder is stable. */
 e.y2_level = 4; y2_downgrade(&e, Y2_EVENT_CRC, true); assert(e.y2_level == 4);
 /* SD at SDR104 refusing 1.8 V lands on 3.3 V High Speed. */
 struct msdc_host s = {0}; s.y2_sd = true; s.mmc.f_max = 200000000;
 s.y2_dt_caps = MMC_CAP_SD_HIGHSPEED | MMC_CAP_UHS; y2_apply_level(&s, 0);
 y2_downgrade(&s, Y2_EVENT_VOLTAGE, false);
 assert(s.y2_level == 3 && !(s.mmc.caps & MMC_CAP_UHS));
 return 0;
}''')

    def test_fault_classification_ignores_tuning_sweeps_and_policy_status(self):
        run_c(STUBS + r'''
#define mmc_op_tuning(op) ((op) == 19 || (op) == 21)
''' + between(self.text, 'static int y2_request_fault(', '/* An unanswered CMD55') + r'''
int main(void) {
 assert(y2_request_fault(21, 0, -EILSEQ) == -1);       /* HS200 tuning sweep */
 assert(y2_request_fault(19, -EILSEQ, 0) == -1);       /* SD tuning sweep */
 assert(y2_request_fault(18, 0, -EILSEQ) == Y2_EVENT_CRC);
 assert(y2_request_fault(25, -EILSEQ, 0) == Y2_EVENT_CRC);
 assert(y2_request_fault(18, 0, -ETIMEDOUT) == Y2_EVENT_TIMEOUT);
 assert(y2_request_fault(18, 0, -EIO) == Y2_EVENT_CONTROLLER);
 assert(y2_request_fault(18, -EIO, 0) == -1);           /* R1 status policy */
 assert(y2_request_fault(18, -EROFS, 0) == -1);         /* storage firewall */
 assert(y2_request_fault(18, 0, 0) == -1);
 return 0;
}''')

    def test_tuned_modes_get_one_retune_before_stepping_down(self):
        t = self.text
        worker = between(t, 'static void y2_msdc_clock_recovery(', 'static int y2_verify_emmc(')
        run_c(STUBS + r'''
#define container_of(p, T, m) ((T *)(p))
#define to_delayed_work(w) (w)
struct work_struct { int unused; };
static int atomic_xchg(int *p, int v) { int o = *p; *p = v; return o; }
static int __mmc_claim_host(struct mmc_host *m, void *a, void *b) { return 0; }
static void mmc_release_host(struct mmc_host *m) {}
static int pm_runtime_resume_and_get(void *d) { return 0; }
static void pm_runtime_mark_last_busy(void *d) {}
static void pm_runtime_put_autosuspend(void *d) {}
static int downgrades, last_event;
static bool y2_tuned_timing(unsigned char t) { return t == MMC_TIMING_MMC_HS200 || t == MMC_TIMING_UHS_SDR104 || t == MMC_TIMING_UHS_SDR50; }
static void y2_downgrade(struct msdc_host *h, enum y2_storage_event e, bool r) { downgrades++; last_event = e; h->y2_level_strikes = 0; }
struct worker_host { struct msdc_host h; int y2_pending_event, y2_removing, y2_recovery_abort; };
''' + worker.replace('struct msdc_host *host = container_of(to_delayed_work(work), struct msdc_host, y2_clock_recovery);',
                     'struct worker_host *w = (struct worker_host *)work; struct msdc_host *host = &w->h;')
                .replace('READ_ONCE(host->y2_removing)', 'w->y2_removing')
                .replace('atomic_xchg(&host->y2_pending_event, 0)', 'atomic_xchg(&w->y2_pending_event, 0)')
                .replace('&host->y2_recovery_abort', '&w->y2_recovery_abort')
                .replace('static void y2_msdc_clock_recovery(struct work_struct *work)',
                         'static void y2_msdc_clock_recovery(void *work)') + r'''
int main(void) {
 struct worker_host w = {0}; w.h.y2_emmc = true; w.h.mmc.ios.timing = MMC_TIMING_MMC_HS200;
 w.y2_pending_event = Y2_EVENT_CRC + 1; y2_msdc_clock_recovery(&w);
 assert(downgrades == 0 && w.h.y2_level_strikes == 1);   /* core retunes first */
 w.y2_pending_event = Y2_EVENT_CRC + 1; y2_msdc_clock_recovery(&w);
 assert(downgrades == 1 && last_event == Y2_EVENT_CRC);  /* second strike */
 w.h.mmc.ios.timing = MMC_TIMING_MMC_DDR52;              /* untuned: immediate */
 w.y2_pending_event = Y2_EVENT_TIMEOUT + 1; y2_msdc_clock_recovery(&w);
 assert(downgrades == 2);
 w.h.mmc.ios.timing = MMC_TIMING_MMC_HS200;              /* controller: immediate */
 w.y2_pending_event = Y2_EVENT_CONTROLLER + 1; y2_msdc_clock_recovery(&w);
 assert(downgrades == 3);
 w.y2_pending_event = 0; y2_msdc_clock_recovery(&w); assert(downgrades == 3);
 w.h.y2_clock_error = -EIO; w.y2_pending_event = Y2_EVENT_CONTROLLER + 1;
 y2_msdc_clock_recovery(&w); assert(downgrades == 3);    /* blocked host: reboot to safe */
 return 0;
}''')

    def test_voltage_switch_tuning_and_readback_hooks(self):
        t = self.text
        volt = between(t, 'static int msdc_ops_switch_volt(', 'static int msdc_card_busy(')
        self.assertIn('host->y2_sd && ios->signal_voltage == MMC_SIGNAL_VOLTAGE_180', volt)
        self.assertIn('y2_msdc_pad_drive(1, v18, NULL)', volt)
        self.assertIn('if (host->pinctrl)', volt)
        self.assertNotIn('y2_msdc_pad_drive(0', volt)          # eMMC IO never switches
        tuning = between(t, 'static int y2_msdc_execute_tuning(', 'static int msdc_prepare_hs400_tuning(')
        self.assertIn('y2_downgrade(host, Y2_EVENT_TUNING, false)', tuning)
        self.assertIn('.execute_tuning = y2_msdc_execute_tuning', t)
        verify = between(t, 'static int y2_verify_sd(', '/* Readback in the negotiated mode')
        run_c(STUBS + r'''
#define GFP_KERNEL 0
static void *kzalloc(size_t n, int f) { return calloc(1, n); }
static void kfree(void *p) { free(p); }
static int reads, corrupt, width = 2;
static int mmc_app_sd_status(struct mmc_card *c, void *ssr) {
 unsigned char *b = ssr; memset(b, 0, 64); b[0] = width << 6; b[8] = 0x30;
 if (corrupt && reads++ == 1)
  b[8] ^= 1;
 return 0; }
''' + verify + r'''
int main(void) {
 struct msdc_host h = {0}; struct mmc_card c = {0};
 assert(y2_verify_sd(&h, &c) == 0);
 reads = 0; corrupt = 1; assert(y2_verify_sd(&h, &c) == -EILSEQ);
 corrupt = 0; width = 0; assert(y2_verify_sd(&h, &c) == -EILSEQ);  /* not 4-bit */
 return 0;
}''')

    def test_sd_replacement_and_runtime_pm_keep_tuning(self):
        t = self.text
        empty = between(t, 'static void y2_sd_slot_empty(', '/* Record a transport fault')
        self.assertIn('y2_sd_slot_empty(host)', between(t, 'static void msdc_track_cmd_data(', 'static void msdc_request_done('))
        run_c(STUBS + between(t, '#define Y2_MANAGED_CAPS', 'static int y2_msdc_clock_ready(') + empty + r'''
int main(void) {
 struct msdc_host h = {0}; h.y2_sd = true; h.mmc.f_max = 200000000;
 h.y2_dt_caps = MMC_CAP_SD_HIGHSPEED | MMC_CAP_UHS; h.y2_ceiling = 0;
 y2_apply_level(&h, 3);            /* previous card failed 1.8 V */
 y2_sd_slot_empty(&h);             /* slot observed empty */
 assert(h.y2_level == 0 && (h.mmc.caps & MMC_CAP_UHS_SDR104));
 h.y2_ceiling = 3; y2_apply_level(&h, 3); y2_sd_slot_empty(&h);
 assert(h.y2_level == 3);          /* a boot ceiling is never exceeded */
 return 0;
}''')
        # Runtime PM retains every tuned register and the DDR clock mode.
        rpm = (PLATFORM / 'msdc-rpm-policy.h').read_text()
        for offset in ('0x00, /* MSDC_CFG */', '0x04, /* MSDC_IOCON */', '0xec, /* MSDC_PAD_TUNE */',
                       '0xf0, /* MSDC_DAT_RDDLY0 */', '0xf4, /* MSDC_DAT_RDDLY1 */'):
            self.assertIn(offset, rpm)
        run_c(r'''
#include <assert.h>
#include "msdc-rpm-policy.h"
int main(void) {
 unsigned ddr = (2u << 16) | (1u << 8) | 0x81; /* CKMOD=DDR, divider, CKSTB+MODE */
 assert(y2_msdc_cfg_restore(ddr) == ((2u << 16) | (1u << 8) | 0x01));
 return 0;
}''')


class StorageStatus(unittest.TestCase):
    def test_platform_status_reports_the_negotiated_mode_without_register_decoding(self):
        import sys
        sys.path.insert(0, str(ROOT / 'tools/platform'))
        from y2_platform.common import Context
        from y2_platform.observe import storage
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory)
            base = Path(directory) / 'sys/bus/platform/devices'
            for name, mode in (('11230000.mmc', 'host=emmc level=HS200 ceiling=HS200 timing=hs200 width=8 '
                                'clock_hz=200000000 actual_hz=200000000 cap_hz=200000000 signal_mv=1800 '
                                'vqmmc_mv=1800 card_level=HS200 card_caps=0x57 hs400_card=1 tuning=pass '
                                'tune_pad=0x80000 verify=pass fallbacks=0 crc_events=0 timeout_events=2 tuning_events=0 '
                                'verify_events=0\n'),
                               ('11240000.mmc', None)):
                (base / name).mkdir(parents=True)
                (base / name / 'y2_performance').write_text('cap_hz=200000000 actual_hz=200000000 transport_errors=0 fallbacks=0 clock_error=0\n')
                if mode:
                    (base / name / 'y2_storage').write_text(mode)
            controllers = {c['name']: c for c in storage(ctx)['controllers']}
            emmc = controllers['11230000.mmc']['mode']
            self.assertEqual(emmc['level'], 'HS200')
            self.assertEqual(emmc['width'], 8)
            self.assertEqual(emmc['signal_mv'], 1800)
            self.assertEqual(emmc['card_caps'], 0x57)
            self.assertEqual(emmc['tune_pad'], 0x80000)
            self.assertEqual(emmc['verify'], 'pass')
            self.assertEqual(emmc['tuning'], 'pass')  # result, not the event count
            self.assertEqual(emmc['timeout_events'], 2)
            self.assertIsNone(controllers['11240000.mmc']['mode'])  # older kernel
            self.assertEqual(controllers['11240000.mmc']['cap_hz'], 200000000)


class StorageDeviceTree(unittest.TestCase):
    def test_production_ceiling_and_rail_ownership(self):
        production = (ROOT / 'kernel/dts/innioasis-y2-production.dts').read_text()
        mmc0 = between(production, '&mmc0 {', '};')
        mmc1 = between(production, '&mmc1 {', '};')
        for prop in ('bus-width = <8>', 'max-frequency = <200000000>', 'mmc-hs200-1_8v',
                     'mmc-ddr-1_8v', 'vqmmc-supply = <&vio18>'):
            self.assertIn(prop, mmc0)
        self.assertNotIn('vmmc-supply', mmc0)       # the root eMMC is never power-cycled
        self.assertNotIn('hs400', mmc0)
        for prop in ('bus-width = <4>', 'max-frequency = <200000000>', 'sd-uhs-sdr104',
                     'sd-uhs-ddr50', 'sd-uhs-sdr50', 'vmmc-supply = <&vmch_sd>', 'vqmmc-supply = <&vmc_sd>'):
            self.assertIn(prop, mmc1)
        self.assertNotIn('ldo_vemc3v3', production)
        self.assertRegex(production, r'ldo_vio18 \{[^}]*regulator-always-on')
        self.assertRegex(production, r'ldo_vmc \{[^}]*1800000[^}]*3300000')
        # The minimal first-boot profile declares no SD/eMMC rails at all.
        for name in ('innioasis-y2.dts', 'innioasis-y2-first-boot.dts'):
            text = (ROOT / 'kernel/dts' / name).read_text()
            for rail in ('ldo_vmc', 'ldo_vmch', 'ldo_vio18', 'ldo_vemc3v3'):
                self.assertNotIn(rail, text, (name, rail))

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'), 'needs actual build')
    def test_built_dtb_admits_only_the_reviewed_storage_modes_and_rails(self):
        from tools.validation.dev_dtb import check
        build = Path(os.environ['Y2_ARTIFACT_TEST_ROOT'])
        data = (build / 'y2.dtb').read_bytes()
        size = (build / 'initramfs.cpio.gz').stat().st_size
        check(data, size, production=True)
        for old, new in ((b'mmc-hs200-1_8v', b'mmc-hs400-1_8v'),       # no data strobe
                         (b'sd-uhs-sdr104', b'sd-uhs-sdr105'),         # unreviewed mode
                         (b'vqmmc-supply', b'vmmc-supply\0'[:12]),     # eMMC supply ownership
                         (b'regulator-always-on', b'regulator-always-xx')):
            with self.subTest(old=old):
                self.assertIn(old, data)
                with self.assertRaises((ValueError, KeyError)):
                    check(data.replace(old, new, 1), size, production=True)


if __name__ == '__main__':
    unittest.main()
