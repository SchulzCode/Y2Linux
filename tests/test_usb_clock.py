"""Exercise the production clock snapshot guards and partial-read failures."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]


class UsbClockSnapshot(unittest.TestCase):
    def test_guards_and_read_failures(self):
        source = r'''
#include <assert.h>
#include "usb_clock.h"
struct fixture { unsigned reads, fail; };
static int rd(void *context, unsigned address, unsigned *value) {
    static const unsigned expected[] = {
        0x10003018, 0x10000060, 0x10209220, 0x1020922c
    };
    struct fixture *f = context;
    assert(f->reads < 4 && address == expected[f->reads]);
    ++f->reads;
    if (f->reads == f->fail) return -16;
    *value = 0xabcd0000U + f->reads;
    return 0;
}
int main(void) {
    for (unsigned scenario = 0; scenario < 10; ++scenario) {
        struct fixture f = { .fail = scenario >= 6 ? scenario - 5 : 0 };
        struct y2_usb_clock_io io = { &f, rd };
        struct y2_pwrap_snapshot power = {
            .magic = Y2_PWRAP_MAGIC, .valid = 7, .cid = 0x2023, .vusb = 0xc000
        };
        struct y2_usb_clock_snapshot s = { .valid = 15, .peri = 99 };
        switch (scenario) {
        case 1: power.magic = 0; break;
        case 2: power.result = -110; break;
        case 3: power.valid = 1; break;
        case 4: power.cid = 0x6397; break;
        case 5: power.vusb = 0x4000; break;
        }
        y2_usb_clock_probe(&io, &power, &s);
        if (scenario >= 1 && scenario <= 5) {
            assert(!f.reads && s.result == -19 && !s.valid && !s.peri);
        } else {
            unsigned good = f.fail ? f.fail - 1 : 4;
            assert(f.reads == (f.fail ? f.fail : 4));
            assert(s.result == (f.fail ? -16 : 0));
            assert(s.valid == (1U << good) - 1);
            assert(s.peri == (good >= 1 ? 0xabcd0001 : 0));
            assert(s.mux == (good >= 2 ? 0xabcd0002 : 0));
            assert(s.pll == (good >= 3 ? 0xabcd0003 : 0));
            assert(s.pll_power == (good >= 4 ? 0xabcd0004 : 0));
        }
    }
}
'''
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'test.c').write_text(source)
            subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror',
                            '-I' + str(PROJECT / 'kernel/diagnostic'),
                            str(path / 'test.c'), '-o', str(path / 'test')], check=True)
            subprocess.run([str(path / 'test')], check=True)
