"""Full-suspend checked unwind and observer proof. Host fixtures, no device I/O."""
import importlib.util
import fcntl
import tempfile
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c
from test_cpu_fix02 import JOURNAL_FIXTURE
from test_cpu_fix03 import DEVICE
import test_cpu_fix03

ROOT = Path(__file__).resolve().parents[1]


class FullSuspend(unittest.TestCase):
    def test_normal_pcm_restore_rejects_every_dropped_contract_write(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "spm-policy.h"
#include "spm-regs.h"
#include "spm-suspend-policy.h"
static unsigned regs[1024], corrupt;
static unsigned rd(void *p,unsigned r){ return regs[r/4] ^ (r==corrupt ? 1U : 0); }
static void wr(void *p,unsigned r,unsigned v){ regs[r/4]=v; }
static void wait(unsigned n){}
int main(void){
 struct y2_spm_io io={0,rd,wr,wait};
 corrupt=~0U;assert(!y2_spm_suspend_restore(&io,0x81000000));
 for(unsigned n=0;n<2;n++){
  corrupt=n?SPM_PCM_IM_LEN:SPM_PCM_IM_PTR;
  assert(y2_spm_suspend_restore(&io,0x81000000)==-EIO);
 }
 corrupt=SPM_PCM_PWR_IO_EN;assert(y2_spm_suspend_restore(&io,0x81000000)==-EIO);
}
''')

    def test_actual_platform_entry_balances_abort_and_checks_all_restores(self):
        source = (ROOT/'kernel/platform/spm.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include <stdint.h>
enum { Y2_PM_CIRQ_CLONED, Y2_PM_PCM_INSTALLED, Y2_PM_WAKE_MASK_PROGRAMMED,
Y2_PM_CPU_CONTEXT_SAVING, Y2_PM_AFTER_SPM_RETURN, Y2_PM_CPU_CONTEXT_RESTORED,
Y2_PM_NORMAL_PCM_RESTORED, Y2_PM_TIMER_RESTORED, Y2_PM_CIRQ_REPLAYED, Y2_PM_ABORTED };
static void y2_pm_mark(unsigned s,int e){}
static void y2_pm_note(const char *s,int e){}
static void y2_pm_backstop_pause(bool pause){}
#include "spm-policy.h"
#include "spm-regs.h"
#define BIT(n) (1U<<(n))
#define PM_SUSPEND_MEM 3
#define WRITE_ONCE(v,x) ((v)=(x))
#define READ_ONCE(v) (v)
#define dsb(x) do{}while(0)
#define __pa_symbol(x) 0x80400000UL
#define raw_spin_lock_irqsave(lock,f) do{assert(!*(lock));*(lock)=1;f=0;}while(0)
#define raw_spin_unlock_irqrestore(lock,f) do{assert(*(lock));*(lock)=0;(void)f;}while(0)
#define pr_info(...) do{}while(0)
#define pr_warn(...) do{}while(0)
typedef int suspend_state_t;typedef uint64_t u64;
static unsigned biureg=0x12345678,spmregs[1024];
static void *spm_base=spmregs,*spm_biu=&biureg;
static int spm_lock;static bool spm_broken,suspend_cirq_pending;
static unsigned entries,resumes,aborts,last_wake,last_ticks,last_debug,last_event,last_r13,suspend_restore_failures,last_sleep_isr,last_cpu_wake;
static int last_result;static u64 resume_restore_ns;
static unsigned pcm_address=0x81000000,normal_address=0x81001000;
static struct y2_spm_io spm_io;
static unsigned cirq_enter,cirq_exit,normal_restores,sleeps;
static int fail_cirq,fail_arm,fail_sleep,fail_restore;
static bool bad_timer,bad_cirq,bad_biu;
static unsigned num_online_cpus(void){return 1;}
static unsigned smp_processor_id(void){return 0;}
static void y2_cpu_resume(void){}
static void __cpuc_flush_dcache_area(void *p,unsigned n){}
static int y2_ccf_boot_vector(unsigned long address){return 0;}
static int y2_cirq_begin(void){cirq_enter++;return fail_cirq;}
static void y2_cirq_end(void){cirq_exit++;}
static bool y2_cirq_restore_ok(void){return !bad_cirq;}
static unsigned readl(void *p){return *(unsigned*)p;}
static void writel(unsigned v,void *p){if(bad_biu)v^=1;*(unsigned*)p=v;}
static bool suspend_infra_pdn;static unsigned infra_pdn_entries,infra_pdn_restore_failures;
static int y2_infra_pdn_save(void){return -ENODEV;}
static int y2_infra_pdn_restore(void){return 0;}
static int y2_spm_suspend_arm(struct y2_spm_io *io,unsigned addr,unsigned infra){(void)infra;return fail_arm;}
static int y2_spm_finish(unsigned long n){return 0;}
static int cpu_suspend(unsigned long n,int(*fn)(unsigned long)){assert(n==0);sleeps++;return fail_sleep;}
static int y2_spm_suspend_restore(struct y2_spm_io *io,unsigned addr){normal_restores++;return fail_restore;}
static bool y2_local_timer_context_ok(void){return !bad_timer;}
static unsigned spm_read(void *p,unsigned reg){return ((unsigned*)p)[reg/4];}
static void y2_spm_journal_snapshot(void){}
/* POWER-FINAL-13: stock VPROC sleep voltage must be restored on every path
 * that armed it; the golden capture is read-only. */
static unsigned vproc_begins,vproc_ends,pmic_begins,pmic_ends,wrap_gated,wrap_gates;
static void y2_golden_capture(void){}
/* F47: UART-busy dump/retry (y2_spm_uart_busy) is exercised in test_sleep_relight. */
static unsigned uart_busy_recoveries;
static bool y2_spm_uart_busy(void){return false;}
static void y2_golden_entry_apply(void){}
static void y2_golden_entry_restore(void){}
static void y2_pmic_golden_capture(void){assert(!wrap_gated);}
static void y2_pmic_vproc_sleep_begin(void){assert(!wrap_gated);vproc_begins++;}
static void y2_pmic_vproc_sleep_end(void){assert(!wrap_gated);vproc_ends++;}
static void y2_pmic_sleep_begin(void){assert(!wrap_gated);pmic_begins++;}
static void y2_pmic_sleep_end(void){assert(!wrap_gated);pmic_ends++;}
/* 19: the AP PMIC wrapper is unusable while its clock is gated. */
static void y2_pwrap_clock_gate(void){wrap_gated=1;wrap_gates++;}
static void y2_pwrap_clock_ungate(void){wrap_gated=0;}
static u64 ktime_get_mono_fast_ns(void){static u64 n;return ++n;}
static void reset(void){
 assert(!suspend_cirq_pending);
 spm_broken=false;entries=resumes=aborts=suspend_restore_failures=0;
 cirq_enter=cirq_exit=normal_restores=sleeps=0;
 fail_cirq=fail_arm=fail_sleep=fail_restore=last_result=0;
 bad_timer=bad_cirq=bad_biu=false;biureg=0x12345678;
 spmregs[SPM_PCM_REG_DATA_INI/4]=0;
}
''' + function(source, 'y2_spm_enter') + function(source, 'y2_spm_resume_syscore') + r'''
int main(void){
 reset();assert(!y2_spm_enter(PM_SUSPEND_MEM));
 assert(suspend_cirq_pending && !cirq_exit && vproc_begins==1 && vproc_ends==1 && pmic_begins==1 && pmic_ends==1);
 y2_spm_resume_syscore((void *)0);
 assert(entries==1 && resumes==1 && !aborts && !spm_broken && sleeps==1);
 assert(cirq_enter==cirq_exit);
 reset();fail_sleep=-EBUSY;assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 y2_spm_resume_syscore((void *)0);assert(cirq_exit==1);
 assert(!resumes && aborts==1 && !spm_broken && normal_restores>=1 && !spm_lock);
 reset();fail_arm=-EBUSY;assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 y2_spm_resume_syscore((void *)0);assert(!sleeps && cirq_exit==1);
 assert(vproc_begins==vproc_ends); /* every abort after arming restores VPROC */
 assert(pmic_begins==pmic_ends); /* and the 18 PMIC fields */
 assert(!wrap_gated); /* 19: never left gated */
 for(unsigned fault=0;fault<4;fault++){
  reset();bad_biu=fault==0;bad_timer=fault==1;bad_cirq=fault==2;fail_restore=fault==3?-EIO:0;
  int result=y2_spm_enter(PM_SUSPEND_MEM);
  /* Timer/CIRQ checks belong after the core GIC restore. Their error is
   * durable and quarantines SPM even though syscore resume returns void. */
  assert(result==((bad_timer || bad_cirq)?0:-EIO));
  y2_spm_resume_syscore((void *)0);
  assert(last_result==-EIO && resumes==1 && spm_broken && suspend_restore_failures && !spm_lock);
  assert(cirq_exit==1 && !suspend_cirq_pending);
  unsigned prior=sleeps;assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY && sleeps==prior);
 }
}
''')


class ResumeErrors(unittest.TestCase):
    def test_device_resume_failure_survives_a_successful_kernel_exit(self):
        funcs = test_cpu_fix03.ResumeCompletionDiagnostics().functions
        run_c(JOURNAL_FIXTURE + DEVICE + funcs('stage_name', 'commit', 'ring_reset',
            'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark', 'y2_pm_backstop_ping', 'y2_pm_device') + r'''
int main(void){
 struct device d={"11230000.mmc"};
 for(unsigned phase=5;phase<=7;phase++){
  y2_pm_mark(Y2_PM_SUSPEND_REQUEST,0);
  y2_pm_mark(Y2_PM_DEVICES_RESUMING,0);
  y2_pm_device(&d,phase,0,false);
  y2_pm_device(&d,phase,-EIO,true);
  y2_pm_mark(Y2_PM_EXIT,0);
  assert((int)journal_record[4]==-EIO && journal_record[3]==Y2_PM_DEVICES_RESUMING);
  y2_pm_device(&d,phase,-ETIMEDOUT,true);
  assert((int)journal_record[4]==-EIO); /* retain the original failure */
 }
 y2_pm_mark(Y2_PM_SUSPEND_REQUEST,0);
 y2_pm_device(&d,1,-EBUSY,true);
 y2_pm_mark(Y2_PM_DPM_PREPARED,-EBUSY);
 assert(journal_record[3]==Y2_PM_DPM_PREPARED); /* charger refusal stays typed */
}
''')


class Observer(unittest.TestCase):
    def test_inherited_lease_keeps_other_device_owners_excluded(self):
        spec = importlib.util.spec_from_file_location('suspend_observer', ROOT/'tools/development/qualify-platform-v1-suspend.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            module.DEVICE_LOCK = str(Path(directory)/'device.lock')
            with module.device_lock() as coordinator:
                fcntl.flock(coordinator, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with module.device_lock(coordinator.fileno()) as trial:
                    fcntl.flock(trial, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with module.device_lock() as other:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    with open(Path(directory)/'wrong.lock', 'w') as wrong:
                        with self.assertRaises(ValueError):
                            module.device_lock(wrong.fileno())
            with module.device_lock() as after:
                fcntl.flock(after, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_staged_pm_and_reset_are_not_deep_resume(self):
        spec = importlib.util.spec_from_file_location('suspend_observer', ROOT/'tools/development/qualify-platform-v1-suspend.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        files = {'/proc/sys/kernel/random/boot_id':'one','/proc/sys/kernel/tainted':'0',
                 '/sys/power/pm_test':'[none] core', '/proc/interrupts':' 66: 1 0 0 0 mt6323 20 Level mt6397-rtc',
                 '/sys/devices/platform/10006000.power-controller/state':'entries=0 resumes=0 debug=0 broken=0',
                 '/sys/firmware/y2_pm/state':'valid=1 stage=COMPLETE error=0 reset_resume_entry=0x0 pcm_length=0'}
        result = {'mode':'freezer','before':{'files':dict(files)},'after':{'files':dict(files)},'helper':{'code':0}}
        self.assertTrue(module.verdict(result)['pass'])
        self.assertFalse(module.verdict(result)['deep_resume'])
        result.update(mode='none',sleep_proxy_s=10,expected_wake='rtc')
        self.assertFalse(module.verdict(result)['pass'])
        result['after']['files']['/sys/devices/platform/10006000.power-controller/state']='entries=1 resumes=1 debug=0 broken=0'
        result['after']['files']['/sys/firmware/y2_pm/state']='valid=1 stage=COMPLETE error=0 reset_resume_entry=0x59325253 pcm_length=596'
        result['after']['files']['/proc/interrupts']=' 66: 2 0 0 0 mt6323 20 Level mt6397-rtc'
        self.assertTrue(module.verdict(result)['deep_resume'])
        result['after']['files']['/proc/sys/kernel/random/boot_id']='two'
        self.assertFalse(module.verdict(result)['pass'])
