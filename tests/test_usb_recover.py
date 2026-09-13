"""Production saved-current recovery, fault injection and write containment."""
import subprocess, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class Recover(unittest.TestCase):
    def test_recover_connected_passive_and_failures(self):
        source=r'''
#include <assert.h>
#include <string.h>
#include "recover.h"
struct mock { unsigned phy[112], mac[0x280], writes, reads, fail, drop, delays; };
static int rd(void *p,unsigned a,unsigned width,unsigned *v) {
 struct mock *m=p;(void)width;
 if(++m->reads==m->fail) return -5;
 *v=a>=Y2_USB_PHY_BASE ? m->phy[a-Y2_USB_PHY_BASE] : m->mac[a-Y2_USB_MAC_BASE];return 0;
}
static int wr(void *p,unsigned a,unsigned v) {
 struct mock *m=p;unsigned off=a-Y2_USB_PHY_BASE;
 if(a<Y2_USB_PHY_BASE) {
  off=a-Y2_USB_MAC_BASE;assert(off==0xa4 || off==0x0b || off==0x06 || off==0x08 || off==0x01 || off==0x60);
  m->mac[off]=v;return 0;
 }
 assert(off==0x1a || off==0x1d || (off>=0x68 && off<=0x6b) || off==0x6e || off==0x6d);
 ++m->writes;if(m->writes!=m->drop) m->phy[off]=v;return 0;
}
static void delay(void *p,unsigned us) { struct mock *m=p;assert(us==50 || us==800);m->delays++; }
static void init(struct mock *m,struct y2_platform_snapshot *s,int saved) {
 memset(m,0,sizeof(*m));
 *s=(struct y2_platform_snapshot){
 .power={.magic=Y2_PWRAP_MAGIC,.valid=7,.cid=0x2023,.vusb=0xc000,.chrdet=0x7b},
 .clock={.valid=15,.mux=0x01010100,.pll=0xfd000001,.pll_power=0x80000001}};
 m->mac[1]=0x20;m->mac[0x60]=0x80;m->mac[0x6c]=0x6503;
 m->phy[0]=0x6e;m->phy[5]=0x44;m->phy[0x15]=0x10;m->phy[0x1a]=0x10;
 m->phy[0x6a]=saved ? 0xbe : 4;m->phy[0x68]=saved ? 0xf4 : 0;
 m->phy[0x69]=saved ? 0x3c : 0;m->phy[0x6b]=2;m->phy[0x6c]=0x12;
 struct y2_usb_state_io io={m,rd};y2_usb_state_probe(&io,s);m->reads=0;
}
int main(void) {
 struct mock m;struct y2_platform_snapshot s;struct y2_recover_io io={{&m,rd},wr,delay};
 for(unsigned saved=0;saved<2;saved++) for(unsigned cable=0;cable<2;cable++) {
  init(&m,&s,saved);s.power.chrdet=cable ? 0x7b : 0;
  y2_usb_recover(&io,&s);assert(!s.wake.result && s.wake.after_valid==0xfff);
  assert(m.writes==17 && m.delays==2 && s.wake.written==1);
  assert(m.phy[0]==0x6e && m.phy[5]==0x44 && m.phy[0x15]==0x10 && !m.phy[0x6d]);
 }
 for(unsigned bad=0;bad<6;bad++) {
  init(&m,&s,1);
  switch(bad) {
   case 0:s.power.result=-5;break;case 1:s.clock.valid=0;break;
   case 2:s.usb.values[1]=0x84;break;
   case 3:s.usb.values[2]=1;break;
   case 4:s.usb.values[13]=1;break;
   case 5:s.usb.values[20]=1;break;
  }
  y2_usb_recover(&io,&s);assert(s.wake.result && !m.writes);
 }
 /* Loader-forced peripheral values, dormant DMA config and unrelated bits
  * are preserved/normalized, not arbitrary refusal conditions. */
 init(&m,&s,1);m.mac[1]=0x60;m.mac[0x204]=0x1200;
 m.phy[0x6d]=0xbe;m.phy[0x6c]=0;m.phy[0x6a]|=0x40;
 {struct y2_usb_state_io state={&m,rd};y2_usb_state_probe(&state,&s);}
 y2_usb_recover(&io,&s);assert(!s.wake.result && m.phy[0x6d]==0x80 && m.phy[0x6a]==0x40);
 init(&m,&s,1);y2_usb_recover(&io,&s);unsigned reads=m.reads;
 for(unsigned fail=1;fail<=reads;fail++) {
  init(&m,&s,1);m.fail=fail;y2_usb_recover(&io,&s);
  assert(s.wake.result && s.wake.after_valid!=0xfff && m.writes<=17);
 }
 init(&m,&s,1);m.drop=4;y2_usb_recover(&io,&s);assert(s.wake.result==-5 && m.writes==4);
 return 0;
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text(source)
            subprocess.run(['cc','-Wall','-Wextra','-Werror','-I'+str(ROOT/'kernel/usb'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
