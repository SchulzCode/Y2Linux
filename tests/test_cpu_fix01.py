"""Focused correction regressions. Synthetic evidence, never a device receipt."""
from pathlib import Path
import json
import unittest
from test_audio import function
from test_power import run_c
from tools.build.run import prepare_overlay
ROOT = Path(__file__).resolve().parents[1]

def overlay(path):
    spec = next(x for x in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays'] if x['path']==path)
    return prepare_overlay(ROOT,spec).read_text()

class CpuFix01(unittest.TestCase):
    def test_inherited_counter_predicates_and_rate(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "timer-policy.h"
int main(void){
 assert(!y2_gpt6_predicate(13000000,1U<<16,0,0));
 assert(!y2_gpt6_predicate(13000000,1U<<16,0x31,0));
 assert(y2_gpt6_adopt(0x31,0) && y2_gpt6_adopt(0x33,0));
 assert(!y2_gpt6_adopt(0x11,0) && !y2_gpt6_adopt(0x31,1));
 assert(!strcmp(y2_gpt6_predicate(26000000,1U<<16,0,0),"gpt2_rate_not_13mhz"));
 assert(!strcmp(y2_gpt6_predicate(13000000,0,0,0),"generic_timer_feature_absent"));
 assert(y2_gpt6_predicate(13000000,1U<<16,0x80,0));
 assert(y2_gpt6_predicate(13000000,1U<<16,0,0x80));
 assert(y2_gpt_rate_matches(26000,26000));
 assert(!y2_gpt_rate_matches(26000,13000) && !y2_gpt_rate_matches(26000,52000));
 assert(!y2_gpt_rate_matches(25999,25999));
}
''')

    def test_gpt4_reclaim_and_gpt1_fallback(self):
        s=(ROOT/'kernel/platform/local-timer.c').read_text().replace('bool __init y2_local_timer_broadcast_prepare(', 'static bool y2_local_timer_broadcast_prepare(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <string.h>
typedef unsigned u32;
#define __init
#define __iomem
#define BIT(n) (1U<<(n))
#define pr_info(...) ((void)0)
#define pr_warn(...) ((void)0)
#define strscpy(d,s,n) snprintf(d,n,"%s",s)
#include <stdio.h>
static char broadcast_admission[64];
static unsigned regs[32],fail;
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){if(fail && p==(void *)&regs[0x40/4] && !v)return;*(unsigned *)p=v;}
''' + function(s,'y2_local_timer_broadcast_prepare') + r'''
int main(void){
 regs[0]=0x29;regs[0x10/4]=0x11;regs[0x40/4]=0x33;regs[0x44/4]=0x10;
 assert(y2_local_timer_broadcast_prepare(regs));
 assert(!(regs[0]&1) && regs[0x10/4]==0x10 && !regs[0x40/4] && !regs[0x44/4]);
 assert(regs[0]&0x20); /* GPT6 remains masked/enabled exactly as inherited caller set it. */
 regs[0]=1;regs[0x10/4]=0x11;regs[0x40/4]=0x80;
 assert(!y2_local_timer_broadcast_prepare(regs) && regs[0]==1 && regs[0x10/4]==0x11);
 regs[0x40/4]=0x31;regs[0x44/4]=1;regs[0x4c/4]=123;fail=1;
 assert(!y2_local_timer_broadcast_prepare(regs));
 assert(regs[0]==1 && regs[0x10/4]==0x11 && regs[0x40/4]==0x31 && regs[0x4c/4]==123);
}
''')

    def test_charger_prepare_refusal_balances_real_gfp_unwind(self):
        s=overlay('drivers/base/power/main.c').replace('int dpm_suspend_start(', 'static int dpm_suspend_start(').replace('void dpm_resume_end(', 'static void dpm_resume_end(')
        run_c(r'''
#include <assert.h>
#include <errno.h>
typedef int pm_message_t;typedef int ktime_t;
#define SUSPEND_PREPARE 1
static int refusal,depth,prepared,suspended,resumed,completed;
static int ktime_get(void){return 0;}
static int dpm_prepare(int state){assert(!depth);prepared++;return refusal;}
static int dpm_suspend(int state){assert(depth==1);suspended++;return 0;}
static void pm_restrict_gfp_mask(void){assert(!depth);depth++;}
static void pm_restore_gfp_mask(void){assert(depth==1);depth--;}
static void dpm_save_failed_step(int state){}
static void dpm_show_time(int a,int b,int c,const char *d){}
static void dpm_resume(int state){assert(depth==1);resumed++;}
static void dpm_complete(int state){assert(!depth);completed++;}
''' + function(s,'dpm_suspend_start') + function(s,'dpm_resume_end') + r'''
int main(void){
 refusal=-EBUSY;assert(dpm_suspend_start(1)==-EBUSY && !suspended);
 dpm_resume_end(1);assert(!depth && resumed==1 && completed==1);
 refusal=0;assert(!dpm_suspend_start(1) && suspended==1);
 dpm_resume_end(1);assert(!depth && prepared==2 && completed==2);
}
''')

    def test_reset_journal_torn_writes_never_replace_last_valid_stage(self):
        s=(ROOT/'kernel/platform/pm-journal.c').read_text()
        run_c(r'''
#include <assert.h>
#include <string.h>
#include <setjmp.h>
#include "pm-journal-policy.h"
static unsigned sram[65],journal_record[Y2_PM_WORDS],slot,writes,cut;
static void *journal=sram;static jmp_buf reset;
static void writel(unsigned v,void *p){if(++writes==cut)longjmp(reset,1);*(unsigned *)p=v;}
static unsigned readl(void *p){return *(unsigned *)p;}
#define wmb() ((void)0)
''' + function(s,'commit') + r'''
int main(void){
 for(unsigned n=1;n<31;n++){
  memset(sram,0,sizeof(sram));memset(journal_record,0,sizeof(journal_record));slot=0;cut=0;writes=0;
  journal_record[2]=10;commit();assert(y2_pm_valid(sram+32));
  writes=0;cut=n;journal_record[2]=11;
  if(!setjmp(reset))commit();
  unsigned *old=sram+32,*new=sram;
  assert(y2_pm_valid(old) && old[2]==10);
  if(y2_pm_valid(new))assert(new[2]==11 && y2_pm_newer(new[1],old[1]));
 }
 assert(y2_pm_newer(1,~0U));
}
''')

    def test_slow_coordinator_requirements_and_manual_topology_preserved(self):
        s=(ROOT/'kernel/platform/system-idle.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include "system-idle-policy.h"
#define BIT(n) (1U<<(n))
#define msecs_to_jiffies(n) (n)
static unsigned parked_mask,restore_count,quiet_since,hold_until,jiffies,online=1,refuse;
static int last_error;static bool broken;
static int cpu_online(unsigned c){return online & BIT(c);}
static int add_cpu(unsigned c){if(refuse==c)return -EIO;online|=BIT(c);return 0;}
''' + function(s,'restore_locked') + r'''
int main(void){
 assert(!y2_system_quiet(29999,0,1,0,1,598000));
 assert(y2_system_quiet(30000,10,1,0,1,598000));
 assert(!y2_system_quiet(90000,11,1,0,1,598000));
 assert(!y2_system_quiet(90000,0,0,0,1,598000));
 assert(!y2_system_quiet(90000,0,1,1,1,598000));
 assert(!y2_system_quiet(90000,0,1,0,0,598000));
 assert(!y2_system_quiet(90000,0,1,0,1,1040000));
 parked_mask=BIT(2)|BIT(3);jiffies=20;
 assert(!restore_locked() && online==13 && !parked_mask && hold_until==60020);
 /* CPU1 was manually offline and must remain so. */
 parked_mask=BIT(2);online=1;refuse=2;
 assert(restore_locked()==-EIO && broken && parked_mask==BIT(2));
}
''')

    def test_unused_clock_engines_and_real_decoded_owners(self):
        import re
        source=(ROOT/'kernel/platform/clocks.c').read_text()
        owners=re.search(r'static const char \*const slow_owners\[24\] = \{.*?\n\};',source,re.S).group()
        run_c(r'''
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "idle-clock-policy.h"
#define READ_ONCE(x) (x)
#define ARRAY_SIZE(x) (sizeof(x)/sizeof(*(x)))
#define BIT(n) (1U<<(n))
#define PAGE_SIZE 4096
#define scnprintf snprintf
struct kernel_param{int unused;};
static unsigned slow_blockers;
''' + owners + function(source,'slow_names_get') + r'''
int main(void){
 char names[4096];struct kernel_param parameter={0};
 slow_blockers=0xe07800;slow_names_get(names,&parameter);
 for(unsigned bit=0;bit<24;bit++)if(slow_blockers&BIT(bit)){
  char expected[16];snprintf(expected,sizeof expected,"bit%u=",bit);assert(strstr(names,expected));
 }
 assert(strstr(names,"APDMA:") && strstr(names,"MSDC0:eMMC") && strstr(names,"MSDC1:SD"));
 assert(strstr(names,"MSDC2:unused") && strstr(names,"I2C0:wheel") && strstr(names,"I2C1:DAC") && strstr(names,"I2C2:unused"));
 assert(!strstr(names,"BTIF:"));
 assert(!y2_unused_clock_busy(1,0,0) && !y2_unused_clock_busy(0,0,0));
 for(unsigned bit=0;bit<2;bit++)assert(y2_unused_clock_busy(1,BIT(bit),0));
 assert(y2_unused_clock_busy(1,0,1) && y2_unused_clock_busy(0,1,0) && y2_unused_clock_busy(0,0,1));
 assert(!y2_unused_clock_busy(0,2,2));
}
''')

    def test_arch_registration_rollback_and_normal_linux_event_selection(self):
        s=overlay('drivers/clocksource/arm_arch_timer.c')
        register=function(s,'arch_timer_register')
        self.assertLess(register.index('cpuhp_setup_state'),register.index('y2_local_timer_registration(0)'))
        self.assertIn('y2_local_timer_registration(err)',register)
        self.assertIn('free_percpu(arch_timer_evt)',register)
        setup=function(s,'__arch_timer_setup')
        self.assertIn('CLOCK_EVT_FEAT_ONESHOT',setup)
        self.assertIn('clockevents_config_and_register',setup)
        self.assertNotIn('CLOCK_EVT_FEAT_DUMMY',setup)
        self.assertIn('frequency != 13000000',function(s,'arch_timer_of_init'))
        self.assertIn('y2_local_timer_cpu_init',function(s,'arch_timer_starting_cpu'))

    def test_durable_pre_entry_receipt_and_nohz_highres_mode(self):
        import sys,tempfile
        sys.path.insert(0,str(ROOT/'tools/platform'))
        from y2_platform.common import Context
        from y2_platform.suspend_record import record
        from y2_platform.observe import timer_runtime
        with tempfile.TemporaryDirectory() as root:
            ctx=Context(root)
            usb=ctx.path('/sys/bus/platform/drivers/y2-usb/11200000.usb/status')
            usb.parent.mkdir(parents=True);usb.write_text('pm_suspends=1 pm_restores=1\n')
            value=record(ctx,'quiescing_radios',0)
            self.assertIn('pm_restores=1',value['usb_restore']['controllers']['11200000.usb'])
            record(ctx,'kernel_suspend',0)
            self.assertEqual(ctx.json('/data/system/platform/suspend-last.json')['stage'],'kernel_suspend')
            self.assertEqual(value['requested_mode'],'deep')
            timer=ctx.path('/proc/timer_list');timer.parent.mkdir(parents=True,exist_ok=True)
            timer.write_text('cpu: 0\n .hres_active : 1\n .nohz : 2\n')
            result=timer_runtime(ctx)
        self.assertTrue(result['highres_active'])
        self.assertTrue(result['no_hz_active'])
