"""Exercise the production charger sequencer and worker, without physical IO."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]

FIXTURE = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include "policy.h"
#include "power-math.h"
#include "charger-policy.h"
#include "source-policy.h"
#include "charge-limits.h"
#define READ_ONCE(v) (v)
#define WRITE_ONCE(v,x) ((v)=(x))
#define min(a,b) ((a)<(b)?(a):(b))
#define BIT(n) (1U<<(n))
typedef unsigned long long u64;
static unsigned regs[1024], operations, fail_at, force_count, starts, adc_fail;
static int persistent, corrupt_readback, enabled_once;
static u64 clock_ms;
#define ktime_get_boottime_seconds() (clock_ms/1000)
#define ktime_get_boottime() clock_ms
#define ktime_to_ms(t) (t)
static int io_read(void *ctx,unsigned reg,unsigned *v){
 (void)ctx;
 if(++operations==fail_at || persistent) return -EIO;
 *v=regs[reg/2];
 if(corrupt_readback && reg==0 && enabled_once)*v^=0x10;
 return 0;
}
static int io_update(void *ctx,unsigned reg,unsigned mask,unsigned v,int force){
 (void)ctx;
 if(++operations==fail_at || persistent) return -EIO;
 unsigned old=regs[reg/2], next=(old&~mask)|(v&mask);
 assert(!(mask&~y2_pmic_write_mask(reg)));
 assert(y2_pmic_value_allowed(reg,next));
 if(reg==0x1a && !(next&0x10)) assert(!(regs[0]&0x18));
 if(reg==0 && (next&0x18)) {
  assert((regs[0x1a/2]&0x1f)==0x10 && !(regs[0x1e/2]&4));
  assert((regs[0x3c/2]&0x23)==0x20 && (regs[0xc/2]&0x41)==1);
  assert((regs[4/2]&0xe)==0xe && (regs[0x2e/2]&0xc4)==0xc4);
  assert((regs[6/2]&31)==30 && y2_cs_selector_permitted(regs[8/2]&15));
  enabled_once=1;
  if((next&0x18)==0x18 && (old&0x18)!=0x18) starts++;
 }
 regs[reg/2]=next;
 if(reg==0x1a && (v&mask&0x100)) {assert(force);force_count++;}
 if(reg==0x1e && (v&mask&2)) {assert(force);regs[reg/2]&=~4;}
 return 0;
}
static struct y2_charge_io io={0,io_read,io_update,0};
static unsigned refused_reg, refused_actual;
__attribute__((unused)) static void record_refusal(void *ctx,unsigned reg,unsigned mask,unsigned expected,unsigned actual,int error){
 (void)ctx;(void)mask;(void)expected;assert(error==-EIO);refused_reg=reg;refused_actual=actual;
}
static void reset(void){
 memset(regs,0,sizeof(regs));
 operations=fail_at=force_count=starts=adc_fail=0; clock_ms=10000;
 persistent=corrupt_readback=enabled_once=0;
 regs[0]=0x63;regs[1]=0xf2;regs[2]=0x84;regs[3]=30;regs[4]=15;
 regs[6]=1;regs[7]=5;regs[0x1a/2]=0x10;regs[0x1e/2]=1;
 regs[0x20/2]=5;regs[0x28/2]=0x22;regs[0x2a/2]=0x24;
 regs[0x2c/2]=1;regs[0x2e/2]=0x90;
}
'''

def worker_fixture():
    """The production worker functions, compiled against a register/ADC model."""
    s = (ROOT/'kernel/platform/mt6323-charger.c').read_text()
    declarations = re.search(r'enum y2_charge_fault \{.*?\n\};', s, re.S)[0]
    declarations += re.search(r'struct y2_charger \{.*?\n\};', s, re.S)[0]
    return FIXTURE + r'''
#define Y2_PET_LATE_MS 2500
#define Y2_SOURCE_RETRIES 5
#define POWER_SUPPLY_PROP_CURRENT_MAX 1
#define POWER_SUPPLY_STATUS_UNKNOWN 0
#define POWER_SUPPLY_STATUS_CHARGING 1
#define POWER_SUPPLY_STATUS_DISCHARGING 2
#define POWER_SUPPLY_STATUS_NOT_CHARGING 3
#define POWER_SUPPLY_STATUS_FULL 4
#define POWER_SUPPLY_CHARGE_BEHAVIOUR_AUTO 0
#define POWER_SUPPLY_CHARGE_BEHAVIOUR_INHIBIT_CHARGE 1
struct device {int awake;};struct regmap {int dummy;};
struct iio_channel {int raw;};struct power_supply {int budget;};
struct delayed_work {int dummy;};struct notifier_block {int dummy;};
struct mutex {int dummy;};struct workqueue_struct {int dummy;};
union power_supply_propval {int intval;};
static struct regmap map_identity;
static int regmap_read(struct regmap *m,unsigned r,unsigned *v){assert(m==&map_identity);return io_read(m,r,v);}
static int regmap_update_bits(struct regmap *m,unsigned r,unsigned mask,unsigned v){assert(m==&map_identity);return io_update(m,r,mask,v,0);}
static int regmap_write_bits(struct regmap *m,unsigned r,unsigned mask,unsigned v){assert(m==&map_identity);return io_update(m,r,mask,v,1);}
static struct power_supply supply={500000};
static int source_kind, bc_step, phy_error, removed_at_wait, waits;
/* Comparator outcome of each 80 ms BC1.1 stage by source kind (BSP flow:
 * DCD, then A1 | A2 [+ B2]): 0 SDP [0,0], 2 CDP [0,1,0], 3 nonstandard [1,0],
 * 4 DCP [0,1,1], 5 Apple [1,1]. */
static int bc_comparator(int kind,int step){
 switch(kind){case 2:return step==1;case 3:return step==0;case 4:return step>0;case 5:return step<=1;}
 return 0;
}
static bool phy_owned;
static int y2_usb_charge_allocation(void){return supply.budget;}
static void y2_usb_source_invalidate(void){}
static int y2_usb_bc11_begin(void){assert(!phy_owned);if(phy_error)return phy_error;phy_owned=true;bc_step=0;return 0;}
static int y2_usb_bc11_end(bool data){assert(phy_owned);phy_owned=false;return 0;}
static void msleep(unsigned ms){
 clock_ms+=ms;
 if(++waits==removed_at_wait)regs[0]&=~0x20;
 if(ms==80) {
  /* normal DCD=0, A2=0 gives SDP; DCP is 0,1,1 */
  regs[0x24/2]&=~0x80;
  if(bc_comparator(source_kind,bc_step))regs[0x24/2]|=0x80;
  bc_step++;
 }
}
static int iio_read_channel_raw(struct iio_channel *ch,int *v){
 if(adc_fail)return -ETIMEDOUT;*v=ch->raw;return 0;
}
static int iio_read_channel_processed(struct iio_channel *ch,int *v){return iio_read_channel_raw(ch,v);}
static void pm_stay_awake(struct device *d){d->awake=1;}
static void pm_relax(struct device *d){d->awake=0;}
static bool charger_dormant_blocked=true;
#define dev_err_ratelimited(...) ((void)0)
#define dev_info(...) ((void)0)
''' + declarations + ''.join(function(s,n) for n in (
            'y2_charge_read','y2_charge_update','y2_charge_mismatch','y2_charge_awake','y2_charger_inhibit','y2_charge_account_now',
            'y2_charge_sample','y2_wait_source','y2_charger_detect','y2_charger_target','y2_charger_sample_fault','y2_charger_run')) + r'''
static struct device dev;
static struct iio_channel bat={17000},baton={10388},isense={17000},die={25000};
static void charger(struct y2_charger *c){
 *c=(struct y2_charger){.map=&map_identity,.charge_ceiling_ua=650000,.dev=&dev,.io=io,.battery=&bat,.baton=&baton,.isense=&isense,.die=&die,.input=&supply};
 c->io=(struct y2_charge_io){c,y2_charge_read,y2_charge_update,y2_charge_mismatch};
}
'''


class Charger(unittest.TestCase):
    def test_sequence_every_io_failure_and_preserved_protections(self):
        run_c(FIXTURE + r'''
int main(void){
 reset();assert(!y2_charge_prepare(&io,15));assert(!y2_charge_start(&io,15));
 assert((regs[0]&0x18)==0x18 && starts==1);
 unsigned end=operations;
 for(unsigned fail=1;fail<=end;fail++) {
  reset();fail_at=fail;
  int ret=y2_charge_prepare(&io,15);
  if(!ret)ret=y2_charge_start(&io,15);
  assert(ret<0);
  /* The worker's failure path always invokes this, including partial start. */
  assert(!y2_charge_stop(&io));
  assert(!(regs[0]&0x18));
  assert((regs[0xc/2]==1 || regs[0xc/2]==3) && regs[0xe/2]==5 && regs[0x20/2]==5);
  assert((regs[6/2]&31)==30 && (regs[8/2]&15)==15);
 }
 reset();assert(!y2_charge_prepare(&io,15));assert(!y2_charge_start(&io,15));
 for(int i=0;i<100;i++)assert(!y2_charge_pet(&io));
 assert(force_count==101); /* repeated strobes must never disappear */
 persistent=1;unsigned last_wdt=regs[0x1a/2];
 assert(y2_charge_stop(&io)<0 && regs[0x1a/2]==last_wdt);
 persistent=0;assert(!y2_charge_stop(&io));
 /* Changing critical protection/test bits must prevent enable, without repair. */
 unsigned badregs[]={0,0xc,0xe,0x18,0x20,0x3c,0x44};
 unsigned badbits[]={1,0x40,0x1000,1,0x100,1,0x100};
 for(unsigned i=0;i<7;i++){
  reset();regs[badregs[i]/2]^=badbits[i];
  assert(y2_charge_prepare(&io,15)<0 && !starts);
 }
 /* Any inherited input OVP becomes stock's 7 V (0xb), the low bits kept. */
 for(unsigned hv=0;hv<16;hv++){
  reset();regs[1]=(hv<<4)|2;
  assert(!y2_charge_prepare(&io,15) && regs[1]==0xb2 && !y2_charge_start(&io,15) && starts==1);
 }
 /* Battery OVP: switched on at stock 4.3 V whatever it was (deglitch kept);
  * only an over-voltage the detector reports now refuses, untouched. */
 unsigned ovs[]={0,1,2,3,5,0xf,0x11,0x1f};
 for(unsigned i=0;i<8;i++){
  reset();regs[6]=ovs[i];
  assert(!y2_charge_prepare(&io,15) && (regs[6]&0x4f)==3 && (regs[6]&0x10)==(ovs[i]&0x10));
  assert(!y2_charge_start(&io,15) && starts==1);
 }
 reset();regs[6]=0x41;assert(y2_charge_prepare(&io,15)<0 && regs[6]==0x41 && !starts);
 /* #47: whatever CV earlier software left (31 = 4.1875 V on the reporter's
  * player), charging sets 4.175 V, verifies it and starts. */
 for(unsigned cv=0;cv<32;cv++){
  reset();regs[3]=cv;
  assert(!y2_charge_prepare(&io,15) && (regs[3]&31)==30);
  assert(!y2_charge_start(&io,15) && starts==1);
 }
 /* A refused inherited value names its register (BAT0 check_*). */
 struct y2_charge_io seen={0,io_read,io_update,record_refusal};
 reset();regs[6]=0x43;assert(y2_charge_prepare(&seen,15)==-EIO && refused_reg==0xc && refused_actual==0x43);
 reset();assert(!y2_charge_prepare(&io,15));regs[0x44/2]=0x100;
 assert(y2_charge_start(&io,15)<0 && !starts);
 reset();assert(!y2_charge_prepare(&io,15));regs[0]&=~0x20;
 assert(y2_charge_start(&io,15)<0 && !starts);
 reset();assert(!y2_charge_prepare(&io,15));corrupt_readback=1;
 assert(y2_charge_start(&io,15)<0);corrupt_readback=0;
 assert(!y2_charge_stop(&io) && !(regs[0]&0x18));
}
''')

    def test_elapsed_time_confirmation_and_no_invented_full(self):
        run_c(FIXTURE + r'''
int main(void){
 struct y2_charge_cycle c={0};
 y2_charge_account(&c,3600,4000000);assert(c.total==3600 && !c.cv && !c.timed_out);
 y2_charge_account(&c,1,4050001);assert(c.topoff && c.cv==1);
 y2_charge_account(&c,10799,4000000);assert(c.timed_out); /* voltage dip cannot reset timeout */
 c=(struct y2_charge_cycle){0};
 y2_charge_account(&c,86400,3700000);assert(c.timed_out);
 c=(struct y2_charge_cycle){0};
 /* Stop immediately even when the independent analog CV bit is absent.
  * Six spaced ENGINE-OFF samples complete the voltage-limited cycle. */
 assert(y2_charge_termination(&c,100,4175000,1) && c.hold && !c.full);
 for(unsigned t=101;t<160;t++)assert(!y2_charge_termination(&c,t,4140000,0));
 assert(y2_charge_termination(&c,160,4140000,0) && c.full);
 c.total=1234;c.precharge=10;
 /* 28: a Full that rests at 4.14 V refills below the stock 4.110 V (26: 4.00 V). */
 assert(c.full_rest_uv==4140000 && y2_recharge_uv(&c)==4110000);
 for(unsigned t=170;t<=300;t+=10)assert(!y2_charge_termination(&c,t,4110000,0));
 for(unsigned t=301;t<361;t++)assert(!y2_charge_termination(&c,t,4109999,0));
 assert(y2_charge_termination(&c,361,4109999,0) && !c.hold && !c.full);
 assert(c.total==0 && c.precharge==0 && c.cv==0 && !c.finish);
 /* The measured field loop: the pack stops at 4.175 V under load and rests at
  * 4.09 V. That is Full (it used to restart forever against the 4.11 V rule),
  * and its refill level keeps 30 mV below that rest, never the 4.11 V loop. */
 c=(struct y2_charge_cycle){0};
 assert(y2_recharge_uv(&c)==4000000);                     /* no Full yet: 26 level */
 assert(y2_charge_termination(&c,0,4178759,1) && c.hold && !c.full && c.completing);
 for(unsigned t=1;t<60;t++)assert(!y2_charge_termination(&c,t,4090209,0));
 assert(y2_charge_termination(&c,60,4090209,0) && c.hold && c.full);
 assert(y2_recharge_uv(&c)==4060209);
 for(unsigned t=70;t<=400;t+=10)assert(!y2_charge_termination(&c,t,4070000,0));
 c.full_rest_uv=4020000; assert(y2_recharge_uv(&c)==4000000); /* never below 26's floor */
 /* A hold that rests below the full level is an unfinished charge: it resumes
  * after a sustained window and never reports Full. */
 c=(struct y2_charge_cycle){0};
 assert(y2_charge_termination(&c,0,4176000,1) && c.hold && !c.full);
 for(unsigned t=1;t<=60;t++)assert(!y2_charge_termination(&c,t,4069999,0));
 assert(y2_charge_termination(&c,61,4069999,0) && !c.hold && !c.full);
 /* A single dip and recovery does not restart the window early. */
 c=(struct y2_charge_cycle){0};
 assert(y2_charge_termination(&c,0,4176000,1));
 assert(!y2_charge_termination(&c,30,4069999,0));
 assert(!y2_charge_termination(&c,45,4090000,0));
 for(unsigned t=46;t<105;t++)assert(!y2_charge_termination(&c,t,4090000,0));
 assert(y2_charge_termination(&c,105,4090000,0) && c.full);
 /* High voltage at insertion cannot invent a completed charging cycle. */
 c=(struct y2_charge_cycle){0};
 assert(y2_charge_termination(&c,0,4180000,0));
 for(unsigned t=1;t<=100;t++)assert(!y2_charge_termination(&c,t,4180000,0));
 assert(c.hold && !c.full);
 /* An isolated low sample cannot restart a held battery. */
 assert(!y2_charge_termination(&c,101,3999999,0));
 assert(!y2_charge_termination(&c,151,4000000,0));
 assert(!y2_charge_termination(&c,152,3999999,0));
 for(unsigned t=153;t<212;t++)assert(!y2_charge_termination(&c,t,3999999,0));
 assert(y2_charge_termination(&c,212,3999999,0) && !c.hold);
 /* 29 finish stage: 4.175 V at 450 mA steps to 200 mA, at 200 mA to 70 mA. */
 c=(struct y2_charge_cycle){0};
 c.charge_ua=450000;
 assert(y2_charge_termination(&c,0,4175000,1) && c.finish==200000 && !c.hold);
 c.charge_ua=200000;
 assert(!y2_charge_termination(&c,5,4150000,1) && !c.hold);
 assert(y2_charge_termination(&c,10,4175000,1) && c.finish==70000 && !c.hold);
 c.charge_ua=650000; c.finish=0;
 assert(y2_charge_termination(&c,11,4175000,1) && c.finish==200000);
 /* At 70 mA the 4.175 V target completes the cycle exactly as before. */
 c=(struct y2_charge_cycle){0}; c.finish=70000; c.charge_ua=70000;
 assert(y2_charge_termination(&c,0,4175000,1) && c.hold && c.completing && !c.full);
 for(unsigned t=1;t<60;t++)assert(!y2_charge_termination(&c,t,4150000,0));
 assert(y2_charge_termination(&c,60,4150000,0) && c.full && c.full_rest_uv==4150000);
 assert(y2_recharge_uv(&c)==4110000);
 /* Stock taper rule at 70 mA under hardware CV: net drop <= 10.2 mV for six
  * samples at least 10 s apart (BAT_TopOffModeAction: 150 mA * 68 mOhm). */
 c=(struct y2_charge_cycle){0}; c.finish=70000; c.charge_ua=70000;
 c.net_drop_uv=15000;
 for(unsigned t=0;t<100;t+=10)assert(!y2_charge_termination(&c,t,4170000,1) && !c.hold);
 c.net_drop_uv=10200;
 for(unsigned t=100;t<150;t+=10)assert(!y2_charge_termination(&c,t,4170000,1));
 assert(!y2_charge_termination(&c,145,4170000,1));          /* too soon after the 5th */
 assert(y2_charge_termination(&c,150,4170000,1) && c.hold && c.completing);
 /* One higher-drop sample restarts the six-sample window. */
 c=(struct y2_charge_cycle){0}; c.finish=70000; c.charge_ua=70000; c.net_drop_uv=9000;
 for(unsigned t=0;t<50;t+=10)assert(!y2_charge_termination(&c,t,4170000,1));
 c.net_drop_uv=10201; assert(!y2_charge_termination(&c,50,4170000,1) && !c.taper_samples);
 /* The taper rule never applies at a normal current or without a finish stage. */
 c=(struct y2_charge_cycle){0}; c.charge_ua=450000; c.net_drop_uv=0;
 for(unsigned t=0;t<200;t+=10)assert(!y2_charge_termination(&c,t,4100000,1) && !c.hold);
 c=(struct y2_charge_cycle){0}; c.charge_ua=70000; c.net_drop_uv=0;
 for(unsigned t=0;t<200;t+=10)assert(!y2_charge_termination(&c,t,4100000,1) && !c.hold);
 /* 29 plateau: archive 126 sat at 70 mA, 4.09 V, ~90 mA estimated, the awake Y2
  * consuming it. No 5 mV rise in 10 min completes the cycle (Full needs the rest). */
 c=(struct y2_charge_cycle){0}; c.finish=70000; c.charge_ua=70000; c.net_drop_uv=17900;
 for(unsigned t=0;t<600;t+=30)assert(!y2_charge_termination(&c,t,4089550+(t%60?1000:0),1) && !c.hold);
 assert(y2_charge_termination(&c,600,4090000,1) && c.hold && c.completing);
 /* A real rise restarts the plateau window. */
 c=(struct y2_charge_cycle){0}; c.finish=70000; c.charge_ua=70000; c.net_drop_uv=17900;
 for(unsigned t=0;t<1200;t+=30)assert(!y2_charge_termination(&c,t,4090000+t*10,1) && !c.hold);
 /* Not in the finish stage: no plateau completion. */
 c=(struct y2_charge_cycle){0}; c.charge_ua=450000; c.net_drop_uv=90000;
 for(unsigned t=0;t<2000;t+=30)assert(!y2_charge_termination(&c,t,4090000,1) && !c.hold);
 /* Stock 3-h top-off: a finishing cycle completes, a normal one still faults. */
 c=(struct y2_charge_cycle){0}; c.finish=200000;
 y2_charge_account(&c,1,4100000); y2_charge_account(&c,10800,4100000);
 assert(c.topoff_done && !c.timed_out);
 c.charge_ua=200000; c.net_drop_uv=40000;
 assert(y2_charge_termination(&c,0,4120000,1) && c.hold && c.completing);
 c=(struct y2_charge_cycle){0};
 y2_charge_account(&c,1,4100000); y2_charge_account(&c,10800,4100000);
 assert(c.timed_out && !c.topoff_done);
 /* Unknown current (0): 26 behaviour, the first target sample holds. */
 c=(struct y2_charge_cycle){0}; c.finish=70000;
 assert(y2_charge_termination(&c,0,4175000,1) && c.hold && c.completing);
 /* A resumed maintenance cycle starts without the finish stage. */
 c.full=1; c.full_rest_uv=4150000; c.completing=0;
 for(unsigned t=1;t<61;t++)assert(!y2_charge_termination(&c,t,4100000,0));
 assert(y2_charge_termination(&c,61,4100000,0) && !c.hold && !c.finish && !c.topoff_done);
 reset(); /* reference fixture functions under -Werror */
 assert(!io.read(io.context,0,&operations));
}
''')

    def test_actual_worker_faults_usb_removal_suspend_budget_and_manual_inhibit(self):
        run_c(worker_fixture() + r'''
int main(void){
 reset();struct y2_charger c;charger(&c);
 y2_charger_run(&c);assert(c.active && dev.awake && charger_dormant_blocked && !c.fault && c.pets==1);
 unsigned end=operations;
 /* Every initial prepare/sample/enable/service failure reaches inhibit. */
 for(unsigned fail=1;fail<=end;fail++) {
  reset();charger(&c);fail_at=fail;
  y2_charger_run(&c);
  assert(!c.active && !(regs[0]&0x18));
  assert(c.fault && c.status!=POWER_SUPPLY_STATUS_CHARGING);
 }
 reset();charger(&c);y2_charger_run(&c);
 clock_ms+=1000;y2_charger_run(&c);assert(c.active && c.pets==2);
 unsigned active_ops=operations-end;
 for(unsigned fail=1;fail<=active_ops;fail++) {
  reset();charger(&c);y2_charger_run(&c);
  fail_at=operations+fail;clock_ms+=1000;y2_charger_run(&c);
  assert(!c.active && !(regs[0]&0x18) && c.fault);
 }
 /* POWER-FINAL-16 Defect C: like stock, an SDP charges at 450 mA whatever the
  * gadget negotiated (reset, USB suspend of a powered-off PC, unconfigured).
  * Without a data controller (-1) it stays at 70 mA. */
 int ordinary[]={0,2000,8000,100000,499999,500000,-1};
 unsigned currents[]={450000,450000,450000,450000,450000,450000,70000};
 for(unsigned i=0;i<7;i++) {
  reset();charger(&c);supply.budget=ordinary[i];y2_charger_run(&c);
  assert(c.active && c.charge_ua==currents[i]);
 }
 supply.budget=500000;
 reset();charger(&c);y2_charger_run(&c);supply.budget=2000;
 clock_ms+=1000;y2_charger_run(&c);assert(c.active && !c.fault && starts==1);
 supply.budget=500000;clock_ms+=1000;y2_charger_run(&c);assert(c.active && starts==1);
 regs[0]&=~0x20;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && !(regs[0]&0x18) && c.status==POWER_SUPPLY_STATUS_DISCHARGING);
 regs[0]|=0x20;clock_ms+=1000;y2_charger_run(&c);assert(c.active);
 c.behaviour=1;clock_ms+=1000;y2_charger_run(&c);assert(!c.active);
 c.behaviour=0;clock_ms+=1000;y2_charger_run(&c);assert(c.active);
 /* Faults stay latched through successful samples and manual auto requests. */
 adc_fail=1;clock_ms+=1000;y2_charger_run(&c);assert(!c.active && c.fault);
 adc_fail=0;clock_ms+=1000;y2_charger_run(&c);assert(!c.active);
 regs[0]&=~0x20;clock_ms+=1000;y2_charger_run(&c);assert(!c.fault);
 regs[0]|=0x20;clock_ms+=1000;y2_charger_run(&c);assert(c.active);
 clock_ms+=2500;unsigned pets=c.pets;y2_charger_run(&c);
 assert(!c.active && c.pets==pets && (c.fault&Y2_FAULT_WATCHDOG));
 unsigned faultregs[]={0,0xc,0xe,0x44,0x1e};
 unsigned faultbits[]={0x80,0x40,0x1000,0x100,4};
 for(unsigned i=0;i<5;i++) {
  reset();charger(&c);y2_charger_run(&c);regs[faultregs[i]/2]|=faultbits[i];
  clock_ms+=1000;y2_charger_run(&c);assert(!c.active && c.fault);
 }
 /* A configured 500 mA host keeps the validated 450 mA PMIC profile
  * below 3.2 V; the old 70 mA deep-discharge clamp is forbidden. */
 bat.raw=14000;source_kind=0;supply.budget=500000;
 reset();charger(&c);y2_charger_run(&c);
 assert(c.active && c.charge_ua==Y2_DEEP_DISCHARGE_MAX_UA);
 bat.raw=17000;
 reset();charger(&c);y2_charger_run(&c);persistent=1;
 clock_ms+=1000;y2_charger_run(&c);
 assert(c.stop_error && dev.awake && charger_dormant_blocked && (regs[0x1a/2]&0x10)); /* never disable watchdog on unknown engine */
 persistent=0;clock_ms+=1000;y2_charger_run(&c);assert(!c.active && !dev.awake);
 /* Dedicated source ignores USB data budget, but uses the same 450 mA
  * deep-discharge/precharge ceiling until the normal 650 mA profile is safe. */
 source_kind=4;supply.budget=0;
 int raws[]={14000,15000,17000};unsigned levels[]={450000,450000,650000};
 for(unsigned i=0;i<3;i++) {
  reset();charger(&c);bat.raw=raws[i];y2_charger_run(&c);
  assert(c.active && c.source==Y2_SOURCE_DCP && c.charge_ua==levels[i]);
 }
 bat.raw=17000;source_kind=0;supply.budget=500000;
 /* A busy PHY defers classification and leaves engines off; never guesses DCP. */
 reset();charger(&c);phy_error=-EAGAIN;y2_charger_run(&c);
 assert(!c.active && !c.source_valid && !c.fault && !phy_owned);
 phy_error=0;clock_ms+=1000;y2_charger_run(&c);assert(c.active);
 /* A PHY readback failure latches a SOURCE fault (charging stays inhibited), but a
  * classification error is retried a bounded number of times, 2 s apart, instead of
  * waiting for a cable removal: this was the "second connect never registers" lock-up. */
 reset();charger(&c);phy_error=-EIO;y2_charger_run(&c);
 assert(!c.active && c.fault==Y2_FAULT_SOURCE && !phy_owned);
 phy_error=0;clock_ms+=1000;y2_charger_run(&c);assert(!c.active && c.fault==Y2_FAULT_SOURCE);
 clock_ms+=1500;y2_charger_run(&c);assert(c.active && !c.fault && c.source_valid);
 reset();charger(&c);phy_error=-EIO;y2_charger_run(&c);
 for(unsigned n=0;n<8;n++){clock_ms+=2100;y2_charger_run(&c);}
 assert(!c.active && c.fault==Y2_FAULT_SOURCE && c.source_retries==Y2_SOURCE_RETRIES);
 phy_error=0;clock_ms+=2100;y2_charger_run(&c);assert(!c.active && c.fault==Y2_FAULT_SOURCE);
 regs[0]&=~0x20;clock_ms+=1000;y2_charger_run(&c);assert(!c.fault && !c.source_retries);
 regs[0]|=0x20;clock_ms+=1000;y2_charger_run(&c);assert(c.active);
 /* Removal at every BC settling stage must inhibit and release the PHY. */
 for(unsigned n=1;n<=4;n++) {
  reset();charger(&c);waits=0;removed_at_wait=n;y2_charger_run(&c);
  assert(!c.active && !c.source_valid && !phy_owned && !(regs[0]&0x18));
 }
 removed_at_wait=0;
 /* Full requires active CV confirmations; high voltage at insertion is
  * merely a safe hold. Recharge must wait for sustained hysteresis. */
 reset();charger(&c);bat.raw=19001;y2_charger_run(&c);
 assert(c.cycle.hold && !c.cycle.full && c.status==POWER_SUPPLY_STATUS_NOT_CHARGING);
 reset();charger(&c);bat.raw=17000;y2_charger_run(&c);
 bat.raw=19001; /* 4.175V ADC crossing, CV comparator deliberately absent */
 clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && c.cycle.hold && !c.fault && !c.cycle.full);
 unsigned cutoff_pets=c.pets;
 bat.raw=18600; /* relaxes to 4.087V: no charger feed during confirmation */
 for(unsigned n=0;n<59;n++){clock_ms+=1000;y2_charger_run(&c);assert(!c.active && !c.cycle.full);}
 clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && c.cycle.full && c.status==POWER_SUPPLY_STATUS_FULL && !c.fault && c.pets==cutoff_pets);
 bat.raw=18000; /* 3.955 V: below the maintenance recharge level */
 for(unsigned n=0;n<60;n++){clock_ms+=1000;y2_charger_run(&c);assert(!c.active);}
 clock_ms+=1000;y2_charger_run(&c);
 assert(c.active && !c.cycle.full && !c.cycle.hold && !c.fault);
 /* The former failure sample is still a genuine safety stop if reached:
  * no raised guard, no auto clear at lower voltage, no fabricated Full. */
 bat.raw=19115;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && (c.fault&Y2_FAULT_VOLTAGE) && c.first_uv==4200073);
 bat.raw=18500;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && c.fault && !c.cycle.full);
 /* Capture BOTH distinct hardware detector paths before inhibit changes CHR. */
 for(unsigned which=0;which<2;which++) {
  reset();charger(&c);bat.raw=17000;y2_charger_run(&c);
  regs[which?0xc/2:0]|=which?0x40:0x80;
  clock_ms+=1000;y2_charger_run(&c);
  assert(!c.active && (c.first_fault&Y2_FAULT_OVP));
  assert(which ? (c.first_ov&0x40) : (c.first_con0&0x80));
  regs[0]&=~0x20;clock_ms+=1000;y2_charger_run(&c);assert(c.fault);
 }
 bat.raw=17000;
 reset();charger(&c);c.cycle.total=86400;c.cycle.timed_out=1;
 y2_charger_run(&c);assert(!c.active && (c.fault&Y2_FAULT_TIMEOUT));
}
''')

    def test_usb_allowance_and_pm_entry_use_real_callbacks(self):
        usb=(ROOT/'kernel/platform/usb.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#define READ_ONCE(v) (v)
#define WRITE_ONCE(v,x) ((v)=(x))
#define min(a,b) ((a)<(b)?(a):(b))
#define POWER_SUPPLY_PROP_ONLINE 0
#define POWER_SUPPLY_PROP_CURRENT_MAX 1
enum power_supply_property { unused };
union power_supply_propval {int intval;};
struct usb_phy {int unused;};struct power_supply {int changes;};
static struct power_supply supply,*y2_usb_input=&supply;
static unsigned y2_usb_budget_ma;
static bool y2_usb_finished,y2_usb_detached;
static struct {int result,chrdet;} y2_live={0,0x20};
static void power_supply_changed(struct power_supply *p){p->changes++;}
''' + function(usb,'y2_usb_set_power') + function(usb,'y2_usb_input_get') + r'''
int main(void){
 union power_supply_propval v;
 assert(!y2_usb_input_get(&supply,POWER_SUPPLY_PROP_CURRENT_MAX,&v) && v.intval==0);
 unsigned events[]={8,100,500,2,500,0,100,500};
 for(unsigned i=0;i<8;i++) {
  assert(!y2_usb_set_power(0,events[i]));
  assert(!y2_usb_input_get(&supply,POWER_SUPPLY_PROP_CURRENT_MAX,&v));
  assert(v.intval==(int)events[i]*1000);
 }
 y2_usb_detached=true;
 assert(!y2_usb_input_get(&supply,POWER_SUPPLY_PROP_CURRENT_MAX,&v) && !v.intval);
 y2_usb_detached=false;y2_live.result=-EIO;
 assert(!y2_usb_input_get(&supply,POWER_SUPPLY_PROP_CURRENT_MAX,&v) && !v.intval);
 y2_live.result=0;y2_usb_finished=true;
 assert(!y2_usb_input_get(&supply,POWER_SUPPLY_PROP_CURRENT_MAX,&v) && !v.intval);
 assert(supply.changes==8);
}
''')
        s=(ROOT/'kernel/platform/mt6323-charger.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
struct device {void *data;bool wake;};
struct y2_charger {int lock,work,*wq,irq;bool active,paused,stopping,irq_wake;int stop_error;};
static int locked,cancelled,queued,inhibit_error,wake_error,wake_depth;
static void *dev_get_drvdata(struct device *d){return d->data;}
static void mutex_lock(int *l){assert(!locked);locked=1;}
static void mutex_unlock(int *l){assert(locked);locked=0;}
static int y2_charger_inhibit(struct y2_charger *c){assert(locked && !c->active);return inhibit_error;}
static void cancel_delayed_work_sync(int *w){assert(!locked);cancelled++;}
static void mod_delayed_work(int *q,int *w,int delay){assert(locked && !delay);queued++;}
static bool device_may_wakeup(struct device *d){return d->wake;}
static int enable_irq_wake(int irq){assert(!wake_depth);if(wake_error)return wake_error;wake_depth++;return 0;}
static int disable_irq_wake(int irq){assert(wake_depth);if(wake_error)return wake_error;wake_depth--;return 0;}
''' + ''.join(function(s,n) for n in ('y2_charger_prepare_pm','y2_charger_complete_pm',
                                      'y2_charger_suspend','y2_charger_resume')) + r'''
int main(void){
 struct y2_charger c={0};struct device d={&c,true};
 c.active=true;assert(y2_charger_prepare_pm(&d)==-EBUSY && !cancelled && !c.paused);
 c.active=false;c.stop_error=-EIO;assert(y2_charger_prepare_pm(&d)==-EBUSY && !cancelled);
 c.stop_error=0;inhibit_error=-EIO;
 assert(y2_charger_prepare_pm(&d)==-EIO && !c.paused && !cancelled);
 inhibit_error=0;assert(!y2_charger_prepare_pm(&d) && c.paused && cancelled==1);
 wake_error=-EIO;assert(y2_charger_suspend(&d)==-EIO && !c.irq_wake);
 wake_error=0;assert(!y2_charger_suspend(&d) && c.irq_wake && wake_depth==1);
 assert(!y2_charger_resume(&d) && !c.irq_wake && !wake_depth);
 y2_charger_complete_pm(&d);assert(!c.paused && queued==1 && !locked);
 c.stopping=true;y2_charger_complete_pm(&d);assert(queued==1);
}
''')


class BootOcv(unittest.TestCase):
    def test_power_on_ocv_matches_stock_get_hw_ocv(self):
        # 28, ledger F30: live 27 boot 5919b4fe read 0x724 = 0xc813 -> 4062 mV.
        run_c(r'''
#include <assert.h>
#include "power-math.h"
int main(void){
 assert(y2_hw_ocv_uv(0xc813)==4062000);
 assert(y2_hw_ocv_uv(0x4813)==0);          /* bit 15: not latched */
 assert(y2_hw_ocv_uv(0x8000)==0);          /* 8 mV: implausible */
 assert(y2_hw_ocv_uv(0xffff)==0);          /* 7208 mV: implausible */
 return 0;
}
''')
        driver = (Path(__file__).resolve().parents[1] / 'kernel/platform/mt6323-charger.c').read_text()
        self.assertIn('regmap_read(c->map, 0x724, &ocv)', driver)
        self.assertIn('POWER_SUPPLY_PROP_VOLTAGE_OCV', driver)
