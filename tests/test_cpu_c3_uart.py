"""Actual MT6582 UART/SPM owners with fault injection; no hardware PASS."""
from pathlib import Path
import json
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class UartSleep(unittest.TestCase):
    def test_actual_clock_transaction_defers_only_prepared_uart1_and_never_gates_it(self):
        source = (ROOT/'kernel/platform/clocks.c').read_text()
        for name in ('y2_ccf_deep_idle_begin', 'y2_ccf_deep_idle_end', 'y2_ccf_deep_idle_blockers'):
            source = source.replace('int '+name+'(', 'static int '+name+'(')
        body = function(source,'uart_idle_read') + function(source,'uart_idle_write')
        body += 'static struct y2_spm_io uart_idle_io={.read=uart_idle_read,.write=uart_idle_write};\n'
        for name in ('y2_uart_sleep_available','y2_uart_sleep_restore','y2_ccf_deep_idle_end',
                     'y2_ccf_deep_idle_begin','y2_ccf_deep_idle_blockers'):
            body += function(source,name)
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#include "uart-idle-policy.h"
#include "idle-clock-policy.h"
#define __iomem
#define BIT(x) (1U<<(x))
#define dsb(x) do{}while(0)
static unsigned top[64],peri[64],infra[64],uart0[0x80/4],uart1[0x80/4];
static void *y2_clock_bases[]={top,peri,infra},*dormant_uart[]={uart0,uart1};
static unsigned deep_peri_raw,deep_uart_deferred,deep_peri_blockers,deep_infra_blockers;
static unsigned deep_disp0_blockers,deep_disp1_blockers,idle_bus,idle_audio;
static unsigned uart1_gate_before,uart1_gate_after,uart_restore_failures,uart0_sleep,uart0_dma;
static struct y2_uart_idle_save dormant_uart_save;
static struct y2_uart_idle_save uart1_observed;
static const char *uart_sleep_phase;
static const char *uart_owner_reason;
static unsigned writes,drop_enable,drop_restore;
static int y2_clk_lock;
static unsigned readl(void *p){
 assert(p!=(void *)(uart0+0x14/4) && p!=(void *)(uart1+0x14/4)); /* no destructive LSR reads */
 return *(unsigned *)p;
}
static void writel(unsigned v,void *p){
 assert(p==(void *)(top+1)||p==(void *)(top+0x70/4)||p==(void *)(uart1+0x48/4));
 writes++;if(p==(void *)(uart1+0x48/4) && (v?drop_enable:drop_restore))return;
 *(unsigned *)p=v;
}
static int spin_trylock(int *l){if(*l)return 0;*l=1;return 1;}
static void spin_unlock(int *l){assert(*l);*l=0;}
#define spin_lock_irqsave(l,f) do{f=0;assert(spin_trylock(l));}while(0)
#define spin_unlock_irqrestore(l,f) do{(void)f;spin_unlock(l);}while(0)
static int y2_mm_idle_blockers(unsigned *a,unsigned *b){*a=*b=0;return 0;}
static int y2_ccf_deep_idle_end(void);
''' + body + r'''
int main(void){
 unsigned p,i,b;peri[0x18/4]=~(BIT(16)|BIT(17));infra[0x40/4]=~0U;
 uart0[0x48/4]=1;top[0x70/4]=0x07123456;
 assert(!y2_ccf_deep_idle_blockers(&p,&i,&b) && !p && !writes && !uart1[0x48/4]);
 assert(!y2_ccf_deep_idle_begin() && y2_clk_lock && uart1[0x48/4]==1);
 assert(deep_peri_raw==BIT(17) && deep_uart_deferred==BIT(17) && !deep_peri_blockers);
 assert(!y2_ccf_deep_idle_end() && !y2_clk_lock && !uart1[0x48/4]);
 assert(!uart1_gate_before && !uart1_gate_after && top[1]==0 && top[0x70/4]==0x07123456);
 for(unsigned bit=0;bit<32;bit++)if(bit!=17 && (Y2_DPIDLE_PERI_MASK&BIT(bit))){
  peri[0x18/4]=~(BIT(16)|BIT(17)|BIT(bit));unsigned old=writes;
  assert(y2_ccf_deep_idle_begin()==-EBUSY && writes==old && !y2_clk_lock);
  assert(deep_peri_blockers==BIT(bit)); /* every unrelated blocker preserved */
 }
 peri[0x18/4]=~(BIT(16)|BIT(17));uart1[4/4]=1;
 unsigned old=writes;assert(y2_ccf_deep_idle_begin()==-EBUSY && writes==old);uart1[4/4]=0;
 uart0[0x48/4]=0;assert(y2_ccf_deep_idle_begin()==-EBUSY && writes==old);uart0[0x48/4]=1;
 uart0[0x4c/4]=1;assert(y2_ccf_deep_idle_begin()==-EBUSY && writes==old);uart0[0x4c/4]=0;
 drop_enable=1;assert(y2_ccf_deep_idle_begin()==-EIO && !uart1[0x48/4] && !y2_clk_lock && top[1]==0);drop_enable=0;
 assert(!y2_ccf_deep_idle_begin());drop_restore=1;
 assert(y2_ccf_deep_idle_end()==-EIO && uart_restore_failures==1 && !y2_clk_lock);drop_restore=0;uart1[0x48/4]=0;
 assert(!y2_ccf_deep_idle_begin());peri[0x18/4]|=BIT(17);
 assert(y2_ccf_deep_idle_end()==-EIO && uart_restore_failures==2); /* unexpected hardware gate change detected */
 old=writes;assert(!y2_ccf_deep_idle_begin() && !dormant_uart_save.prepared);
 assert(!y2_ccf_deep_idle_end() && uart1_gate_before && uart1_gate_after && writes==old+4);
 /* Even a malicious deferred mask cannot erase MMC/APDMA/I2C/SPI bits. */
 assert(y2_dpidle_peri_blockers(0,~0U)==(Y2_DPIDLE_PERI_MASK&~BIT(17)));
}
''')

    def test_actual_pio_adoption_preserves_banked_irq_dma_and_unknown_owners(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "uart-idle-policy.h"
static unsigned r[0x80/4],writes,reads,drop;
static unsigned rd(void *c,unsigned a){reads++;return r[a/4];}
static void wr(void *c,unsigned a,unsigned v){assert(a==Y2_UART_SLEEP_EN);writes++;if(!drop)r[a/4]=v;}
int main(void){
 struct y2_spm_io io={0,rd,wr,0};struct y2_uart_idle_save s={0};
 r[0xc/4]=0xbf;assert(y2_uart_idle_prepare(&io,&s)==-EBUSY && reads==1 && !writes);
 r[0xc/4]=0x100;assert(y2_uart_idle_prepare(&io,&s)==-EBUSY && reads==2 && !writes);
 r[0xc/4]=3;r[4/4]=5;assert(y2_uart_idle_prepare(&io,&s)==-EBUSY && !writes);
 r[4/4]=0;
 for(unsigned b=0;b<32;b++)if(b!=2){
  r[0x4c/4]=1U<<b;assert(y2_uart_idle_prepare(&io,&s)==-EBUSY && !writes);
 }
 r[0x4c/4]=4;r[0x48/4]=2;
 assert(y2_uart_idle_prepare(&io,&s)==-EBUSY && !writes);
 r[0x48/4]=0;assert(!y2_uart_idle_prepare(&io,&s) && r[0x48/4]==1 && s.prepared);
 assert(!y2_uart_idle_restore(&io,&s) && !r[0x48/4] && !s.prepared);
 assert(r[0xc/4]==3 && !r[4/4] && r[0x4c/4]==4 && writes==2);
 r[0x48/4]=1;assert(!y2_uart_idle_prepare(&io,&s));
 assert(!y2_uart_idle_restore(&io,&s) && writes==2); /* existing sleep owner unchanged */
}
''')

    def test_sleep_write_restore_and_configuration_corruption_fail_closed(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "uart-idle-policy.h"
static unsigned r[0x80/4],drop;
static unsigned rd(void *c,unsigned a){return r[a/4];}
static void wr(void *c,unsigned a,unsigned v){assert(a==0x48);if(!drop)r[a/4]=v;}
int main(void){
 struct y2_spm_io io={0,rd,wr,0};struct y2_uart_idle_save s={0};
 drop=1;assert(y2_uart_idle_prepare(&io,&s)==-EIO && s.prepared);
 assert(!y2_uart_idle_restore(&io,&s));drop=0;
 assert(!y2_uart_idle_prepare(&io,&s));drop=1;
 assert(y2_uart_idle_restore(&io,&s)==-EIO);drop=0;r[0x48/4]=0;
 assert(!y2_uart_idle_prepare(&io,&s));r[4/4]=1;
 assert(y2_uart_idle_restore(&io,&s)==-EIO && r[4/4]==1); /* never overwrite a live IRQ owner */
}
''')

    def test_spm_actual_handshake_ack_boundaries_timeout_and_exact_request_unwind(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "spm-uart-policy.h"
static unsigned r[0x1000/4],us,ack_at,request_writes,drop_request,drop_restore;
static unsigned rd(void *c,unsigned a){
 if(a==SPM_PCM_REG13_DATA && (r[SPM_POWER_ON_VAL1/4]&1) && us>=ack_at)
  return 0x22|R13_UART_CLK_OFF_ACK;
 return r[a/4];
}
static void wr(void *c,unsigned a,unsigned v){
 assert(a==SPM_POWER_ON_VAL1);request_writes++;
 if((v&1)?drop_request:drop_restore)return;r[a/4]=v;
}
static void delay(unsigned n){assert(n==10);us+=n;}
int main(void){
 struct y2_spm_io io={0,rd,wr,delay};struct y2_spm_uart_sleep s={0};
 for(unsigned boundary=0;boundary<=100;boundary+=10){
  r[SPM_POWER_ON_VAL1/4]=0x15820;us=0;ack_at=boundary;
  assert(!y2_spm_uart_request(&io,&s) && s.request && s.ack && us==boundary);
  assert(s.power_before==0x15820 && s.power_request==0x15821);
  assert(s.r13_ack==(0x22|R13_UART_CLK_OFF_ACK));
 }
 assert(s.attempts==11 && s.successes==11 && !s.timeouts);
 r[SPM_POWER_ON_VAL1/4]=0x15820;ack_at=101;us=0;
 assert(y2_spm_uart_request(&io,&s)==-EBUSY && us==100 && s.timeouts==1 && !s.ack);
 assert(r[SPM_POWER_ON_VAL1/4]==0x15820 && s.power_after==0x15820);
 unsigned previous=request_writes;r[SPM_POWER_ON_VAL1/4]=0x15821;
 assert(y2_spm_uart_request(&io,&s)==-EBUSY && request_writes==previous); /* existing request belongs to another owner */
 r[SPM_POWER_ON_VAL1/4]=0x15820;drop_request=1;us=0;
 assert(y2_spm_uart_request(&io,&s)==-EIO && !us && !s.request && !s.ack);
 drop_request=0;drop_restore=1;us=0;
 assert(y2_spm_uart_request(&io,&s)==-EIO && s.restore_failures==1 && s.timeouts==2);
 assert(s.power_after==0x15821 && !s.ack); /* missing clear cannot be hidden */
}
''')

    def test_pcm_never_runs_without_real_ack_and_request_is_after_fetch(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "spm-idle-policy.h"
static unsigned r[0x1000/4],ack,fetch,runs,us;
static unsigned rd(void *c,unsigned a){return a==SPM_PCM_REG13_DATA && ack && (r[SPM_POWER_ON_VAL1/4]&1)?R13_UART_CLK_OFF_ACK:r[a/4];}
static void wr(void *c,unsigned a,unsigned v){
 if(a==SPM_POWER_ON_VAL1 && (v&1))assert(fetch && !runs);
 if(a==SPM_PCM_CON0 && (v&CON0_IM_KICK))fetch++;
 if(a==SPM_PCM_CON0 && (v&CON0_PCM_KICK)){
  if(r[SPM_PCM_IM_LEN/4]==479){assert(ack);runs++;}
  else assert(r[SPM_PCM_IM_LEN/4]==27 && !(r[SPM_POWER_ON_VAL1/4]&1));
 }
 r[a/4]=v;
}
static void delay(unsigned n){us+=n;}
int main(void){
 struct y2_spm_io io={0,rd,wr,delay};struct y2_spm_uart_sleep s={0};
 r[SPM_POWER_ON_VAL1/4]=0x15820;
 assert(y2_spm_idle_arm(&io,0x81000000,&s)==-EBUSY && !runs && us==100);
 assert(s.timeouts==1 && !s.successes && r[SPM_POWER_ON_VAL1/4]==0x15820);
 ack=1;assert(!y2_spm_idle_arm(&io,0x81000000,&s) && runs==1 && s.successes==1);
 assert(!y2_spm_idle_restore(&io,0x81002000));
 assert(!(r[SPM_POWER_ON_VAL1/4]&1) && r[SPM_PCM_IM_LEN/4]==27);
}
''')

    def test_real_console_driver_only_enables_exact_mt6582_pio_sleep(self):
        from tools.build.run import prepare_overlay
        manifest = json.loads((ROOT/'kernel/patches/manifest.json').read_text())
        spec = next(x for x in manifest['overlays'] if x['patch']=='0061-mt6582-uart-pio-sleep.patch')
        source = prepare_overlay(ROOT,spec).read_text()
        run_c(r'''
#include <assert.h>
#include <string.h>
#define MTK_UART_SLEEP_EN 0x12
struct device{const char *of_node;};
struct uart_port{struct device *dev;void *membase;unsigned regshift;};
struct uart_8250_port{struct uart_port port;void *dma;};
static unsigned registers[0x80/4],writes,warnings,drop;
static int of_device_is_compatible(const char *node,const char *compatible){return !strcmp(node,compatible);}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){assert(p==(void *)(registers+0x48/4));writes++;if(!drop)*(unsigned *)p=v;}
#define dev_warn(...) (++warnings)
''' + function(source,'mtk8250_enable_pio_sleep') + r'''
int main(void){
 struct device d={"mediatek,mt6577-uart"};struct uart_8250_port u={{&d,registers,2},0};
 mtk8250_enable_pio_sleep(&u);assert(!writes);
 d.of_node="mediatek,mt6582-uart";u.dma=&u;mtk8250_enable_pio_sleep(&u);assert(!writes);
 u.dma=0;mtk8250_enable_pio_sleep(&u);assert(writes==1 && registers[0x48/4]==1 && !warnings);
 registers[0x48/4]=0;drop=1;mtk8250_enable_pio_sleep(&u);assert(writes==2 && warnings==1);
}
''')
