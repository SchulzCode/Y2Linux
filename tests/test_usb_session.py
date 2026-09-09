"""Production takeover gate and bounded digital session transitions."""
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]


class Session(unittest.TestCase):
    def test_guards_and_transition(self):
        source = r'''
#include <assert.h>
#include "gate.h"
#include "session.h"
struct fixture {unsigned c,d,dev,reads,writes,delays,drop;};
static unsigned rd(void *p,unsigned off) {
    struct fixture *f=p;++f->reads;
    assert(off==0x6c || off==0x6d || off==0x100);
    return off==0x6c?f->c:off==0x6d?f->d:f->dev;
}
static void wr(void *p,unsigned off,unsigned v) {
    struct fixture *f=p;++f->writes;
    assert(off==0x6c || off==0x6d);
    if(f->writes==f->drop) return;
    if(off==0x6c) f->c=v;else f->d=v;
}
static void delay(void *p) {++((struct fixture*)p)->delays;}
int main(void) {
    struct y2_platform_snapshot good={
        .power={.magic=Y2_PWRAP_MAGIC,.valid=7,.cid=0x2023,.vusb=0xc001},
        .clock={.valid=15,.mux=0x01010100,.pll=0xfd000001,.pll_power=0x80000001},
        .usb={.valid=0x1fffff,.values={0x20,0x80,0x6503,0x1ff,0x1fe,6,0,0,0,2,0x12,0,0}}
    };
    assert(y2_usb_takeover_ready(&good));
    for(unsigned i=0;i<21;++i) {
        struct y2_platform_snapshot s=good;s.usb.valid^=1U<<i;
        assert(!y2_usb_takeover_ready(&s));
        if(i>=13) {s=good;s.usb.values[i]=1;assert(!y2_usb_takeover_ready(&s));}
    }
    for(unsigned i=0;i<12;++i) {
        struct y2_platform_snapshot s=good;
        switch(i) {
        case 0:s.power.result=-110;break;case 1:s.clock.result=-5;break;
        case 2:s.clock.peri=1<<10;break;case 3:s.usb.values[0]=0x60;break;
        case 4:s.usb.values[1]=0x84;break;case 5:s.usb.values[2]=0xffff;break;
        case 6:s.usb.values[6]=1;break;case 7:s.usb.values[7]=1;break;
        case 8:s.usb.values[8]=0xbe;break;case 9:s.usb.values[9]=6;break;
        case 10:s.usb.values[11]=1;break;case 11:s.usb.values[12]=1;break;
        }
        assert(!y2_usb_takeover_ready(&s));
    }
    for(unsigned i=0;i<9;++i) {
        struct fixture f={.c=0xd2,.dev=0x98};
        struct y2_session s;
        struct y2_session_io io={&f,rd,wr,delay};
        if(i==1) f.c=0x10;
        if(i==2) f.d=4;
        if(i==3) f.dev=0x80;
        if(i==4) f.dev=0x99;
        if(i>=5 && i<=7) f.drop=i-4;
        if(i==8) f.dev=0xd8; /* FS/LS status does not make this host mode */
        unsigned original=f.c;
        int rc=y2_session_start(&io,&s);
        if(i==1 || i==2) assert(rc==-19 && !f.writes && s.devctl==0x100);
        else {
            assert(s.written && f.writes==3);
            if(i==3) assert(rc==-110 && f.delays==1000);
            else if(i==4) assert(rc==-19 && !f.delays);
            else if(i>=5 && i<=7) assert(rc==-5 && !f.delays);
            else assert(!rc && !f.delays && f.c==0xee && f.d==0x3e);
            /* Relinquish only owned bits and preserve unrelated changes. */
            f.c^=0x40;f.d|=0x80;
            y2_session_end(&io,&s);
            assert(!s.written && f.writes==5 && f.c==(original^0x40) && f.d==0x80);
        }
        unsigned count=f.writes;y2_session_end(&io,&s);assert(f.writes==count);
    }
}
'''
        with tempfile.TemporaryDirectory() as d:
            path=Path(d);(path/'test.c').write_text(source)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',
                '-I'+str(ROOT/'kernel/usb'),str(path/'test.c'),'-o',str(path/'test')],check=True)
            subprocess.run([str(path/'test')],check=True)
