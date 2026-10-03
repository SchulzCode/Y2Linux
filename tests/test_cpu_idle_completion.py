"""Exercise actual idle owners with fault injection. No synthetic hardware PASS."""
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class Deadline(unittest.TestCase):
    def test_real_13mhz_deadlines_and_near_boundary(self):
        run_c(r'''
#include <assert.h>
#include <limits.h>
#include "idle-completion-policy.h"
int main(void) {
 assert(y2_gpt_ticks_ns(13)==1000 && y2_gpt_ticks_ns(26000)==2000000);
 assert(y2_gpt_ticks_ns(UINT_MAX)==330382099615ULL); /* no 32-bit multiplication */
 assert(y2_dormant_gpt_ready(1,0,8,0,100,26100));
 assert(y2_dormant_gpt_ready(3,0,8,0,100,26100)); /* self-clearing reset strobe */
 assert(!y2_dormant_gpt_ready(1,0,8,0,100,26099));
 assert(!y2_dormant_gpt_ready(1,0,8,0,100,100));
 assert(!y2_dormant_gpt_ready(1,0,8,0,UINT_MAX,26000)); /* past, never wrap into future */
 assert(!y2_dormant_gpt_ready(0x11,0,8,0,0,26000)); /* repeating mode */
 assert(!y2_dormant_gpt_ready(0,0,8,0,0,26000));
 assert(!y2_dormant_gpt_ready(1,1,8,0,0,26000));
 assert(!y2_dormant_gpt_ready(1,0,0,0,0,26000));
 assert(!y2_dormant_gpt_ready(1,0,8,8,0,26000));
 for(unsigned f=0;f<1400000;f+=500) assert(y2_dormant_opp(f)==(f==598000||f==747500));
 assert(!y2_dormant_opp(1040000) && !y2_dormant_opp(1196000) && !y2_dormant_opp(1300000));
}
''')

    def test_diagnostics_do_not_fabricate_absent_states_or_mask_owners(self):
        import sys
        import tempfile
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.common import Context
        from y2_platform.observe import idle_completion
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory)
            result = idle_completion(ctx, [])
            self.assertFalse(result['C1']['registered'])
            self.assertIsNone(result['C1']['entries'])
            self.assertIsNone(result['C2']['blocker_owners'])
            self.assertIsNone(result['C3']['unused_clock_handoff'])
            for path, value in (
                ('/sys/module/clocks/parameters/slow_blockers', str((1 << 11) | (1 << 12))),
                ('/sys/bus/platform/devices/18070000.connectivity/power/runtime_status', 'active'),
                ('/sys/bus/platform/devices/11230000.mmc/power/runtime_status', 'suspended'),
                ('/sys/devices/platform/10006000.power-controller/dormant_preflight',
                 'peri_blockers=0x400 infra_blockers=0x80 disp0=0x400 disp1=0x8 prerequisite_topology=1 prerequisite_frequency=0 unmet=frequency,'),
                ('/sys/module/clocks/parameters/unused_handoff',
                 'UART1 sampled=1 lcr=3 ier=0 lsr=0x60 dma=4 reason=quiet retained=0\nSPI0 sampled=1 command=0 status1=1 reason=quiet retained=0'),
            ):
                p = ctx.path(path)
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text(value)
            rows = [dict(cpu='cpu0', name='WFI', disabled=0, usage=10, time_us=1000, rejected=0)]
            result = idle_completion(ctx, rows)
            self.assertTrue(result['C1']['enabled'])
            self.assertEqual(result['C1']['residency_us'], 1000)
            self.assertEqual([x['clock'] for x in result['C2']['blocker_owners']], ['APDMA', 'MSDC0'])
            self.assertEqual([x['runtime_status'] for x in result['C2']['blocker_owners']], ['active', 'suspended'])
            self.assertEqual(result['C3']['unmet'], ['frequency'])
            self.assertIn('dma=4 reason=quiet', result['C3']['unused_clock_handoff'])
            self.assertEqual([x['clock'] for x in result['C3']['blocker_owners']], ['USB0', 'L2C_SRAM', 'MDP_WROT', 'DPI_ENGINE'])

    def test_dt_authorizes_only_the_exact_new_usb_and_existing_consumers(self):
        from tools.validation.dev_dtb import clock_id_allowed
        top = '/clock-controller@10000000'
        for clock in range(46):
            self.assertEqual(clock_id_allowed(top, '/usb@11200000', 1, clock, True, True, True), clock == 44)
            self.assertEqual(clock_id_allowed(top, '/i2c@11007000', 1, clock, True, True, True), clock < 25)
        self.assertFalse(clock_id_allowed('/syscon@14000000', '/usb@11200000', 1, 0, True, True, True))
        self.assertFalse(clock_id_allowed(top, '/usb@11200000', 2, 44, True, True, True))

    def test_legitimate_unused_controller_activity_is_preserved(self):
        run_c(r'''
#include <assert.h>
#include "idle-clock-policy.h"
int main(void) {
 assert(!y2_unused_uart_busy(0,0,0x60,0));
 assert(y2_unused_uart_busy(0x80,0,0x60,0));
 assert(y2_unused_uart_busy(0,1,0x60,0));
 assert(y2_unused_uart_busy(0,0,0x61,0));
 assert(y2_unused_uart_busy(0,0,0x40,0));
 assert(y2_unused_uart_busy(0,0,0x60,1));
 assert(!y2_unused_uart_busy(3,0,0x60,4)); /* timeout counter metadata */
 for(unsigned bit=0;bit<32;bit++) {
  if(bit!=2) assert(y2_unused_uart_busy(3,0,0x60,1U<<bit));
  if(bit!=5 && bit!=6) assert(y2_unused_uart_busy(3,0,0x60|(1U<<bit),0));
 }
 assert(!y2_unused_nfi_busy(0,0,0,0));
 assert(y2_unused_nfi_busy(0x100,0,0,0));
 assert(y2_unused_nfi_busy(0,1,0,0));
 assert(y2_unused_nfi_busy(0,0,1,0));
 assert(y2_unused_nfi_busy(0,0,0,1));
 assert(!y2_unused_spi_busy(0,1)); /* MT6582 STATUS1 idle polarity */
 assert(y2_unused_spi_busy(0,0));
 assert(!y2_unused_spi_busy(0x13020,1)); /* configuration is not activity */
 for(unsigned bit=0;bit<32;bit++) {
  if((1U<<bit)&0xc17) assert(y2_unused_spi_busy(1U<<bit,1));
  if(bit) assert(y2_unused_spi_busy(0,1|(1U<<bit)));
 }
}
''')

    def test_unused_owner_preserves_partial_clocks_and_captures_real_decision(self):
        source = (ROOT/'kernel/platform/clocks.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#include "idle-clock-policy.h"
#define __iomem
#define BIT(x) (1U<<(x))
struct device {int unused;};
struct y2_unused_handoff {
 unsigned sampled,line_sampled,lcr,ier,lsr,dma,command,status;int busy;
};
static struct y2_unused_handoff unused_handoff[4];
static void *unused_registers[4];
static const unsigned y2_unused_bits[]={0,2,3,4,5,6,7,8,9,15,17,18,19,25,7,13,15};
static unsigned registers[0x220/4], reads, maps, address, map_failure;
static void *devm_ioremap(struct device *d,unsigned a,unsigned size) {
 assert(size<=sizeof(registers));maps++;address=a;
 return map_failure?NULL:registers;
}
static unsigned readl(void *p){reads++;return *(unsigned *)p;}
''' + function(source, 'y2_unused_busy') + r'''
int main(void) {
 struct device dev;
 /* A partial inherited NAND/PWM handoff retains a gate, never aborts the
  * shared CCF provider or accesses an unclocked register partner. */
 assert(y2_unused_busy(&dev,0,BIT(15))==1 && !reads && !maps);
 assert(y2_unused_busy(&dev,9,BIT(0))==1 && !reads && !maps);
 assert(y2_unused_busy(&dev,1,BIT(9))==1 && !reads && !maps);
 registers[0x14/4]=0x60;registers[0x4c/4]=4;
 assert(!y2_unused_busy(&dev,10,0) && address==0x11003000);
 assert(unused_handoff[0].sampled && unused_handoff[0].dma==4 && !unused_handoff[0].busy);
 registers[4/4]=1;
 assert(y2_unused_busy(&dev,11,0) && address==0x11004000);
 assert(unused_handoff[1].ier==1 && unused_handoff[1].busy);
 registers[0x20/4]=1;registers[0x18/4]=0;
 assert(!y2_unused_busy(&dev,13,0) && address==0x1100a000);
 assert(unused_handoff[3].sampled && unused_handoff[3].status==1 && !unused_handoff[3].busy);
 /* STATUS0 is clear-on-read; the diagnostic never touches it. */
 assert(reads==9 && !unused_handoff[1].line_sampled); /* no live IRQ-owner LSR */
 registers[0xc/4]=0xbf;
 assert(y2_unused_busy(&dev,12,0) && reads==10); /* no aliased IER/LSR access */
 assert(unused_handoff[2].lcr==0xbf && unused_handoff[2].busy);
 map_failure=1;unused_registers[2]=NULL;
 assert(y2_unused_busy(&dev,12,0)==-ENOMEM && reads==10);
}
''')

    def test_actual_late_handoff_rechecks_busy_owners_without_unclocked_reads(self):
        source = (ROOT/'kernel/platform/clocks.c').read_text().replace(
            'void y2_ccf_reclaim_unused(', 'static void y2_ccf_reclaim_unused(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include <stddef.h>
#include "idle-clock-policy.h"
#define __iomem
#define BIT(x) (1U<<(x))
#define ARRAY_SIZE(x) (sizeof(x)/sizeof((x)[0]))
#define smp_load_acquire(p) (*(p))
struct device {int unused;};
struct y2_clock {unsigned bit;bool inherited;};
struct clk {struct y2_clock *c;unsigned refs;bool fail,drop;};
struct y2_unused_handoff {unsigned sampled,line_sampled,lcr,ier,lsr,dma,command,status;int busy;};
static struct y2_unused_handoff unused_handoff[4];
static unsigned registers[4][0x220/4], peri[0x30/4], reads, enabled_calls;
static void *unused_registers[4],*y2_clock_bases[4];
static struct y2_clock clocks[4],*unused_hw[4];
static struct clk handles[4],*unused_clk[4];
static struct device dev,*unused_dev=&dev;
static bool unused_ready;
static int radio_status[2];
static int y2_spm_radio_status(unsigned d){return radio_status[d];}
static unsigned unused_rechecks,unused_reclaimed,unused_failures;
static int unused_handoff_lock,y2_clk_lock;
static const unsigned y2_unused_bits[]={0,2,3,4,5,6,7,8,9,15,17,18,19,25,7,13,15};
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){assert(*p);*p=0;}
#define spin_lock_irqsave(p,f) do{f=0;assert(!*(p));*(p)=1;}while(0)
#define spin_unlock_irqrestore(p,f) do{(void)f;assert(*(p));*(p)=0;}while(0)
static void *devm_ioremap(struct device *d,unsigned a,unsigned size){assert(0);return NULL;}
static unsigned readl(void *p) {
 for(unsigned i=0;i<4;i++)
  if((unsigned long)p>=(unsigned long)registers[i] &&
     (unsigned long)p<(unsigned long)(registers[i]+0x220/4)) {
   assert(!(peri[0x18/4]&BIT(clocks[i].bit))); /* no unclocked operands */
   assert(handles[i].refs);assert(unused_handoff_lock && y2_clk_lock);
  }
 reads++;return *(unsigned *)p;
}
static int clk_prepare_enable(struct clk *c) {
 assert(unused_handoff_lock && !y2_clk_lock);enabled_calls++;
 if(c->fail)return -EIO;c->refs++;return 0;
}
static void clk_disable_unprepare(struct clk *c) {
 assert(unused_handoff_lock && !y2_clk_lock && c->refs);
 if(!--c->refs && !c->c->inherited && !c->drop) peri[0x18/4]|=BIT(c->c->bit);
}
static unsigned __clk_get_enable_count(struct clk *c){return c->refs;}
''' + function(source, 'y2_unused_busy') + function(source, 'y2_ccf_reclaim_unused') + r'''
int main(void) {
 y2_clock_bases[1]=peri;
 for(unsigned i=0;i<4;i++) {
  clocks[i].bit=y2_unused_bits[i+10];clocks[i].inherited=true;
  unused_hw[i]=clocks+i;handles[i].c=clocks+i;unused_clk[i]=handles+i;
  unused_registers[i]=registers[i];registers[i][0xc/4]=3;registers[i][0x14/4]=0x60;
 }
 y2_ccf_reclaim_unused();assert(!reads && !enabled_calls);
 unused_ready=true;
 radio_status[0]=1;y2_ccf_reclaim_unused();assert(!reads && !enabled_calls);
 radio_status[0]=0;radio_status[1]=-EIO;
 y2_ccf_reclaim_unused();assert(!reads && !enabled_calls);radio_status[1]=0;
 registers[0][4/4]=1;registers[1][0x4c/4]=1;registers[2][0x14/4]=0x61;
 y2_ccf_reclaim_unused();assert(!peri[0x18/4] && unused_rechecks==4 && !unused_reclaimed);
 for(unsigned i=0;i<4;i++)assert(clocks[i].inherited && !handles[i].refs);
 registers[0][4/4]=0;registers[1][0x4c/4]=4;registers[2][0x14/4]=0x60;
 registers[3][0x20/4]=1;
 y2_ccf_reclaim_unused();assert(unused_reclaimed==4 && unused_rechecks==8 && !unused_failures);
 unsigned before=reads;y2_ccf_reclaim_unused();assert(reads==before);
 /* Externally gated inherited clock: no borrow/re-enable and no operand read. */
 clocks[0].inherited=true;before=enabled_calls;y2_ccf_reclaim_unused();assert(enabled_calls==before);
 /* Real Linux consumer retains its ref even after handoff becomes quiet. */
 peri[0x18/4]&=~BIT(17);handles[0].refs=1;
 y2_ccf_reclaim_unused();assert(handles[0].refs==1 && !clocks[0].inherited && !(peri[0x18/4]&BIT(17)));
 /* Gate readback failure leaves a real blocker and stops further gate writes. */
 clocks[0].inherited=true;handles[0].refs=0;handles[0].drop=true;
 y2_ccf_reclaim_unused();assert(unused_failures==1 && !unused_clk[0] && !(peri[0x18/4]&BIT(17)));
 before=enabled_calls;y2_ccf_reclaim_unused();assert(enabled_calls==before);
 /* Failed CCF borrow keeps the original protection and balances no fake ref. */
 clocks[1].inherited=true;handles[1].fail=true;peri[0x18/4]&=~BIT(18);
 y2_ccf_reclaim_unused();assert(unused_failures==2 && clocks[1].inherited && !handles[1].refs);
}
''')


class DisplayClocks(unittest.TestCase):
    def test_real_owner_quiet_gates_busy_retention_and_coupled_snapshot(self):
        source = (ROOT/'kernel/platform/mm-clocks.c').read_text().replace(
            'void y2_mm_reclaim_unused(', 'static void y2_mm_reclaim_unused(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include "mt6582-clk.h"
#define __iomem
struct clk {unsigned refs;bool enabled;};
static struct clk gates[CLK_MM_NR_CLK], *mm_idle_clocks[CLK_MM_NR_CLK];
static unsigned registers[0x12000/4], reclaimed, retained, reclaim_failures;
static void *mm_idle_base=registers;
static bool bls_handoff,handoff_broken;
static int powered=1,isp;
static int y2_spm_disp_status(void){return powered;}
static int y2_spm_isp_status(void){return isp;}
static unsigned readl(void *p){
 unsigned offset=(unsigned *)p-registers;
 if(offset==0xd000/4 || offset==0xd040/4)
  assert(gates[CLK_MM_DPI_DIGITAL_LANE].enabled && gates[CLK_MM_DPI_ENGINE].enabled);
 return *(unsigned *)p;
}
static void writel(unsigned value,void *p){*(unsigned *)p=value;}
static bool __clk_is_enabled(struct clk *c){return c->enabled;}
static int clk_prepare_enable(struct clk *c){c->refs++;c->enabled=true;return 0;}
static void clk_disable_unprepare(struct clk *c){assert(c->refs);if(!--c->refs)c->enabled=false;}
''' + function(source, 'y2_mm_unused_quiet') + function(source, 'y2_mm_reclaim_unused') + r'''
int main(void){
 for(unsigned i=0;i<CLK_MM_NR_CLK;i++){gates[i].enabled=true;mm_idle_clocks[i]=gates+i;}
 /* Normal DRM owner has stopped engines, but a real unsupported DMA and
  * CMDQ thread remain busy. Those clocks must survive untouched. */
 registers[0x9008/4]=1;registers[0xf104/4]=1;registers[0x507c/4]=1;
 registers[0x1408/4]=0x100;registers[0xa000/4]=0x10001;
 y2_mm_reclaim_unused();
 assert(bls_handoff && !handoff_broken && !registers[0xa000/4]);
 assert(gates[CLK_MM_DISP_WDMA].enabled && gates[CLK_MM_CMDQ].enabled);
 assert(gates[CLK_MM_DISP_OVL].enabled && gates[CLK_MM_SMI_COMMON].enabled);
 assert(!gates[CLK_MM_DPI_ENGINE].enabled && !gates[CLK_MM_DPI_DIGITAL_LANE].enabled);
 assert(gates[CLK_MM_MDP_WROT].enabled); /* reset completed but still enabled */
 assert(reclaimed && retained==3 && !reclaim_failures);
 registers[0x9008/4]=0;registers[0xf104/4]=0;registers[0x507c/4]=0;
 y2_mm_reclaim_unused();assert(!gates[CLK_MM_DISP_WDMA].enabled && !gates[CLK_MM_CMDQ].enabled);
 /* Partially gated DPI cannot read its unclocked partner's registers. */
 gates[CLK_MM_DPI_ENGINE].enabled=true;gates[CLK_MM_DPI_DIGITAL_LANE].enabled=false;
 y2_mm_reclaim_unused();assert(gates[CLK_MM_DPI_ENGINE].enabled);
 unsigned before=reclaimed;powered=0;y2_mm_reclaim_unused();assert(reclaimed==before);
}
''')


class UsbIdleClock(unittest.TestCase):
    def test_actual_save_gate_restore_owner_refuses_session_dma_fifo_and_irq(self):
        source = (ROOT/'kernel/platform/usb.c').read_text()
        for name in ('y2_musb_runtime_gate', 'y2_musb_runtime_clock'):
            source = source.replace('int '+name+'(', 'static int '+name+'(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
#define READ_ONCE(x) (x)
#define BIT(x) (1U<<(x))
#define MUSB_POWER 1
#define MUSB_DEVCTL 0x60
#define MUSB_POWER_SOFTCONN 0x40
#define MUSB_DEVCTL_SESSION 1
#define MUSB_TXCSR_TXPKTRDY 1
#define MUSB_TXCSR_FIFONOTEMPTY 2
#define MUSB_RXCSR_RXPKTRDY 1
#define MUSB_RXCSR_FIFOFULL 2
struct config {unsigned num_eps;};
struct index_context {unsigned txcsr,rxcsr;};
struct musb {void *mregs;int lock;struct config *config;
 struct {struct index_context index_regs[5];} context;};
static unsigned char registers[0x300];
static struct config config={5};
static struct musb owner={.mregs=registers,.config=&config},other;
static struct musb *y2_musb=&owner;
static bool y2_usb_detached,y2_usb_clock_enabled=true;
static unsigned y2_usb_clock_gates,y2_usb_clock_restores,y2_usb_clock_failures,mask,refs=1;
static int y2_usb_bus_clock,fail_enable;
#define spin_lock_irqsave(p,f) do{f=0;assert(!*(p));*(p)=1;}while(0)
#define spin_unlock_irqrestore(p,f) do{(void)f;assert(*(p));*(p)=0;}while(0)
static unsigned readl(void *p){unsigned char *s=p;return s[0]|s[1]<<8|s[2]<<16|s[3]<<24;}
static unsigned readw(void *p){unsigned char *s=p;return s[0]|s[1]<<8;}
static unsigned readb(void *p){return *(unsigned char *)p;}
static void clk_disable(int clock){assert(owner.lock && refs==1);refs--;mask|=BIT(10);}
static int clk_enable(int clock){if(fail_enable)return -EIO;assert(!refs);refs++;mask&=~BIT(10);return 0;}
static int y2_ccf_usb_read(unsigned address,unsigned *value){assert(address==0x10003018);*value=mask;return 0;}
''' + function(source, 'y2_musb_runtime_gate') + function(source, 'y2_musb_runtime_clock') + r'''
int main(void){
 assert(!y2_musb_runtime_gate(&other) && refs==1);
 assert(y2_musb_runtime_gate(&owner)==-EBUSY && refs==1);
 y2_usb_detached=true;registers[0xa4]=1;
 assert(y2_musb_runtime_gate(&owner)==-EBUSY && refs==1);registers[0xa4]=0;
 registers[MUSB_POWER]=0x40;assert(y2_musb_runtime_gate(&owner)==-EBUSY);registers[MUSB_POWER]=0;
 registers[MUSB_DEVCTL]=1;assert(y2_musb_runtime_gate(&owner)==-EBUSY);registers[MUSB_DEVCTL]=0;
 for(unsigned i=0;i<8;i++){
  registers[0x204+16*i]=1;assert(y2_musb_runtime_gate(&owner)==-EBUSY && refs==1);
  registers[0x204+16*i]=0;
 }
 owner.context.index_regs[3].txcsr=2;assert(y2_musb_runtime_gate(&owner)==-EBUSY && refs==1);
 owner.context.index_regs[3].txcsr=0;owner.context.index_regs[2].rxcsr=1;
 assert(y2_musb_runtime_gate(&owner)==-EBUSY && refs==1);owner.context.index_regs[2].rxcsr=0;
 assert(!y2_musb_runtime_gate(&owner) && !y2_usb_clock_enabled && !refs && y2_usb_clock_gates==1);
 assert(!y2_musb_runtime_clock(&owner) && y2_usb_clock_enabled && refs==1 && !mask && y2_usb_clock_restores==1);
 assert(!y2_musb_runtime_clock(&owner) && refs==1); /* no duplicate clock reference */
 assert(!y2_musb_runtime_gate(&owner));fail_enable=1;
 assert(y2_musb_runtime_clock(&owner)==-EIO && !y2_usb_clock_enabled && y2_usb_clock_failures==1);
 fail_enable=0;assert(y2_musb_runtime_clock(&owner)==-EIO && !refs); /* quarantined */
}
''')

    def test_generic_runtime_pm_order_preserves_all_saved_endpoints(self):
        from tools.build.run import prepare_overlay
        import json
        spec = next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/usb/musb/musb_core.c')
        source = prepare_overlay(ROOT, spec).read_text()
        suspend = function(source, 'musb_runtime_suspend')
        self.assertLess(suspend.index('musb_save_context(musb)'), suspend.index('y2_musb_runtime_gate(musb)'))
        self.assertLess(suspend.index('if (error) return error'), suspend.index('musb->is_runtime_suspended = 1'))
        resume = function(source, 'musb_runtime_resume')
        self.assertLess(resume.index('y2_musb_runtime_clock(musb)'), resume.index('musb_restore_context(musb)'))
        self.assertLess(resume.index('if (error) return error'), resume.index('musb_restore_context(musb)'))


class Dormant(unittest.TestCase):
    def test_actual_local_timer_save_restore_and_unsaved_abort(self):
        source = (ROOT/'kernel/platform/local-timer.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
typedef uint64_t u64;typedef uint32_t u32;
struct notifier_block {int unused;};
struct y2_timer_context {u64 compare;u32 control,frequency;bool valid;};
static struct y2_timer_context timer_context;
#define this_cpu_ptr(p) (p)
#define CPU_PM_ENTER 1
#define CPU_PM_EXIT 2
#define CPU_PM_ENTER_FAILED 3
#define NOTIFY_OK 0
#define NOTIFY_BAD 1
#define isb() do{}while(0)
static unsigned context_saves,context_restores,context_failures,writes;
static u64 compare=0x123456789abcdef0ULL;
static u32 control=1,frequency=13000000;
static bool drop_compare;
static bool y2_local_timer_ready(void){return true;}
static u64 y2_cntp_compare(void){return compare;}
static u32 y2_cntp_control(void){return control;}
static u32 y2_cntfrq_read(void){return frequency;}
static void y2_cntfrq_write(u32 value){assert(!control);frequency=value;writes++;}
static void y2_cntp_set_control(u32 value){control=value;writes++;}
static void y2_cntp_set_compare(u64 value){assert(!control);if(!drop_compare)compare=value;writes++;}
''' + function(source, 'y2_timer_cpu_pm') + r'''
int main(void){
 assert(!y2_timer_cpu_pm(0,CPU_PM_ENTER_FAILED,0) && !writes && !context_restores);
 frequency=26000000;assert(y2_timer_cpu_pm(0,CPU_PM_ENTER,0)==NOTIFY_BAD && !timer_context.valid);
 assert(!y2_timer_cpu_pm(0,CPU_PM_ENTER_FAILED,0) && !writes);
 frequency=13000000;assert(!y2_timer_cpu_pm(0,CPU_PM_ENTER,0) && context_saves==1);
 frequency=0;control=0;compare=0;
 assert(!y2_timer_cpu_pm(0,CPU_PM_EXIT,0));
 assert(frequency==13000000 && control==1 && compare==0x123456789abcdef0ULL);
 assert(context_restores==1 && !context_failures && !timer_context.valid);
 assert(!y2_timer_cpu_pm(0,CPU_PM_ENTER,0));
 compare=1;drop_compare=true;
 assert(!y2_timer_cpu_pm(0,CPU_PM_ENTER_FAILED,0) && context_failures==1 && context_restores==2);
}
''')

    def test_actual_entry_guards_budget_real_return_and_restore_quarantine(self):
        source = (ROOT/'kernel/platform/spm.c').read_text().replace(
            'int y2_spm_dormant_idle(', 'static int y2_spm_dormant_idle(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <errno.h>
#include "idle-completion-policy.h"
#define BIT(x) (1U<<(x))
#define Y2_SPM_SECONDARY_CPU_MASK 0xe00U
#define SPM_PWR_STATUS 0
#define SPM_PWR_STATUS_S 4
#define SPM_PCM_REG9_DATA 8
#define SPM_PCM_REG_DATA_INI 12
#define SPM_PCM_EVENT_REG_STA 16
#define SPM_PCM_REG13_DATA 20
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define smp_load_acquire(p) (*(p))
#define __pa_symbol(p) 0x80008000UL
#define dsb(x) do{}while(0)
typedef uint64_t u64;
static unsigned regs[6],biu,cache;
static void *spm_base=regs,*spm_biu=&biu,*spm_cache=&cache;
static int spm_lock,spm_io,idle_address,normal_address;
static unsigned dormant_attempts,dormant_entries,dormant_resumes,dormant_successes;
static unsigned dormant_aborts,dormant_failures,dormant_restore_failures;
static unsigned dormant_wake,dormant_debug,dormant_event,dormant_r13;
static bool spm_broken,dormant_broken;
static int dormant_result,dormant_budget,dormant_wake_result;
static const char *dormant_wake_stage;
static bool dormant_qualification;
#define SYSTEM_RUNNING 0
static int system_state;
enum {Y2_PM_DORMANT_BEGIN,Y2_PM_DORMANT_CONTEXT,Y2_PM_DORMANT_RETURN,Y2_PM_DORMANT_COMPLETE,Y2_PM_DORMANT_ABORTED,Y2_PM_DORMANT_RESTORE_PCM,Y2_PM_DORMANT_RESTORE_CONTEXT,Y2_PM_DORMANT_RESTORE_CIRQ,Y2_PM_DORMANT_RESTORE_CLOCKS};
static bool backstop_ready=true,backstop_running;
static bool y2_pm_dormant_backstop_ready(void){return backstop_ready;}
static bool y2_pm_dormant_backstop_running(void){return backstop_running;}
static void y2_pm_backstop_begin(bool staged){backstop_running=true;}
static void y2_pm_backstop_end(void){backstop_running=false;}
static void y2_pm_mark(unsigned stage,int error){}
static char *dormant_stage;
static u64 dormant_residency_us,ns;
static unsigned cpu,online=1,khz=598000,suspends;
static bool disabled,events=true,cirq=true,dark=true,workload,context=true;
static int timer_error,clock_error,cirq_error,pm_error,cluster_error,arm_error,finish_error,restore_error;
static bool timer_ok=true,cirq_ok=true,clocks_owned,cirq_owned,cpu_owned,cluster_owned;
static bool y2_deep_idle_disabled(void){return disabled;}
static bool y2_local_events_ready(void){return events;}
static bool y2_cirq_ready(void){return cirq;}
static unsigned smp_processor_id(void){return cpu;}
static unsigned num_online_cpus(void){return online;}
static unsigned cpufreq_quick_get(unsigned c){return khz;}
static bool y2_backlight_dark(void){return dark;}
static bool y2_workload_idle_blocked(void){return workload;}
static bool y2_dormant_context_ready(void){return context;}
static bool y2_cirq_restore_ok(void){return cirq_ok;}
static bool y2_local_timer_context_ok(void){return timer_ok;}
static bool y2_usb_idle_ok(void){return true;}
static int y2_local_timer_dormant_check(void){return timer_error;}
static int raw_spin_trylock(int *p){if(*p)return 0;*p=1;return 1;}
static void raw_spin_unlock(int *p){assert(*p);*p=0;}
static unsigned spm_read(void *p,unsigned r){return ((unsigned *)p)[r/4];}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){*(unsigned *)p=v;}
static void y2_cpu_resume(void){}
static void __cpuc_flush_dcache_area(void *p,unsigned n){}
static int y2_ccf_boot_vector(unsigned long p){return 0;}
static int y2_ccf_deep_idle_begin(void){if(clock_error)return clock_error;clocks_owned=true;return 0;}
static int y2_ccf_deep_idle_end(void){assert(clocks_owned);clocks_owned=false;return restore_error;}
static int y2_cirq_begin(void){if(cirq_error)return cirq_error;cirq_owned=true;return 0;}
static void y2_cirq_end(void){assert(cirq_owned);cirq_owned=false;}
static int cpu_pm_enter(void){if(pm_error)return pm_error;cpu_owned=true;return 0;}
static void cpu_pm_exit(void){assert(cpu_owned);cpu_owned=false;}
static int cpu_cluster_pm_enter(void){if(cluster_error)return cluster_error;cluster_owned=true;return 0;}
static void cpu_cluster_pm_exit(void){assert(cluster_owned);cluster_owned=false;}
static int y2_spm_idle_arm(int *io,unsigned address){return arm_error;}
static int y2_spm_idle_restore(int *io,unsigned address){return 0;}
static int y2_spm_finish(unsigned long arg){return 0;}
static int cpu_suspend(unsigned long arg,int (*fn)(unsigned long)){
 assert(arg==1 && fn==y2_spm_finish && clocks_owned && cirq_owned && cpu_owned && cluster_owned);
 suspends++;ns+=5000000;return finish_error;
}
static u64 ktime_get_mono_fast_ns(void){return ++ns;}
static u64 div_u64(u64 n,unsigned d){return n/d;}
''' + function(source, 'y2_spm_dormant_idle') + r'''
int main(void){
 dormant_budget=1;backstop_ready=false;assert(y2_spm_dormant_idle()==-EACCES && !suspends);
 backstop_ready=true;disabled=true;assert(y2_spm_dormant_idle()==-ENODEV && dormant_result==-ENODEV);
 disabled=false;online=4;assert(y2_spm_dormant_idle()==-EBUSY && !suspends);online=1;
 khz=1300000;assert(y2_spm_dormant_idle()==-EBUSY);khz=598000;
 dark=false;assert(y2_spm_dormant_idle()==-EBUSY);dark=true;
 workload=true;assert(y2_spm_dormant_idle()==-EBUSY);workload=false;
 dormant_budget=0;assert(y2_spm_dormant_idle()==-EACCES);dormant_budget=1;
 timer_error=-ETIME;assert(y2_spm_dormant_idle()==-ETIME && dormant_budget==1);timer_error=0;
 regs[0]=0x800;assert(y2_spm_dormant_idle()==-EBUSY && !spm_lock);regs[0]=0;
 regs[1]=2;assert(y2_spm_dormant_idle()==-EBUSY);regs[1]=0;
 /* Powered DISP with completely quiescent clock masks is allowed by the BSP. */
 regs[0]=regs[1]=8;
 cirq_error=-EAGAIN;assert(y2_spm_dormant_idle()==-EAGAIN && !clocks_owned);cirq_error=0;
 cluster_error=-EBUSY;assert(y2_spm_dormant_idle()==-EBUSY && !cpu_owned && !cirq_owned);cluster_error=0;
 arm_error=-EBUSY;assert(y2_spm_dormant_idle()==-EBUSY && !suspends && dormant_budget==1);arm_error=0;
 finish_error=-EBUSY;assert(y2_spm_dormant_idle()==-EBUSY && suspends==1 && !dormant_resumes && !dormant_successes);
 assert(!dormant_budget && !spm_lock && !clocks_owned && !cpu_owned && !cirq_owned && !cluster_owned);
 assert(y2_spm_dormant_idle()==-EACCES && suspends==1); /* never hammer a failed first test */
 dormant_budget=1;finish_error=0;
 assert(!y2_spm_dormant_idle() && dormant_resumes==1 && dormant_successes==1 && dormant_residency_us==5000);
 assert(!dormant_wake_result && dormant_wake_stage);
 assert(y2_spm_dormant_idle()==-EACCES && dormant_result==-EACCES && !dormant_wake_result);
 dormant_budget=1;restore_error=-EIO;
 assert(y2_spm_dormant_idle()==-EIO && dormant_resumes==2 && dormant_successes==1);
 assert(dormant_broken && dormant_wake_result==-EIO && dormant_restore_failures==1 && !clocks_owned && !spm_lock);
 dormant_budget=-1;assert(y2_spm_dormant_idle()==-EIO && suspends==3);
}
''')


class Renderer(unittest.TestCase):
    def test_actual_drm_sleep_retains_buffer_and_rolls_back_failed_modeset(self):
        reborn = ROOT.parent/'Y2Reborn'
        if not reborn.is_dir():
            reborn = Path('/tmp/Y2Reborn')  # locked production builder mount
        source = (reborn/'crates/reborn-graphics/native/graphics.c').read_text()
        source = source.replace('int rb_graphics_sleep(RbGraphics *g, int off) {', 'static int rb_graphics_sleep(RbGraphics *g, int off)\n{')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <errno.h>
struct fb {unsigned id;};struct flip {bool waiting;};
typedef struct {int fd,modeset;bool sleeping;unsigned crtc,connector;int mode;
 void *current;struct flip flip;} RbGraphics;
static int fail,timeout,calls;static unsigned last_fb;
static struct fb fb={42};
static int wait_flip(int fd,struct flip *p){if(timeout)return -1;p->waiting=false;return 0;}
static struct fb *framebuffer(int fd,void *bo){return bo?&fb:NULL;}
static int drmModeSetCrtc(int fd,unsigned crtc,unsigned f,int x,int y,unsigned *conn,int count,int *mode){
 assert(crtc==7);calls++;last_fb=f;
 if(f)assert(conn && *conn==9 && count==1 && *mode==60);
 else assert(!conn && !count && !mode);
 if(fail){errno=EIO;return -1;}return 0;
}
''' + function(source, 'rb_graphics_sleep') + r'''
int main(void){
 int bo;RbGraphics g={.modeset=1,.crtc=7,.connector=9,.mode=60,.current=&bo};
 assert(rb_graphics_sleep(NULL,1)==-EINVAL);
 fail=1;assert(rb_graphics_sleep(&g,1)==-EIO && !g.sleeping && g.current==&bo);
 fail=0;assert(!rb_graphics_sleep(&g,1) && g.sleeping && !last_fb && g.current==&bo);
 int before=calls;assert(!rb_graphics_sleep(&g,1) && calls==before);
 fail=1;assert(rb_graphics_sleep(&g,0)==-EIO && g.sleeping && g.current==&bo);
 fail=0;assert(!rb_graphics_sleep(&g,0) && !g.sleeping && last_fb==42);
 g.flip.waiting=true;timeout=1;before=calls;
 assert(rb_graphics_sleep(&g,1)==-ETIMEDOUT && calls==before && !g.sleeping);
 g.flip.waiting=false;g.modeset=0;g.current=NULL;timeout=0;before=calls;
 assert(!rb_graphics_sleep(&g,1) && calls==before+1 && !last_fb && g.sleeping);
 assert(!rb_graphics_sleep(&g,0) && calls==before+1 && !g.sleeping); /* first present owns wake modeset */
}
''')
