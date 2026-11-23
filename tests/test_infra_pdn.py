"""POWER-FINAL-06: INFRASYS/DDRPHY power-down (vendor infra_pdn=1) save/restore.

The policy header is exercised against an emulated PERICFG block (including the
SET/CLR/STA gate triple); the platform driver's real functions are extracted and
run against fake MMIO windows.
"""
from pathlib import Path
import re
import unittest

from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'
POLICY = (PLATFORM / 'infra-pdn-policy.h').read_text()
DRIVER = (PLATFORM / 'infra-pdn.c').read_text()


class Policy(unittest.TestCase):
    def test_restore_writes_only_what_the_power_down_reset(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#include "spm-policy.h"
''' + POLICY.replace('#include "spm-policy.h"', '') + r'''
static unsigned mem[0x500 / 4], writes, write_log[64][2], stuck_off = ~0U;
static unsigned rd(void *c, unsigned reg) { (void)c; assert(reg < 0x500 && !(reg & 3)); return mem[reg / 4]; }
static void wr(void *c, unsigned reg, unsigned v) {
    (void)c; assert(reg < 0x500 && !(reg & 3));
    write_log[writes][0] = reg; write_log[writes][1] = v; ++writes;
    if (reg == 0x08) mem[0x18 / 4] |= v;			/* PDN0 SET */
    else if (reg == 0x10) mem[0x18 / 4] &= ~v;		/* PDN0 CLR */
    else if (reg == 0x18) assert(!"STA is read-only");
    else if (reg != stuck_off) mem[reg / 4] = v;
}
static void dl(unsigned us) { (void)us; }
static struct y2_spm_io io = { 0, rd, wr, dl };
static unsigned saved[Y2_INFRA_MAX_REGS], woke[Y2_INFRA_MAX_REGS];
static void program(void) {
    memset(mem, 0, sizeof mem);
    mem[0x000 / 4] = 0x00000000; mem[0x004 / 4] = 0x00000010;
    mem[0x018 / 4] = 0x00f00800;	/* MSDC2/I2C2/..., APDMA gated */
    mem[0x050 / 4] = 0x00000081; mem[0x054 / 4] = 0x000000ff; mem[0x058 / 4] = 0x00000003;
    mem[0x05c / 4] = 0x00000001; mem[0x204 / 4] = 0x101fffff; mem[0x304 / 4] = 0x00010101;
    mem[0x314 / 4] = 0x00020202; mem[0x320 / 4] = 0x1; mem[0x400 / 4] = 0x00000102; mem[0x40c / 4] = 0x00000001;
}
static void power_down(void) {	/* INFRASYS reset: PDN0 ungated, everything else at reset value */
    memset(mem, 0, sizeof mem);
    mem[0x054 / 4] = 0xff; mem[0x204 / 4] = 0x101fffff;
}
int main(void) {
    struct y2_infra_result r;
    unsigned i;
    /* Register table sanity: PDN0 is the only gate register, offsets unique and aligned. */
    assert(Y2_PERICFG_REGS <= Y2_INFRA_MAX_REGS);
    for (i = 0; i < Y2_PERICFG_REGS; i++) {
        unsigned j;
        assert(!(y2_pericfg_regs[i].offset & 3) && y2_pericfg_regs[i].offset < 0x500);
        for (j = 0; j < i; j++) assert(y2_pericfg_regs[j].offset != y2_pericfg_regs[i].offset);
        assert((y2_pericfg_regs[i].kind == Y2_INFRA_PDN0) == (y2_pericfg_regs[i].offset == 0x18));
    }
    /* Everything lost: restored exactly, gates through SET (never by writing STA). */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved);
    unsigned golden[0x500 / 4]; memcpy(golden, mem, sizeof mem);
    power_down(); writes = 0;
    assert(!y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r));
    assert(!r.failed && r.lost && memcmp(golden, mem, sizeof mem) == 0);
    for (i = 0; i < writes; i++) assert(write_log[i][0] != 0x10 && write_log[i][0] != 0x18);
    assert(write_log[0][0] != 0x08 || write_log[0][1] == 0x00f00800);
    unsigned set_writes = 0;
    for (i = 0; i < writes; i++) if (write_log[i][0] == 0x08) { ++set_writes; assert(write_log[i][1] == 0x00f00800); }
    assert(set_writes == 1);
    assert(woke[2] == 0 && woke[0] == 0);	/* what the wake actually found */
    /* Nothing lost (aborted entry, or a domain that kept its state): zero writes. */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved); writes = 0;
    assert(!y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r));
    assert(!writes && !r.lost && !r.failed);
    /* Only the gates reset. */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved);
    mem[0x018 / 4] = 0; writes = 0;
    assert(!y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r));
    assert(writes == 1 && write_log[0][0] == 0x08 && r.lost == (1U << 2));
    /* Hardware gated more than before: those bits are cleared, none of the others touched. */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved);
    mem[0x018 / 4] = 0x00f00800 | 0x3; writes = 0;
    assert(!y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r));
    assert(writes == 1 && write_log[0][0] == 0x10 && write_log[0][1] == 0x3 && mem[0x018 / 4] == 0x00f00800);
    /* A verified register that does not take its value fails the restore. */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved);
    power_down(); mem[0x054 / 4] = 0; stuck_off = 0x054;
    assert(y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r) == -EIO);
    assert(r.failed == (1U << 4) && (r.lost & (1U << 4)));
    stuck_off = ~0U;
    /* An unverified register may legitimately read back differently. */
    program(); y2_infra_save(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved);
    power_down(); stuck_off = 0x050;
    assert(!y2_infra_restore(&io, y2_pericfg_regs, Y2_PERICFG_REGS, saved, woke, &r) && !r.failed && r.lost);
    stuck_off = ~0U;
    return 0;
}
''')

    def test_register_table_only_lists_documented_plain_registers(self):
        offsets = [int(m, 16) for m in re.findall(r'\{ (0x[0-9a-f]+), Y2_INFRA_', POLICY)]
        # PERICFG documented read/write registers; no MD1/MD2 gates, no status-only registers.
        self.assertEqual(offsets, [0x000, 0x004, 0x018, 0x050, 0x054, 0x058, 0x05c, 0x204,
                                   0x304, 0x314, 0x320, 0x324, 0x328, 0x32c, 0x400, 0x40c])
        for forbidden in (0x020, 0x024, 0x028, 0x02c, 0x030, 0x034, 0x038, 0x210, 0x208, 0x20c):
            self.assertNotIn(forbidden, offsets)


def function(source, name):
    """Extract one function definition, static or exported."""
    start = re.search(r'^(?:static )?[A-Za-z_][^;{}]*\b' + name + r'\([^;{}]*\)\n\{', source, re.M).start()
    end = source.index('{', start)
    depth = 1
    end += 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end] + '\n'


def _driver_tables():
    start = DRIVER.index('#define Y2_PERICFG_PHYS')
    end = DRIVER.index('static unsigned peri_read')
    return DRIVER[start:end]


class Driver(unittest.TestCase):
    def test_real_driver_functions_against_fake_blocks(self):
        policy = POLICY.replace('#include "spm-policy.h"', '')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <sys/types.h>
#include "spm-policy.h"
#define __iomem
#define __init
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define BIT(n) (1U<<(n))
#define hweight32(x) ((unsigned)__builtin_popcount(x))
#define DEFINE_MUTEX(x) int x
#define mutex_lock(x) do{assert(!*(x));*(x)=1;}while(0)
#define mutex_unlock(x) do{assert(*(x));*(x)=0;}while(0)
#define dsb(x) ((void)0)
#define udelay(x) ((void)(x))
#define pr_info(...) ((void)0)
typedef struct kobject { int unused; } kobject;
struct kobj_attribute { int unused; };
static int sysfs_streq(const char *a, const char *b) {
    size_t n = strlen(b);
    return !strncmp(a, b, n) && (a[n] == 0 || (a[n] == '\n' && a[n + 1] == 0));
}
static const char *last_note; static int last_note_result;
static void y2_pm_note(const char *name, int result) { last_note = name; last_note_result = result; }
''' + policy + r'''
static unsigned char peri_mem[0x500], infra_mem[0x20];
static unsigned peri_stuck, infra_reads, peri_reads_during_save;
static void put32(unsigned char *m, unsigned off, unsigned v) { memcpy(m + off, &v, 4); }
static unsigned get32(const unsigned char *m, unsigned off) { unsigned v; memcpy(&v, m + off, 4); return v; }
static unsigned readl(const void *p) {
    unsigned v;
    if ((const unsigned char *)p >= infra_mem && (const unsigned char *)p < infra_mem + sizeof infra_mem) ++infra_reads;
    memcpy(&v, p, 4); return v;
}
static void writel(unsigned v, void *p) {
    unsigned char *c = p;
    assert(!(c >= infra_mem && c < infra_mem + sizeof infra_mem)); /* INFRACFG is never written */
    if (c >= peri_mem && c < peri_mem + 0x500) {
        unsigned off = c - peri_mem;
        if (off == 0x08) { put32(peri_mem, 0x18, get32(peri_mem, 0x18) | v); return; }
        if (off == 0x10) { put32(peri_mem, 0x18, get32(peri_mem, 0x18) & ~v); return; }
        if (off == peri_stuck) return;
    }
    memcpy(p, &v, 4);
}
''' + _driver_tables() + r'''
static unsigned peri_read(void *context, unsigned reg) { return readl((char *)context + reg); }
static void peri_write(void *context, unsigned reg, unsigned value) { writel(value, (char *)context + reg); }
static void peri_delay(unsigned us) { (void)us; }
''' + function(DRIVER, 'y2_infra_pdn_wanted') + function(DRIVER, 'y2_infra_pdn_save')
        + function(DRIVER, 'record_fallback') + function(DRIVER, 'y2_infra_pdn_restore')
        + function(DRIVER, 'mode_store') + r'''
static void configure(unsigned pdn0) {
    memset(peri_mem, 0, sizeof peri_mem); memset(infra_mem, 0, sizeof infra_mem);
    put32(peri_mem, 0x018, pdn0); put32(peri_mem, 0x054, 0xff); put32(peri_mem, 0x05c, 1); put32(peri_mem, 0x400, 0x102);
    put32(infra_mem, 0x000, 0x300); put32(infra_mem, 0x004, 0x1); put32(infra_mem, 0x01c, 0x5);
    pericfg = peri_mem; infracfg = infra_mem;
    peri_io = (struct y2_spm_io){ .context = pericfg, .read = peri_read, .write = peri_write, .delay = peri_delay };
    ready = true; powerdown_mode = true; fallback = false; fallback_reason = "none"; peri_stuck = ~0U;
    saved_valid = false; infra_reads = 0;
}
int main(void) {
    int ret;
    (void)sysfs_streq; (void)peri_reads_during_save;
    /* Not wanted -> nothing is touched. */
    configure(0); ready = false;
    assert(!y2_infra_pdn_wanted() && y2_infra_pdn_save() == -ENODEV && !saves && !infra_reads);
    configure(0); powerdown_mode = false; assert(!y2_infra_pdn_wanted() && y2_infra_pdn_save() == -ENODEV);
    configure(0); fallback = true; assert(!y2_infra_pdn_wanted() && y2_infra_pdn_save() == -ENODEV && !infra_reads);
    /* Save records PERICFG and the read-only INFRACFG. */
    configure(0x800 | (1U << 12));
    assert(y2_infra_pdn_wanted() && !y2_infra_pdn_save() && saves == 1 && saved_valid);
    assert(reg_saved[PDN0_INDEX] == (0x800 | (1U << 12)) && fabric_saved[0] == 0x300 && fabric_saved[2] == 0x5);
    /* INFRASYS power-down reset PERICFG and INFRACFG. */
    memset(peri_mem, 0, sizeof peri_mem); put32(peri_mem, 0x054, 0xff);
    put32(infra_mem, 0x000, 0); put32(infra_mem, 0x004, 0);
    ret = y2_infra_pdn_restore();
    assert(!ret && restores == 1 && !restore_failures && !fallback && last_lost && !last_failed);
    assert(get32(peri_mem, 0x018) == (0x800 | (1U << 12)) && get32(peri_mem, 0x05c) == 1 && get32(peri_mem, 0x400) == 0x102);
    assert(fabric_lost == 3 && get32(infra_mem, 0x000) == 0);	/* reported, not written back */
    assert(!strcmp(last_note, "infra_restored") && !last_note_result);
    /* Restore with no preceding save is refused. */
    assert(y2_infra_pdn_restore() == -ENODEV);
    /* Nothing lost -> nothing written, nothing reported. */
    configure(0x800); assert(!y2_infra_pdn_save());
    ret = y2_infra_pdn_restore();
    assert(!ret && !last_lost && !fabric_lost && !strcmp(last_note, "infra_kept"));
    /* A verified register that does not take its value pins the boot to retained mode. */
    configure(0x800); assert(!y2_infra_pdn_save());
    memset(peri_mem, 0, sizeof peri_mem); peri_stuck = 0x054;
    assert(y2_infra_pdn_restore() == -EIO);
    assert(fallback && !y2_infra_pdn_wanted() && restore_failures == 1);
    assert(!strcmp(fallback_reason, "restore_verify_failed") && !strcmp(last_note, "infra_failed") && last_note_result == -EIO);
    assert(y2_infra_pdn_save() == -ENODEV);
    /* Owner switch: retained keeps the fallback, powerdown is an explicit retry. */
    assert(mode_store(NULL, NULL, "retained\n", 9) == 9 && !powerdown_mode && fallback);
    assert(mode_store(NULL, NULL, "powerdown", 9) == 9 && powerdown_mode && !fallback && y2_infra_pdn_wanted());
    assert(mode_store(NULL, NULL, "bogus\n", 6) == -EINVAL);
    return 0;
}
''')

    def test_driver_wiring(self):
        spm = (PLATFORM / 'spm.c').read_text()
        enter = function(spm, 'y2_spm_enter')
        finish = function(spm, 'y2_spm_finish')
        # save before arming; arm told the mode; restore right after the CPU returns,
        # after the backstop clock is running again.
        self.assertLess(enter.index('y2_infra_pdn_save()'), enter.index('y2_spm_suspend_arm('))
        self.assertIn('y2_spm_suspend_arm(&spm_io, pcm_address, infra, Y2_SPM_TICKS(armed_seconds))', enter)
        self.assertLess(enter.index('cpu_suspend(0, y2_spm_finish)'), enter.index('y2_infra_pdn_restore()'))
        self.assertLess(enter.index('y2_pm_backstop_pause(false)'), enter.index('y2_infra_pdn_restore()'))
        self.assertLess(enter.index('y2_infra_pdn_restore()'), enter.index('resumes++'))
        # The journal mark that identifies a boot that died inside the attempt is
        # the last one before WFI, after BEFORE_SPM_ENTRY.
        self.assertLess(finish.index('Y2_PM_BEFORE_SPM_ENTRY, 0'), finish.index('Y2_PM_INFRA_PDN_ENTRY'))
        self.assertLess(finish.index('Y2_PM_INFRA_PDN_ENTRY'), finish.index('v7_exit_coherency_flush'))
        # Previous-boot detection uses exactly that stage; no automatic retry loop.
        self.assertIn('y2_pm_previous_stage_was(Y2_PM_INFRA_PDN_ENTRY)', DRIVER)
        self.assertIn('previous_boot_died_in_infra_pdn', DRIVER)

    def test_overlay_rearms_uart_sleep_after_system_resume(self):
        import json
        import sys
        sys.path.insert(0, str(ROOT))
        from tools.build.run import prepare_overlay
        spec = next(s for s in json.loads((ROOT / 'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/tty/serial/8250/8250_mtk.c')
        text = prepare_overlay(ROOT, spec).read_text()
        resume = text[text.index('static int __maybe_unused mtk8250_resume'):]
        resume = resume[:resume.index('static const struct dev_pm_ops')]
        self.assertLess(resume.index('serial8250_resume_port'), resume.index('mtk8250_enable_pio_sleep'))
        self.assertIn('pm_runtime_resume_and_get(dev)', resume)
        self.assertIn('pm_runtime_put(dev)', resume)


if __name__ == '__main__':
    unittest.main()
