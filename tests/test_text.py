"""Production renderer bounds, packet rejection and bounded proc parsing."""
from pathlib import Path
import subprocess
import tempfile
import unittest

PROJECT=Path(__file__).resolve().parents[1]
class TextPolicy(unittest.TestCase):
    def test_packets_pixels_and_fields(self):
        source=r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include "status.h"
int main(void) {
    struct y2_text_frame f; char out[16];
    memset(&f,32,sizeof(f));f.magic=Y2_TEXT_MAGIC;
    assert(sizeof(f)==804 && y2_text_valid(&f));
    f.magic^=1;assert(!y2_text_valid(&f));f.magic=Y2_TEXT_MAGIC;
    for(unsigned i=0;i<Y2_ROWS*Y2_COLS;++i) {
        char *p=&f.rows[i/Y2_COLS][i%Y2_COLS];
        *p=0;assert(!y2_text_valid(&f));*p=127;assert(!y2_text_valid(&f));
        *p=(char)255;assert(!y2_text_valid(&f));*p=' ';
    }
    for(unsigned c=32;c<127;++c) {f.rows[0][0]=(char)c;assert(y2_text_valid(&f));}
    row_pair(f.rows[0],"PID1: ","1");
    row_pair(f.rows[1],"STAGE: ","HEARTBEAT");
    row_pair(f.rows[2],"LINUX: ","6.18.0-y2-m1");
    uint16_t buffer[Y2_FB_BYTES/2+2];buffer[0]=0xdead;buffer[Y2_FB_BYTES/2+1]=0xbeef;
    unsigned lit=0;
    for(unsigned y=0;y<Y2_HEIGHT;++y) for(unsigned x=0;x<Y2_WIDTH;++x) {
        unsigned off=(y*Y2_STRIDE+x*2)/2;
        assert(off<Y2_FB_BYTES/2);
        unsigned pixel=y2_text_pixel(&f,x,y);
        assert(pixel==0 || pixel==0xffff || pixel==0x07e0);
        if(pixel) ++lit;
        buffer[off+1]=(uint16_t)pixel;
    }
    assert(lit>1000 && buffer[0]==0xdead && buffer[Y2_FB_BYTES/2+1]==0xbeef);
    assert(y2_text_pixel(&f,480,0)==0 && y2_text_pixel(&f,0,360)==0);
    assert(y2_glyph('A',0)==14 && y2_glyph('a',0)==14);
    assert(y2_glyph('A',7)==0);
    assert(y2_glyph('~',2)==9 && y2_glyph('~',3)==22);
    assert(field(out,sizeof(out),"MemFree: 3 kB\nMemTotal: 21000 kB\n","MemTotal")==0);
    assert(!strcmp(out,"21000 kB"));
    assert(field(out,sizeof(out),"CPU part\t: 0xc07\n","CPU part")==0);
    assert(!strcmp(out,"0xc07"));
    assert(field(out,sizeof(out),"MemTotalBogus: 8\n","MemTotal")==-61);
    assert(field(out,4,"A: 12345\n","A")==-75);
    assert(irq_count(out,sizeof(out)," 1: 77 GIC x other\n 2: 1024 GIC mtk-clkevt\n")==0);
    assert(!strcmp(out,"1024"));
    assert(irq_count(out,sizeof(out),"IPI0: 1 unrelated\n")==-61);
    assert(irq_count(out,2,"1: 999 mtk-clkevt\n")==-75);
    row_pair(f.rows[19],"ABCDEFGHIJKLMNOPQRSTUVWXYZ","ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    assert(f.rows[19][39]=='~' && y2_text_valid(&f));
    row_code(f.rows[0],"ERR ",-2147483647L-1);
    assert(!memcmp(f.rows[0],"ERR -2147483648",15));
    assert(be32((const unsigned char*)"\x80\0\0\0")==0x80000000U);
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text(source)
            subprocess.run(['clang','-O1','-g','-fsanitize=undefined,bounds','-fsanitize-trap=all',
                '-Wall','-Wextra','-Werror','-Wno-unused-function',
                '-I'+str(PROJECT/'initramfs'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
