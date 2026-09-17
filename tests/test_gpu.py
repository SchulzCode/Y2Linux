"""GPU domain fault containment and emitted DT contract; no hardware access."""
import os
from pathlib import Path
import unittest
from test_power import run_c
from tools.validation.dev_dtb import check

ROOT=Path(__file__).resolve().parents[1]
BUILD=Path(os.environ.get('Y2_ARTIFACT_TEST_ROOT',ROOT/'out/y2linux-gpu-01-build'))

class GPU(unittest.TestCase):
    def test_domain_handshakes_and_fault_containment(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "gpu-policy.h"
struct hardware { unsigned r[1024],writes,delays; int stuck; };
static unsigned read_reg(void *ctx,unsigned reg){return ((struct hardware *)ctx)->r[reg/4];}
static void write_reg(void *ctx,unsigned reg,unsigned value){
 struct hardware *h=ctx;h->writes++;
 assert(reg==0 || reg==0x214); /* Cannot mutate CPU/radio/display domains. */
 if(reg==0){assert(value==0x0b160001);return;}
 assert(((h->r[reg/4]^value)&~0xf1fU)==0); /* Preserve all unrelated bits. */
 h->r[reg/4]=value;
 if(h->stuck!=1) {
  if((value&0xf00)==0xf00)h->r[reg/4]|=0x1000;
  else h->r[reg/4]&=~0x1000U;
 }
 if(h->stuck!=2)h->r[0x60c/4]=(h->r[0x60c/4]&~0x10U)|((value&12)==12?0x10:0);
 if(h->stuck!=3)h->r[0x610/4]=(h->r[0x610/4]&~0x10U)|((value&12)==12?0x10:0);
}
static unsigned delays;
static void delay(unsigned us){assert(us==1);delays++;}
int main(void){
 struct hardware h={0};struct y2_spm_io io={&h,read_reg,write_reg,delay};
 h.r[0x214/4]=0x1f12;h.r[0x60c/4]=h.r[0x610/4]=0x3f4c;
 for(unsigned cycle=0;cycle<100;cycle++){
  assert(!y2_mfg_sequence(&io,1));assert(h.r[0x214/4]==13);
  assert(h.r[0x60c/4]==0x3f5c && h.r[0x610/4]==0x3f5c);
  assert(!y2_mfg_sequence(&io,0));assert(h.r[0x214/4]==0x1f12);
  assert(h.r[0x60c/4]==0x3f4c && h.r[0x610/4]==0x3f4c);
 }
 /* Stuck power-up status must not release isolation/reset or SRAM. */
 for(int stuck=2;stuck<=3;stuck++){
  h=(struct hardware){0};h.r[0x214/4]=0x1f12;h.stuck=stuck;delays=0;
  assert(y2_mfg_sequence(&io,1)==-ETIMEDOUT);
  assert(delays==10000 && h.r[0x214/4]==0x1f1e && h.writes==3);
 }
 /* Stuck SRAM on shutdown must not remove a rail or assert reset. */
 h=(struct hardware){0};assert(!y2_mfg_sequence(&io,1));h.stuck=1;delays=0;
 assert(y2_mfg_sequence(&io,0)==-ETIMEDOUT);
 assert(delays==10000 && (h.r[0x214/4]&31)==13);
 /* Power-off status timeout propagates, while unrelated status survives. */
 for(int stuck=2;stuck<=3;stuck++){
  h=(struct hardware){0};h.r[0x60c/4]=h.r[0x610/4]=0x3f4c;
  assert(!y2_mfg_sequence(&io,1));h.stuck=stuck;delays=0;
  assert(y2_mfg_sequence(&io,0)==-ETIMEDOUT);assert(delays==10000);
  assert((h.r[0x60c/4]&~16U)==0x3f4c && (h.r[0x610/4]&~16U)==0x3f4c);
 }
}
''')

    def test_standard_uapi_is_exact_upstream(self):
        self.assertEqual((ROOT/'tools/graphics/include/lima_drm.h').read_bytes(),
                         (ROOT/'.cache/sources/linux-6.18/include/uapi/drm/lima_drm.h').read_bytes())

    @unittest.skipUnless((BUILD/'y2.dtb').exists(),'emitted GPU DTB not built yet')
    def test_emitted_gpu_dt_rejects_bad_irq_and_memory(self):
        from tools.validation.dtb import cells
        data=(BUILD/'y2.dtb').read_bytes();size=(BUILD/'initramfs.cpio.gz').stat().st_size
        check(data,size)
        for old,new in ((cells(0,170,8,0,171,8),cells(0,170,4,0,171,8)),
                        (cells(0x13010000,0x10000),cells(0x13010000,0x20000)),
                        (b'arm,mali-400\0',b'arm,mali-450\0'),
                        (cells(0x81800000,0x2800000),cells(0x81800000,0x1800000))):
            self.assertIn(old,data)
            with self.subTest(old=old),self.assertRaises(ValueError):check(data.replace(old,new),size)
