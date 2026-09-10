"""Fault-inject actual regmap callbacks, including PMIC writes, without hardware."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
class Transport(unittest.TestCase):
    def test_regmap_callbacks(self):
        src=(ROOT/'kernel/platform/pwrap.c').read_text()
        functions=src[src.index('static int wrap_reg_read('):src.index('static const struct regmap_config')]
        fixture=r'''
#include <assert.h>
#include <stdint.h>
#include "policy.h"
#include "pwrap.h"
#define EINVAL 22
#define EPERM 1
#define BIT(n) (1U<<(n))
struct y2_wrap {void *base;};
static unsigned mem[1024],phase,word=0xab00,polls,delays,commands,writes,acks,fault;
static unsigned readl(void *p) {
 unsigned off=(char*)p-(char*)mem;
 assert(off==0xa0);
 if(fault==1)return 0x00360000; /* unowned stale read: never ack */
 if(fault==2)return 0x00100000; /* lost INIT */
 if(phase==1) {if(fault==3||polls++<2)return 0x00340000;return 0x00360000|word;}
 if(phase==2) {if(fault==4||polls++<2)return 0x00340000;phase=0;}
 return 0x00300000;
}
static void writel(unsigned value,void *p) {
 unsigned off=(char*)p-(char*)mem;
 if(off==0xa4){assert(value==1&&phase==1);++acks;phase=0;return;}
 assert(off==0x9c&&!phase);++commands;polls=0;
 if(value&BIT(31)){++writes;word=value&65535;phase=2;}
 else phase=1;
}
static unsigned rd(void *p,unsigned off){return readl((char*)p+off);}
static void wr(void *p,unsigned off,unsigned v){writel(v,(char*)p+off);}
static void wait(void *p){(void)p;++delays;assert(delays<=2000);}
static struct y2_pwrap_io wrap_io(struct y2_wrap *w)
{return (struct y2_pwrap_io){w->base,rd,wr,wait};}
'''
        fixture+=functions+r'''
int main(void) {
 struct y2_wrap w={mem};unsigned value;
 for(fault=0;fault<5;++fault) {
  phase=polls=delays=commands=writes=acks=0;word=0xab00;
  int ret=wrap_reg_write(&w,0x330,0xaa00);
  if(!fault)assert(!ret&&word==0xaa00&&writes==1&&acks==1);
  if(fault==1||fault==2)assert(ret<0&&!commands&&!acks&&!writes);
  if(fault==3)assert(ret==-110&&commands==1&&!acks&&!writes);
  if(fault==4)assert(ret==-110&&writes==1&&acks==1);
 }
 fault=phase=polls=delays=commands=writes=acks=0;word=0xab00;
 assert(wrap_reg_write(&w,0x330,0xaa01)==-EPERM&&writes==0); /* mode bits */
 commands=acks=0;
 for(unsigned reg=0;reg<4096;++reg)if(!y2_pmic_write_mask(reg))
  assert(wrap_reg_write(&w,reg,65535)==-EPERM&&!commands);
 assert(wrap_reg_read(&w,1,&value)==-EINVAL&&!commands);
 assert(!wrap_reg_write(&w,0x160,1)&&writes==1); /* mask, no read needed */
 assert(!wrap_reg_read(&w,0x160,&value)&&value==1);
 phase=polls=delays=commands=writes=acks=0;word=0x0055;
 assert(!wrap_reg_write(&w,0x50c,0x8055)&&word==0x8055&&writes==1);
 assert(wrap_reg_write(&w,0x50c,0x8054)==-EPERM); /* enable, not unrelated mode */
 phase=polls=delays=commands=writes=acks=0;word=0xff1f;
 assert(!wrap_reg_write(&w,0x532,0xff5f)&&word==0xff5f&&writes==1);
 assert(wrap_reg_write(&w,0x532,0xfe5f)==-EPERM); /* selector, not adjacent rail */

}
'''
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'test.c').write_text(fixture)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',
                '-I'+str(ROOT/'kernel/platform'),'-I'+str(ROOT/'kernel/diagnostic'),
                str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
