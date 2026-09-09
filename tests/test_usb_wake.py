"""Production PHY wake guards, one-write bound, readback and fault behavior."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]


class UsbWake(unittest.TestCase):
    def test_guarded_single_release(self):
        source = r'''
#include <assert.h>
#include <stddef.h>
#include <string.h>
#include "usb_wake.h"
struct fixture { unsigned scenario, reads, fail, writes, delays, phy[0x70]; };
static int rd(void *context,unsigned address,unsigned width,unsigned *value) {
    static const unsigned pre[]={0,5,0x15,0x1a,0x1d,0x22,0x63};
    struct fixture *f=context;
    unsigned i=f->reads,expected;
    assert(width==1 && i<19);
    if(i<7) expected=0x11210800+pre[i];
    else {
        unsigned a=i-7;
        assert(f->writes==1 && f->delays==1);
        expected=a<7 ? 0x11210868+a : a<10 ? 0x11210800+pre[a-7] :
                 a==10 ? 0x11200001 : 0x11200060;
    }
    assert(address==expected);
    if(++f->reads==f->fail) return -12;
    if(address==0x11200001) *value=f->scenario==45 ? 0x60 : 0x20;
    else if(address==0x11200060) *value=f->scenario==46 ? 0x84 : f->scenario==47 ? 0x98 : 0x80;
    else *value=f->phy[address-0x11210800];
    return 0;
}
static int release(void *context,unsigned *written) {
    struct fixture *f=context;
    assert(f->reads==7 && !f->writes && !*written);
    if(f->scenario==28) return -16;
    assert(f->phy[0x6a]==4);
    f->phy[0x6a]=0;f->writes=1;*written=1;
    return 0;
}
static void settle(void *context) {
    struct fixture *f=context;
    assert(f->writes==1 && !f->delays);++f->delays;
    if(f->scenario==41) f->phy[0x6a]=4;
    if(f->scenario==42) f->phy[0]^=1;
    if(f->scenario==43) f->phy[5]^=1;
    if(f->scenario==44) f->phy[0x15]^=1;
}
int main(void) {
    for(unsigned scenario=0;scenario<49;++scenario) {
        struct fixture f={.scenario=scenario};
        f.phy[0]=0x11;f.phy[5]=0x22;f.phy[0x15]=0x40;
        f.phy[0x1a]=0x10;f.phy[0x63]=2;f.phy[0x6a]=4;f.phy[0x6b]=2;f.phy[0x6c]=0x12;
        struct y2_platform_snapshot s={
            .power={.magic=Y2_PWRAP_MAGIC,.valid=7,.cid=0x2023,.vusb=0xc000},
            .clock={.valid=15,.mux=0x01010100,.pll=0xfd000001,.pll_power=0x80000001},
            .usb={.valid=0x1fffff,.values={0x20,0x80,0x6503,0x1ff,0x1fe,6,0,0,4,2,0x12,0,0}},
            .wake={.written=99,.before_valid=99,.after_valid=99}
        };
        switch(scenario) {
        case 1:s.power.result=-19;break;
        case 2:s.usb.result=-5;break;
        case 3:s.usb.valid=1;break;
        case 4:s.usb.values[0]=0x60;break;
        case 5:s.usb.values[1]=0x84;break;
        case 6:s.usb.values[2]=0x1900;break;
        case 7:s.usb.values[6]=8;break;
        case 8:s.usb.values[7]=4;break;
        case 9:s.usb.values[8]=0;break;
        case 10:s.usb.values[9]=6;break;
        case 11:s.usb.values[10]=0x10;break;
        case 12:s.usb.values[11]=2;break;
        case 13:s.usb.values[12]=1;break;
        case 14:s.usb.values[13]=1;break;
        case 15:s.usb.values[20]=1;break;
        case 16:f.phy[0x15]|=0x80;break;
        case 17:f.phy[0x1a]|=0x80;break;
        case 18:f.phy[0x1d]|=0x10;break;
        case 19:f.phy[0x22]|=1;break;
        case 20:f.phy[0x22]|=2;break;
        case 48:s.usb.values[8]=0xbe;break; /* observed connected-start refusal */
        }
        if(scenario>=21 && scenario<=27) f.fail=scenario-20;
        if(scenario>=29 && scenario<=40) f.fail=scenario-21;
        struct y2_platform_snapshot before=s;
        struct y2_usb_wake_io io={&f,rd,release,settle};
        y2_usb_wake_probe(&io,&s);
        assert(!memcmp(&s,&before,offsetof(struct y2_platform_snapshot,wake)));
        assert(s.wake.written==f.writes && f.writes<=1 && f.delays==f.writes);
        if((scenario>=1 && scenario<=15) || scenario==48) {
            assert(!f.reads && !f.writes && s.wake.result==-19 && !s.wake.before_valid);
        } else if(scenario>=16 && scenario<=20) {
            assert(f.reads==7 && !f.writes && s.wake.result==-19 && s.wake.before_valid==0x7f);
        } else if(scenario==28) {
            assert(f.reads==7 && !f.writes && s.wake.result==-16);
        } else if(f.fail) {
            assert(f.reads==f.fail && s.wake.result==-12);
            unsigned good=f.fail-1;
            assert(s.wake.before_valid==(good<7 ? (1U<<good)-1 : 0x7f));
            assert(s.wake.after_valid==(good<7 ? 0 : (1U<<(good-7))-1));
            assert(f.writes==(good>=7));
        } else {
            assert(f.reads==19 && f.writes==1 && s.wake.before_valid==0x7f && s.wake.after_valid==0xfff);
            assert(s.wake.result==(scenario>=41 && scenario<=46 ? -5 : 0));
        }
        if(!f.writes) assert(!s.wake.after_valid);
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
