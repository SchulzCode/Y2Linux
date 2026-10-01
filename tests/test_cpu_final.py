"""Run actual CPU Final MMIO algorithms with synthetic MT6582 register banks."""
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c
ROOT = Path(__file__).resolve().parents[1]


class CpuFinal(unittest.TestCase):
    def test_broadcast_and_dormant_abort_observations_are_explicit(self):
        import sys
        import tempfile
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.common import Context
        from y2_platform.observe import cpu
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory)
            self.assertIsNone(cpu(ctx)['timer']['broadcast_clockevent'])
            for name, value in (
                ('/sys/devices/system/clockevents/broadcast/current_device', 'y2-gpt4-broadcast'),
                ('/sys/module/idle/parameters/dormant_aborts', '7'),
                ('/sys/module/idle/parameters/last_error', '-62'),
            ):
                path = ctx.path(name)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(value)
            result = cpu(ctx)
            self.assertEqual(result['timer']['broadcast_clockevent'], 'y2-gpt4-broadcast')
            self.assertEqual(result['idle_diagnostics']['dormant_aborts'], '7')
            self.assertEqual(result['idle_diagnostics']['last_error'], '-62')
            self.assertIsNone(result['timer']['highres_active'])

    def test_runtime_pcm_protocol_distinct_from_suspend_and_bounded_uart(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "spm-policy.h"
#include "spm-idle-policy.h"
static unsigned r[0x1000/4],writes,delays,kicks,broken;
static unsigned rd(void *c,unsigned a){return a==SPM_PCM_IM_LEN && broken?0:r[a/4];}
static void wr(void *c,unsigned a,unsigned v){r[a/4]=v;writes++;
 if(a==SPM_PCM_CON0 && (v&CON0_PCM_KICK) && r[SPM_PCM_IM_LEN/4]==479){
  kicks++; assert(r[SPM_PCM_IM_LEN/4]==479);
  assert(r[SPM_PCM_EVENT_VECTOR3/4]==EVENT_VEC(31,1,0,68));
  assert(r[SPM_APMCU_PWRCTL/4]==(1U<<6));
  assert((r[SPM_CLK_CON/4]&(CC_DISABLE_DORM_PWR|CC_DISABLE_INFRA_PWR))==CC_DISABLE_INFRA_PWR);
  assert(!(r[SPM_PCM_CON1/4]&CON1_PCM_WDT_EN));
  assert(!(r[SPM_SLEEP_WAKEUP_EVENT_MASK/4]&WAKE_SRC_GPT));
 }}
static void delay(unsigned u){delays+=u;}
int main(void){struct y2_spm_io io={0,rd,wr,delay};
 r[SPM_PWR_STATUS/4]=0x800;assert(y2_spm_idle_arm(&io,0x81000000)==-EBUSY && !writes);
 r[SPM_PWR_STATUS/4]=0;broken=1;assert(y2_spm_idle_arm(&io,0x81000000)==-EIO && !kicks);
 broken=0;assert(y2_spm_idle_arm(&io,0x81000000)==-EBUSY && !kicks && delays==100);
 assert(!(r[SPM_POWER_ON_VAL1/4]&R7_UART_CLK_OFF_REQ));
 r[SPM_PCM_REG13_DATA/4]=R13_UART_CLK_OFF_ACK;
 assert(!y2_spm_idle_arm(&io,0x81000000) && kicks==1);
 y2_spm_suspend_clean(&io);
 assert(!(r[SPM_PCM_CON1/4]&(CON1_PCM_TIMER_EN|CON1_PCM_WDT_EN)));
 assert(!r[SPM_PCM_PWR_IO_EN/4]);
 y2_spm_normal(&io,0x81002000);assert(r[SPM_PCM_IM_LEN/4]==27);
}
''')

    def test_cirq_clone_mask_and_replay_actual_code(self):
        s = (ROOT/'kernel/platform/cirq.c').read_text().replace('int y2_cirq_begin(', 'static int y2_cirq_begin(').replace('void y2_cirq_end(', 'static void y2_cirq_end(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include "cirq-policy.h"
#define BIT(x) (1U<<(x))
#define WARN_ON_ONCE(x) (x)
#define dsb(x) do{}while(0)
static unsigned c[0x400/4],g[0x1000/4],p[7],online=1,cpu;
static void *cirq=c,*dist=g,*pol=p;static unsigned masks[7],entries,flushes,last_pending;
static bool active;static unsigned saved_control;
static bool y2_cirq_ready(void){return true;}
static bool irqs_disabled(void){return true;}
static unsigned num_online_cpus(void){return online;}
static unsigned smp_processor_id(void){return cpu;}
static unsigned readl(void *addr){return *(unsigned *)addr;}
static void writel(unsigned v,void *addr){
 unsigned off;
 if(addr>=(void *)g && addr<(void *)(g+sizeof(g)/4)){
  off=(unsigned *)addr-g;
  if(off>=0x180/4 && off<0x180/4+7)g[off-0x80/4]&=~v;
  else if((off>=0x100/4 && off<0x100/4+7)||(off>=0x200/4 && off<0x200/4+7))g[off]|=v;
  else g[off]=v;
 }else if(addr>=(void *)c && addr<(void *)(c+sizeof(c)/4)){
  off=(unsigned *)addr-c;c[off]=v;
 }else assert(0);
}
''' + function(s, 'y2_cirq_begin') + function(s, 'y2_cirq_end') + r'''
int main(void){
 for(unsigned i=1;i<7;i++)g[0x100/4+i]=0xa5a5a5a5;
 online=4;assert(y2_cirq_begin()==-EBUSY && !entries);online=1;
 g[0x200/4+2]=1;assert(y2_cirq_begin()==-EAGAIN && !entries);g[0x200/4+2]=0;
 assert(!y2_cirq_begin() && active && c[0x300/4]==3);
 assert(g[0x100/4+4]==BIT(21));
 assert(c[0x100/4]==0xa5a5a5a5);
 assert(c[0x100/4+4]==(0xa5a5a5a5&0x07ffffff));
 c[0]=0x12345678;c[4]=~0U;
 y2_cirq_end();assert(!active && flushes==1 && c[0x300/4]==2);
 assert(g[0x200/4+2]==0x12345678 && g[0x200/4+6]==0x07ffffff);
 for(unsigned i=1;i<7;i++)assert(g[0x100/4+i]==0xa5a5a5a5);
 y2_cirq_end();assert(flushes==1);
 for(unsigned b=0;b<5;b++)assert(y2_cirq_valid(b)==(b==4?0x07ffffff:~0U));
 assert(y2_cirq_sensitivity(0xaaaaaaaa,0xaaaaaaaa)==0);
 assert(y2_cirq_sensitivity(0,0)==~0U);
 assert(y2_cirq_ack_mask(3,1,~0U)==~1U);
}
''')

    def test_expiry_worker_never_expires_a_renewed_lease(self):
        s = (ROOT/'kernel/platform/workload.c').read_text()
        run_c(r'''
#include <assert.h>
struct work_struct{int unused;};struct delayed_work{struct work_struct work;unsigned delay;};
struct y2_workload{struct delayed_work expiry;unsigned long deadline;int lock;};
#define container_of(p,t,m) ((t *)(p))
#define to_delayed_work(p) ((struct delayed_work *)(p))
#define time_before(a,b) ((long)((a)-(b))<0)
static unsigned long jiffies;static unsigned expired;static int system_wq;
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){*p=0;}
static void mod_delayed_work(int q,struct delayed_work *w,unsigned n){w->delay=n;}
static void y2_workload_idle(struct y2_workload *h){h->deadline=0;expired++;}
''' + function(s,'y2_workload_expire') + r'''
int main(void){struct y2_workload h={0};h.deadline=1250;jiffies=1000;
 y2_workload_expire(&h.expiry.work);assert(!expired && h.expiry.delay==250);
 jiffies=1250;y2_workload_expire(&h.expiry.work);assert(expired==1 && !h.deadline);
 h.deadline=20;jiffies=~0UL-10;y2_workload_expire(&h.expiry.work);assert(expired==1);
}
''')

    def test_usb_stale_status_and_live_dma_refusal(self):
        s = (ROOT/'kernel/platform/usb.c').read_text()
        s = s.replace('int y2_musb_system_quiesce(', 'static int y2_musb_system_quiesce(')
        s = s.replace('void y2_musb_system_saved(', 'static void y2_musb_system_saved(')
        s = s.replace('void y2_musb_before_restore(', 'static void y2_musb_before_restore(')
        s = s.replace('void y2_musb_after_restore(', 'static void y2_musb_after_restore(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
typedef uint8_t u8;typedef uint16_t u16;
#define __iomem
#define BIT(x) (1U<<(x))
#define CONFIG_USB_INVENTRA_DMA 1
#define IS_ENABLED(x) (x)
#define READ_ONCE(x) (x)
#define dsb(x) do{}while(0)
#define MUSB_INTRUSB 0x0a
#define MUSB_INTRUSBE 0x0b
#define MUSB_INTRTX 0x02
#define MUSB_INTRRX 0x04
#define MUSB_INTRTXE 0x06
#define MUSB_INTRRXE 0x08
#define MUSB_POWER 1
#define MUSB_POWER_SOFTCONN 0x40
struct musb{void *mregs;void *dma_controller;int lock;struct{unsigned power;}context;};
static unsigned char regs[0x300];static struct musb *y2_musb;
static bool y2_pm_disconnect,y2_pm_system_saved;static unsigned y2_pm_suspends,y2_pm_restores,y2_pm_stale,jiffies,y2_pm_l1_mask;
static struct {unsigned long tick;unsigned burst,jiffy;} y2_irq_guard;
static struct {int result;} y2_live;
static void lock(int *l){assert(!*l);*l=1;}
#define spin_lock_irqsave(p,f) do{(f)=0;lock(p);}while(0)
#define spin_unlock_irqrestore(p,f) do{*(p)=0;(void)(f);}while(0)
static u8 readb(void *p){return *(u8 *)p;}
static u16 readw(void *p){return *(u16 *)p;}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writeb(u8 v,void *p){unsigned off=(u8 *)p-regs;
 if(off==MUSB_INTRUSB || off==0x200)*(u8 *)p&=~v;else *(u8 *)p=v;}
static void writew(u16 v,void *p){unsigned off=(u8 *)p-regs;
 if(off==MUSB_INTRTX || off==MUSB_INTRRX)*(u16 *)p&=~v;else *(u16 *)p=v;}
static void writel(unsigned v,void *p){*(unsigned *)p=v;}
static void musb_g_disconnect(struct musb *m){assert(m->lock);}
''' + function(s,'y2_musb_clear_stale') + function(s,'y2_musb_system_quiesce') + function(s,'y2_musb_system_saved') + function(s,'y2_musb_before_restore') + function(s,'y2_musb_after_restore') + r'''
int main(void){struct musb m={regs,(void *)1,0,{0}};y2_musb=&m;
 regs[MUSB_POWER]=MUSB_POWER_SOFTCONN;regs[MUSB_INTRUSB]=7;regs[0x200]=0xff;
 *(u16 *)(regs+MUSB_INTRTX)=0xff;*(u16 *)(regs+MUSB_INTRRX)=0xaa;
 y2_irq_guard.burst=500;y2_irq_guard.jiffy=900;assert(!y2_musb_system_quiesce(&m));
 assert(!regs[MUSB_INTRUSB] && !regs[0x200] && !readw(regs+MUSB_INTRTX));
 assert(!(regs[MUSB_POWER]&0x40) && !y2_irq_guard.burst && !y2_irq_guard.jiffy && y2_pm_suspends==1);
 y2_musb_system_saved(&m);assert(m.context.power&0x40);
 regs[MUSB_INTRUSB]=1;*(unsigned *)(regs+0xa4)=15;
 y2_musb_before_restore(&m);assert(!*(unsigned *)(regs+0xa4) && !regs[MUSB_INTRUSB]);
 assert(y2_pm_restores==1);
 y2_musb_after_restore(&m);assert(*(unsigned *)(regs+0xa4)==15);
 regs[MUSB_INTRUSB]=2;regs[0x200]=4;
 y2_musb_before_restore(&m);y2_musb_after_restore(&m);
 assert(regs[MUSB_INTRUSB]==2 && regs[0x200]==4);
 y2_musb_before_restore(&m);y2_live.result=-EIO;
 y2_musb_after_restore(&m);assert(!*(unsigned *)(regs+0xa4));
 regs[MUSB_POWER]=0x40;*(u16 *)(regs+0x204)=1;
 assert(y2_musb_system_quiesce(&m)==-EBUSY && (regs[MUSB_POWER]&0x40));
 assert(regs[MUSB_INTRUSB]==2 && regs[0x200]==4);
 assert(y2_pm_suspends==1 && !m.lock);
}
''')

    def test_radio_retry_only_after_complete_isolation(self):
        s = (ROOT/'kernel/platform/connectivity/core.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#define dev_warn(...) do{}while(0)
struct y2_conn{bool powered,dma_active,clock_on,rail_on[4];};
static unsigned boot_retries,attempts,delays;static int result=-ETIMEDOUT;
static int power_on_once(struct y2_conn *c){attempts++;return attempts==1?result:0;}
static void msleep(unsigned ms){assert(ms==20);delays++;}
''' + function(s,'power_on') + r'''
int main(void){struct y2_conn c={0};assert(!power_on(&c) && attempts==2 && boot_retries==1 && delays==1);
 attempts=0;c.dma_active=true;assert(power_on(&c)==-ETIMEDOUT && attempts==1);
 c.dma_active=false;attempts=0;c.rail_on[2]=true;assert(power_on(&c)==-ETIMEDOUT && attempts==1);
 c.rail_on[2]=false;attempts=0;result=-EINVAL;assert(power_on(&c)==-EINVAL && attempts==1);
 assert(boot_retries==1);
}
''')
