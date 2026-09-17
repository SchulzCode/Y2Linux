"""Bounded loader, BC1.1, and physical CPU-domain policy fault tests."""
import unittest
from test_power import run_c
from test_charger import FIXTURE

class PowerPolicy(unittest.TestCase):
    def test_loader_tags_and_malformed_metadata(self):
        run_c(r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include "boot-policy.h"
static unsigned char b[256];
static void word(unsigned at,unsigned value){for(unsigned n=0;n<4;n++)b[at+n]=value>>(n*8);}
static void reset(void){
 memset(b,0,sizeof b);word(0,2);word(4,0x54410001);
 word(8,3);word(12,0x41000802);word(16,8);
 word(20,10);word(24,0x54410009);strcpy((char *)b+28,"console=tty0 boot_reason=1");
}
int main(void){
 struct y2_boot_info i;
 reset();i=y2_boot_parse(b,sizeof b);assert(i.valid && i.mode==8 && i.reason==1 && y2_boot_offline(&i));
 for(unsigned mode=0;mode<10;mode++){
  reset();word(16,mode);i=y2_boot_parse(b,sizeof b);
  assert(i.valid && y2_boot_offline(&i)==(mode==8 || mode==9));
 }
 for(unsigned size=0;size<68;size++){
  reset();i=y2_boot_parse(b,size);assert(!i.valid);
 }
 unsigned indices[]={0,8,20};
 for(unsigned n=0;n<3;n++)for(unsigned value=0;value<2;value++){
  reset();word(indices[n],value);assert(!y2_boot_parse(b,sizeof b).valid);
 }
 reset();word(8,0xffffffff);assert(!y2_boot_parse(b,sizeof b).valid);
 reset();word(16,10);assert(!y2_boot_parse(b,sizeof b).valid);
 reset();memset(b+28,'x',32);assert(!y2_boot_parse(b,sizeof b).valid);
 reset();strcpy((char *)b+28,"boot_reason=10");assert(!y2_boot_parse(b,sizeof b).valid);
 reset();strcpy((char *)b+28,"boot_reason=1 boot_reason=0");assert(!y2_boot_parse(b,sizeof b).valid);
 reset();strcpy((char *)b+28,"xboot_reason=1");assert(!y2_boot_parse(b,sizeof b).valid);
 reset();word(60,3);word(64,0x41000802);word(68,0);assert(!y2_boot_parse(b,sizeof b).valid);
 /* Arbitrary loader memory tags are bounded but never imported into policy. */
 reset();word(60,4);word(64,0x54410002);word(68,0xffffffff);word(72,0);
 i=y2_boot_parse(b,sizeof b);assert(i.valid && i.mode==8);
}
''')

    def test_bc11_all_classifications_io_failures_and_removal(self):
        run_c(FIXTURE + r'''
static unsigned steps, waits, remove_wait, pattern[3];
static int settle(void *ctx,unsigned ms){
 (void)ctx;
 assert(!(regs[0]&0x18));
 if(++waits==remove_wait)return -ENODEV;
 if(ms==80){assert(steps<3);regs[0x24/2]=(regs[0x24/2]&~0x80)|(pattern[steps++]?0x80:0);}
 else assert(ms==300 || ms==50);
 return 0;
}
int main(void){
 unsigned outcomes[]={Y2_SOURCE_SDP,Y2_SOURCE_CDP,Y2_SOURCE_DCP,Y2_SOURCE_NONSTANDARD,Y2_SOURCE_APPLE};
 unsigned bits[5][3]={{0,0,0},{0,1,0},{0,1,1},{1,0,0},{1,1,0}};
 struct y2_bc11_io bc={.pmic=io,.wait_source=settle};
 enum y2_source_type type;
 for(unsigned p=0;p<5;p++){
  reset();steps=waits=0;memcpy(pattern,bits[p],sizeof pattern);
  assert(!y2_bc11_detect(&bc,&type) && type==outcomes[p]);
  assert(!(regs[0x26/2]&0x1ff) && !(regs[0x24/2]&0xc));
  unsigned count=operations;
  for(unsigned f=1;f<=count;f++){
   reset();steps=waits=0;fail_at=f;memcpy(pattern,bits[p],sizeof pattern);
   assert(y2_bc11_detect(&bc,&type)<0 && type==Y2_SOURCE_UNKNOWN && !starts);
   assert(regs[0xc/2]==1 && regs[0xe/2]==5 && regs[0x20/2]==5);
  }
  for(unsigned f=1;f<=(p==1 || p==2?5:4);f++){
   reset();steps=waits=0;remove_wait=f;memcpy(pattern,bits[p],sizeof pattern);
   assert(y2_bc11_detect(&bc,&type)==-ENODEV && type==Y2_SOURCE_UNKNOWN);
   assert(!(regs[0x26/2]&0x1ff) && !(regs[0x24/2]&0xc) && !starts);
  }
  remove_wait=0;
 }
 for(unsigned source=0;source<=Y2_SOURCE_APPLE;source++)for(int budget=-1;budget<=500001;budget+=10000){
  unsigned ua=y2_source_charge_ua(source,budget);
  assert(ua<=650000 && (ua==0 || y2_charge_selector(ua)>=0));
  if(source==Y2_SOURCE_UNKNOWN || source==Y2_SOURCE_NONSTANDARD)assert(ua==70000);
  if(source==Y2_SOURCE_SDP && budget>=0 && budget<100000)assert(!ua);
 }
 for(unsigned selector=0;selector<16;selector++){
  reset();int ret=y2_charge_prepare(&io,selector);
  if(selector==15 || selector==12 || selector==10){
   assert(!ret && !y2_charge_start(&io,selector));
   assert((regs[8/2]&15)==selector);
  }else assert(ret==-EINVAL && !starts);
 }
}
''')

    def test_cpu_domain_sequence_and_bounded_failed_acks(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "spm-policy.h"
static unsigned regs[1024], delays, writes, cpu, stuck;
/* Independent stock FC1/FC2/FC3 masks, also observed when CPU3 cleared bit 9. */
static const unsigned cpu_mask[] = {0, 0x800, 0x400, 0x200};
static unsigned rd(void *ctx,unsigned reg){(void)ctx;assert(reg<sizeof regs);return regs[reg/4];}
static void wr(void *ctx,unsigned reg,unsigned value){
 (void)ctx;writes++;unsigned power=0x214+cpu*4,sram=0x25c+cpu*8,bit=cpu_mask[cpu];
 assert(reg==0 || reg==power || reg==sram);
 if(reg==0)assert(value==0xb160001);
 if(reg==power && (value&1) && !(regs[reg/4]&1)){
  assert((regs[0x60c/4]&bit) && (regs[0x610/4]&bit));
  assert(!(regs[sram/4]&0x101)); /* release reset after verified SRAM */
 }
 regs[reg/4]=value;
 if(reg==sram && stuck!=1)regs[reg/4]=(value&~0x100)|((value&1)?0x100:0);
 if(reg==power && stuck!=2 && ((value&12)==0 || (value&12)==12)){
  for(unsigned at=0x60c;at<=0x610;at+=4)regs[at/4]=(regs[at/4]&~bit)|((value&12)?bit:0);
 }
}
static void wait(unsigned us){assert(us==1);delays++;assert(delays<=20001);}
static struct y2_spm_io io={0,rd,wr,wait};
static void reset(void){memset(regs,0,sizeof regs);delays=writes=stuck=0;
 regs[(0x214+cpu*4)/4]=0x4d;regs[0x60c/4]=regs[0x610/4]=cpu_mask[cpu];
 regs[0x720/4]=1U<<(15+cpu);
}
int main(void){
 assert(y2_spm_cpu_power(&io,0,0)==-EINVAL && !writes);
 for(cpu=1;cpu<=3;cpu++){
  reset();assert(!y2_spm_cpu_power(&io,cpu,0));
  assert(!(regs[0x60c/4]&(cpu_mask[cpu])) && !delays);
  assert(!y2_spm_cpu_power(&io,cpu,1));assert(regs[(0x214+cpu*4)/4]==0x4d);
  reset();regs[0x720/4]=0;assert(y2_spm_cpu_power(&io,cpu,0)==-ETIMEDOUT);
  assert(writes==1 && delays==10000 && regs[(0x214+cpu*4)/4]==0x4d);
  reset();stuck=1;assert(y2_spm_cpu_power(&io,cpu,0)==-ETIMEDOUT);
  assert(delays==10000 && (regs[(0x214+cpu*4)/4]&12)==12);
  reset();assert(!y2_spm_cpu_power(&io,cpu,0));stuck=2;delays=0;
  assert(y2_spm_cpu_power(&io,cpu,1)==-ETIMEDOUT);
  assert(!(regs[(0x214+cpu*4)/4]&1) && delays==10001);
 }
}
''')

    def test_spm_suspend_prerequisites_and_cleanup(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "spm-policy.h"
#include "spm-suspend-policy.h"
static unsigned regs[1024], writes, delay_us, fetched, armed, bad_read;
static unsigned rd(void *ctx,unsigned reg){
 (void)ctx;assert(reg<sizeof regs && !(reg&3));
 if(reg==SPM_PCM_IM_LEN && bad_read)return 0;
 return regs[reg/4];
}
static void wr(void *ctx,unsigned reg,unsigned value){
 (void)ctx;assert(reg<sizeof regs && !(reg&3));writes++;
 if(reg==SPM_PCM_CON0){
  assert((value>>16)==SPM_PROJECT_CODE);
  if(value&CON0_IM_KICK)fetched++;
  if(value&CON0_PCM_KICK){
   armed++;
   if(regs[SPM_PCM_IM_LEN/4]==596){
    assert(fetched && (regs[SPM_PCM_REG13_DATA/4]&R13_UART_CLK_OFF_ACK));
    assert(regs[SPM_SLEEP_WAKEUP_EVENT_MASK/4]==~Y2_SPM_WAKE);
    assert((regs[SPM_CLK_CON/4]&(CC_DISABLE_DORM_PWR|CC_DISABLE_INFRA_PWR))==CC_DISABLE_INFRA_PWR);
    assert(regs[SPM_PCM_EVENT_VECTOR3/4]==EVENT_VEC(31,1,0,99));
    assert(regs[SPM_PCM_WDT_TIMER_VAL/4]>regs[SPM_PCM_TIMER_VAL/4]);
   }
  }
 }
 if(reg==SPM_PCM_CON1)assert((value>>16)==SPM_PROJECT_CODE);
 regs[reg/4]=value;
}
static void wait(unsigned us){delay_us+=us;assert(delay_us<=100);}
static struct y2_spm_io io={0,rd,wr,wait};
static void reset(void){memset(regs,0,sizeof regs);writes=delay_us=fetched=armed=bad_read=0;
 regs[SPM_POWER_ON_VAL1/4]=0x15820;regs[SPM_PCM_REG13_DATA/4]=R13_UART_CLK_OFF_ACK;
}
int main(void){
 for(unsigned copy=SPM_PWR_STATUS;copy<=SPM_PWR_STATUS_S;copy+=4)for(unsigned bit=9;bit<=11;bit++){
  reset();regs[copy/4]=1U<<bit;
  assert(y2_spm_suspend_arm(&io,0x81000000)==-EBUSY && !writes && !armed);
 }
 reset();regs[SPM_PCM_REG13_DATA/4]=0;
 assert(y2_spm_suspend_arm(&io,0x81000000)==-EBUSY && !armed && delay_us==100);
 assert(!(regs[SPM_POWER_ON_VAL1/4]&R7_UART_CLK_OFF_REQ));
 reset();bad_read=1;assert(y2_spm_suspend_arm(&io,0x81000000)==-EIO && !armed);
 /* CPU0/debug/MCU status bits remain powered; they are not secondary CPUs. */
 reset();regs[SPM_PWR_STATUS/4]=regs[SPM_PWR_STATUS_S/4]=0x314c;
 assert(!y2_spm_suspend_arm(&io,0x81000000) && armed==1);
 y2_spm_suspend_clean(&io);
 assert(!(regs[SPM_PCM_CON1/4]&(CON1_PCM_WDT_EN|CON1_PCM_TIMER_EN)));
 assert(!regs[SPM_PCM_PWR_IO_EN/4]);
 assert(!(regs[SPM_CLK_CON/4]&(CC_SRCLKENA_MASK|CC_LOCK_INFRA_DCM)));
 assert(regs[SPM_SLEEP_ISR_MASK/4]==ISRM_ALL);
 y2_spm_normal(&io,0x81001000);
 assert(regs[SPM_PCM_IM_LEN/4]==27 && regs[SPM_PCM_IM_PTR/4]==0x81001000);
 assert(regs[SPM_POWER_ON_VAL1/4]==0x15820 && !regs[SPM_PCM_PWR_IO_EN/4]);
}
''')
