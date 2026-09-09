"""Fault-inject the production PWRAP read protocol; never access hardware."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]


class PwrapProtocol(unittest.TestCase):
    def test_read_only_and_fault_bounds(self):
        source = r'''
#include <assert.h>
#include "pwrap.h"
struct fixture {
    unsigned fault, commands, acks, reads, delays, phase, polls;
};
static unsigned rd(void *ctx, unsigned reg) {
    struct fixture *f=ctx; ++f->reads; assert(f->reads<2200);
    switch(reg) {
    case 0: return f->fault==1 ? 1 : 0;
    case 4: return f->fault==2 ? 0 : 1;
    case 0x50: return f->fault==3 ? 0x1f7 : 0x1ff;
    case 0x94: return f->fault==4 ? 0 : 1;
    case 0x98: return f->fault==5 ? 0 : 1;
    case 0xa0: break;
    default: assert(0);
    }
    if (!f->commands) {
        switch(f->fault) {
        case 6: return Y2_PWRAP_SYNC; /* init absent */
        case 7: return Y2_PWRAP_INIT; /* sync absent */
        case 8: return 0x00380000; /* request busy */
        case 9: return 0x00320000;
        case 10: return 0x00340000;
        case 11: return 0x00360000; /* stale completion: DO NOT ACK */
        case 12: return 0xffffffff;
        }
    }
    if (f->phase==1) {
        ++f->polls;
        if(f->fault==13 || (f->fault==17 && f->commands==2) || (f->fault==21 && f->commands==3)) return 0x00340000;
        if(f->fault==14 || (f->fault==22 && f->commands==3)) return 0x00160000; /* lost init */
        if(f->fault==15 || (f->fault==23 && f->commands==3)) return 0x00370000; /* invalid FSM */
        if(f->polls<3) return 0x00340000; /* real completion requires polling */
        return 0x00360000 | (f->commands==1 ?
               (f->fault==16 ? 0x6397 : 0x2023) :
               f->commands==2 ? 0xc001 : f->fault==20 ? 0xa500 : 0xa520);
    }
    if (f->phase==2 && (f->fault==18 || (f->fault==24 && f->commands==3))) return 0x00360000;
    if (f->phase==2 && (f->fault==19 || (f->fault==25 && f->commands==3))) return 0x00100000;
    return 0x00300000;
}
static void wr(void *ctx, unsigned reg, unsigned value) {
    struct fixture *f=ctx;
    if(reg==0x9c) {
        assert(!(value & 0x80000000U)); /* never a PMIC write */
        assert(f->commands==f->acks && f->commands<3);
        assert(value==(f->commands==0 ? 0x00800000 : f->commands==1 ? 0x02810000 : 0x00000000));
        ++f->commands; f->phase=1; f->polls=0;
    } else {
        assert(reg==0xa4 && value==1 && f->phase==1 && f->polls>=3);
        ++f->acks; f->phase=2;
    }
}
static void delay(void *ctx) { ++((struct fixture *)ctx)->delays; }
int main(void) {
    for(unsigned fault=0;fault<26;++fault) {
        struct fixture f={.fault=fault};
        struct y2_pwrap_io io={&f,rd,wr,delay};
        struct y2_pwrap_snapshot s;
        y2_pwrap_probe(&io,&s);
        assert(s.magic==Y2_PWRAP_MAGIC);
        assert(f.delays<=Y2_PWRAP_POLLS+6);
        if(fault>=1 && fault<=12) {
            assert(s.result<0 && !s.valid && !f.commands && !f.acks);
        } else if(fault==13 || fault==14 || fault==15) {
            assert(s.result<0 && !s.valid && f.commands==1 && !f.acks);
        } else if(fault==16) {
            assert(s.result==-19 && s.valid==1 && s.cid==0x6397);
            assert(f.commands==1 && f.acks==1);
        } else if(fault==17) {
            assert(s.result==-110 && s.valid==1 && f.commands==2 && f.acks==1);
        } else if(fault==18 || fault==19) {
            assert(s.result<0 && !s.valid && f.commands==1 && f.acks==1);
        } else if(fault>=21 && fault<=23) {
            assert(s.result==(fault==21 ? -110 : -5) && s.valid==3);
            assert(f.commands==3 && f.acks==2 && !s.chrdet);
        } else if(fault==24 || fault==25) {
            assert(s.result==(fault==24 ? -110 : -5) && s.valid==3);
            assert(f.commands==3 && f.acks==3); /* raw response is not yet valid */
        } else {
            assert(!s.result && s.valid==7 && s.cid==0x2023 && s.vusb==0xc001);
            assert(f.commands==3 && f.acks==3 && s.after==0x00300000);
            assert(s.chrdet==(fault==20 ? 0xa500U : 0xa520U));
            assert(((s.chrdet >> 5) & 1)==(fault!=20));
        }
        if(fault==13 || fault==17 || fault==18) assert(s.result==-110);
        unsigned value=0,state=0,reads=f.reads;
        assert(y2_pwrap_read_pmic(&io,0x102,&value,&state)==-22);
        assert(f.reads==reads); /* reject arbitrary-address access before IO */
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
