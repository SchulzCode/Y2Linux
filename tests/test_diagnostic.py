"""Exercise the actual pure diagnostic guard/pixel policy with hostile states."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]

class DiagnosticPolicy(unittest.TestCase):
    def test_guard_and_pixel_bounds(self):
        source = r'''
#include <assert.h>
#include <stdint.h>
#include "policy.h"
int main(void) {
    struct y2_snapshot s = {0x2200005c, 1, 12, 0x016801e0, 0x1000,
        0x016801e0, 0, Y2_FB_BASE, 960, 1, 2, 1}, bad;
    assert(y2_valid(&s));
    /* Any single-bit corruption of address, size, position or ROI must fail. */
    for (unsigned bit=0; bit<32; ++bit) {
        bad=s; bad.addr ^= 1U<<bit; assert(!y2_valid(&bad));
        bad=s; bad.size ^= 1U<<bit; assert(!y2_valid(&bad));
        bad=s; bad.offset ^= 1U<<bit; assert(!y2_valid(&bad));
        bad=s; bad.roi ^= 1U<<bit; assert(!y2_valid(&bad));
    }
    bad=s; bad.wdt_mode |= 1; assert(!y2_valid(&bad));
    bad=s; bad.addr=0xbfc02c00; assert(!y2_valid(&bad));
    bad=s; bad.addr=0xbf700000; assert(!y2_valid(&bad));
    bad=s; bad.pitch=1920; assert(!y2_valid(&bad));
    bad=s; bad.src_con=8; assert(!y2_valid(&bad));
    bad=s; bad.ovl_en=0; assert(!y2_valid(&bad));
    bad=s; bad.dsi_start=0; assert(!y2_valid(&bad));
    bad=s; bad.dsi_ctrl=3; assert(!y2_valid(&bad));
    for (unsigned mode=0;mode<4;++mode) {
        bad=s; bad.dsi_mode=mode; assert(y2_valid(&bad)==(mode==1));
    }
    for(unsigned bit=16;bit<=20;++bit) {
        bad=s; bad.dsi_mode |= 1U<<bit; assert(!y2_valid(&bad));
    }
    for(unsigned bit=0;bit<32;++bit) {
        if (0x70fff100U & (1U<<bit)) {
            bad=s; bad.con ^= 1U<<bit; assert(!y2_valid(&bad));
        }
    }
    assert(y2_next_stage(0,0,'I')==1);
    assert(y2_next_stage(0,0,'H')==-1 && y2_next_stage(0,0,'F')==-1);
    for(unsigned i=0;i<Y2_HEARTBEATS;++i) {
        assert(y2_next_stage(1,i,'H')==(int)(2+(i&1)));
        assert(y2_next_stage(1,i,'F')==-1);
        assert(y2_next_stage(1,i,'I')==-1);
    }
    assert(y2_next_stage(1,Y2_HEARTBEATS,'H')==-1);
    assert(y2_next_stage(1,Y2_HEARTBEATS,'F')==4);
    assert(y2_next_stage(2,Y2_HEARTBEATS,'H')==-1);
    assert(y2_next_stage(2,Y2_HEARTBEATS,'F')==-1);
    assert(y2_next_stage(1,0,'X')==-1);
    assert(Y2_FB_BASE + Y2_FB_BYTES == 0xbfb54600U);
    uint16_t pixels[Y2_FB_BYTES/2 + 2];
    for(unsigned stage=0;stage<5;++stage) {
        pixels[0]=0xdead; pixels[Y2_FB_BYTES/2+1]=0xbeef;
        for(unsigned y=0;y<Y2_HEIGHT;++y)
            for(unsigned x=0;x<Y2_WIDTH;++x) {
                unsigned off=(y*Y2_STRIDE+x*2)/2;
                assert(off<Y2_FB_BYTES/2);
                pixels[1+off]=y2_pixel(stage,x,y);
            }
        assert(pixels[0]==0xdead && pixels[Y2_FB_BYTES/2+1]==0xbeef);
    }
    assert(y2_pixel(0,0,0)==0 && y2_pixel(0,40,0)==0xffff);
    assert(y2_pixel(1,0,0)==0x07e0 && y2_pixel(1,479,359)==0x07e0);
    assert(y2_pixel(2,0,0)==0xffff && y2_pixel(2,479,359)==0x07e0);
    assert(y2_pixel(3,0,0)==0x07e0 && y2_pixel(3,479,359)==0xffff);
    assert(y2_pixel(4,0,0)==0 && y2_pixel(4,0,40)==0xffff);
}
'''
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)
            (path/'test.c').write_text(source)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',
                '-I'+str(PROJECT/'kernel/diagnostic'),str(path/'test.c'),'-o',str(path/'test')],check=True)
            subprocess.run([str(path/'test')],check=True)
