"""Exercise production USB snapshot guards, register widths and partial reads."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]


class UsbStateSnapshot(unittest.TestCase):
    def test_guards_widths_and_failures(self):
        source = r'''
#include <assert.h>
#include <string.h>
#include "usb_state.h"
struct fixture { unsigned reads, fail; };
static int rd(void *context, unsigned address, unsigned width, unsigned *value) {
    static const unsigned mac[] = {1,0x60,0x6c,6,8,0xb};
    static const unsigned sizes[] = {1,1,2,2,2,1};
    struct fixture *f=context;
    unsigned i=f->reads;
    assert(i<21);
    if(i<6) {assert(address==0x11200000+mac[i]);assert(width==sizes[i]);}
    else if(i<13) {assert(address==0x11210868+i-6);assert(width==1);}
    else {assert(address==0x11200204+16*(i-13));assert(width==2);}
    ++f->reads;
    if(f->reads==f->fail) return -16;
    *value=0x20+i;
    return 0;
}
int main(void) {
    for(unsigned scenario=0;scenario<37;++scenario) {
        struct fixture f={.fail=scenario>=16 ? scenario-15 : 0};
        struct y2_usb_state_io io={&f,rd};
        struct y2_platform_snapshot s={
            .power={.magic=Y2_PWRAP_MAGIC,.valid=7,.cid=0x2023,.vusb=0xc000},
            .clock={.valid=15,.mux=0x01010100,.pll=0xfd000001,.pll_power=0x80000001},
            .usb={.valid=0x1fffff,.values={123}}
        };
        switch(scenario) {
        case 1:s.power.magic=0;break;
        case 2:s.power.result=-110;break;
        case 3:s.power.valid=1;break;
        case 4:s.power.cid=0x6397;break;
        case 5:s.power.vusb=0x4000;break;
        case 6:s.clock.result=-16;break;
        case 7:s.clock.valid=7;break;
        case 8:s.clock.peri=1<<10;break;
        case 9:s.clock.mux|=1<<23;break;
        case 10:s.clock.mux^=1<<16;break;
        case 11:s.clock.pll&=~(1<<26);break;
        case 12:s.clock.pll&=~1U;break;
        case 13:s.clock.pll_power&=~1U;break;
        case 14:s.clock.pll_power|=2;break;
        case 15:s.clock.pll&=~(1<<24);break;
        }
        struct y2_platform_snapshot before=s;
        y2_usb_state_probe(&io,&s);
        assert(!memcmp(&before.power,&s.power,sizeof(s.power)));
        assert(!memcmp(&before.clock,&s.clock,sizeof(s.clock)));
        unsigned good=0;
        if(scenario>=1 && scenario<=15) {
            assert(!f.reads && s.usb.result==-19 && !s.usb.valid);
        } else {
            good=f.fail ? f.fail-1 : 21;
            assert(f.reads==(f.fail ? f.fail : 21));
            assert(s.usb.result==(f.fail ? -16 : 0));
            assert(s.usb.valid==(1U<<good)-1);
        }
        for(unsigned i=0;i<21;++i) assert(s.usb.values[i]==(i<good ? 0x20+i : 0));
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
