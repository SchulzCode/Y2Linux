"""CPU context ownership: a masked CIRQ snapshot must not replace core state."""
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class CpuPmOwnership(unittest.TestCase):
    def test_real_platform_entry_defers_irq_replay_until_core_restores_context(self):
        source = (ROOT / 'kernel/platform/spm.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
typedef unsigned suspend_state_t;
typedef uint64_t u64;
#define PM_SUSPEND_MEM 3
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define __pa_symbol(x) ((uintptr_t)(x))
#define dsb(x) ((void)0)
#define pr_info(...) ((void)0)
#define pr_warn(...) ((void)0)
#define raw_spin_lock_irqsave(p,f) do { assert(!*(p)); *(p)=1; (f)=0; } while(0)
#define raw_spin_unlock_irqrestore(p,f) do { assert(*(p)); *(p)=0; (void)(f); } while(0)
enum { Y2_PM_TIMER_RESTORED, Y2_PM_CIRQ_REPLAYED, Y2_PM_CIRQ_CLONED,
 Y2_PM_PCM_INSTALLED, Y2_PM_WAKE_MASK_PROGRAMMED, Y2_PM_CPU_CONTEXT_SAVING,
 Y2_PM_AFTER_SPM_RETURN, Y2_PM_CPU_CONTEXT_RESTORED, Y2_PM_NORMAL_PCM_RESTORED,
 Y2_PM_ABORTED };
enum { SPM_PCM_REG9_DATA, SPM_PCM_TIMER_OUT, SPM_PCM_REG_DATA_INI,
 SPM_PCM_EVENT_REG_STA, SPM_PCM_REG13_DATA, SPM_SLEEP_ISR_STATUS, SPM_SLEEP_CPU_WAKEUP_EVENT };
static bool suspend_cirq_pending, spm_broken, cirq_active, core_restored;
static bool timer_ok=true, cirq_ok=true;
static unsigned entries, resumes, aborts, suspend_restore_failures;
static unsigned last_wake,last_ticks,last_debug,last_event,last_r13,last_sleep_isr,last_cpu_wake;
static u64 resume_restore_ns;
static int last_result, spm_lock, vector_error, cirq_error, arm_error, return_error;
static int restore_error, journal_error;
static unsigned live_mask, saved_mask, cirq_mask, flushes, restores;
static unsigned biu_storage, *spm_biu=&biu_storage;
static char y2_cpu_resume[64];
static void *spm_base;
static unsigned pcm_address=0x81000000, normal_address=0x81001000;
static struct { int unused; } spm_io;
static unsigned num_online_cpus(void) { return 1; }
static unsigned smp_processor_id(void) { return 0; }
static void __cpuc_flush_dcache_area(void *p, unsigned n) { assert(n==64); }
static int y2_ccf_boot_vector(uintptr_t p) { return vector_error; }
static int y2_cirq_begin(void) {
 if(cirq_error) return cirq_error;
 assert(!cirq_active); cirq_active=true; cirq_mask=live_mask; live_mask=0x200000;
 return 0;
}
static void y2_cirq_end(void) {
 assert(core_restored && cirq_active); cirq_active=false; live_mask=cirq_mask; flushes++;
}
static bool y2_cirq_restore_ok(void) { return cirq_ok; }
static bool y2_local_timer_context_ok(void) { return timer_ok; }
static void y2_pm_note(const char *name,int error) { assert(error); }
static void y2_pm_mark(unsigned stage,int error) { if(error)journal_error=error; }
static void y2_spm_journal_snapshot(void) { }
static void y2_pm_backstop_pause(bool pause) { assert(!pause); }
static unsigned readl(unsigned *p) { return *p; }
static void writel(unsigned v,unsigned *p) { *p=v; }
static unsigned spm_read(void *p,unsigned reg) { return 0; }
static u64 ktime_get_mono_fast_ns(void) { static u64 n; return ++n; }
static bool suspend_infra_pdn; static unsigned infra_pdn_entries, infra_pdn_restore_failures;
static int infra_save=-ENODEV, infra_restore, arm_infra;
static unsigned infra_saves, infra_restores;
static int y2_infra_pdn_save(void) { ++infra_saves; return infra_save; }
static int y2_infra_pdn_restore(void) { ++infra_restores; return infra_restore; }
static int y2_spm_suspend_arm(void *io,unsigned address,unsigned infra) { arm_infra=infra; assert(infra_saves>=1 || !infra); return arm_error; }
static int y2_spm_suspend_restore(void *io,unsigned address) { restores++; return restore_error; }
static int y2_spm_finish(unsigned long arg) { return 0; }
/* POWER-FINAL-13: VPROC sleep voltage armed only with CIRQ active and always
 * restored before the normal PCM; the golden capture is read-only. */
static int vproc_armed;
static void y2_golden_capture(void) { }
/* F47: UART-busy dump/retry (y2_spm_uart_busy) is exercised in test_sleep_relight. */
static unsigned uart_busy_recoveries;
static bool y2_spm_uart_busy(void){return false;}
static void y2_golden_entry_apply(void){}
static void y2_golden_entry_restore(void){}
static void y2_pmic_golden_capture(void){}
static unsigned wrap_gated;
static void y2_pmic_vproc_sleep_begin(void) { assert(cirq_active); vproc_armed++; }
static void y2_pmic_vproc_sleep_end(void) { assert(!wrap_gated); vproc_armed = 0; }
static unsigned pmic_armed;
static void y2_pmic_sleep_begin(void) { assert(cirq_active && vproc_armed && !wrap_gated); pmic_armed++; }
static void y2_pmic_sleep_end(void) { assert(vproc_armed && !wrap_gated); pmic_armed = 0; }
static void y2_pwrap_clock_gate(void) { assert(pmic_armed); wrap_gated = 1; }
static void y2_pwrap_clock_ungate(void) { wrap_gated = 0; }
static int cpu_suspend(unsigned long arg,int (*fn)(unsigned long)) {
 assert(cirq_active && live_mask==0x200000); live_mask=0; return return_error;
}
''' + function(source, 'y2_spm_resume_syscore') + function(source, 'y2_spm_enter') + r'''
static void reset(void) {
 assert(!spm_lock && !suspend_cirq_pending && !cirq_active);
 spm_broken=false; core_restored=false; timer_ok=cirq_ok=true;
 vector_error=cirq_error=arm_error=return_error=restore_error=journal_error=0;
 live_mask=saved_mask=0x713421; flushes=restores=0;
 infra_save=-ENODEV; infra_restore=0; arm_infra=0;
 infra_saves=infra_restores=0; suspend_infra_pdn=false;
}
static void core_resume(void) {
 assert(!vproc_armed);
 live_mask=saved_mask; core_restored=true; y2_spm_resume_syscore((void *)0);
 assert(!suspend_cirq_pending && !cirq_active && live_mask==saved_mask);
}
int main(void) {
 reset(); assert(!y2_spm_enter(PM_SUSPEND_MEM));
 assert(suspend_cirq_pending && cirq_active && !flushes && !live_mask);
 core_resume(); assert(flushes==1 && !journal_error);
 y2_spm_resume_syscore((void *)0); assert(flushes==1);
 /* Every entry-abort path still balances a successful CIRQ begin. */
 reset(); arm_error=-EBUSY; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 assert(suspend_cirq_pending); core_resume(); assert(flushes==1);
 reset(); return_error=-EBUSY; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 core_resume(); assert(flushes==1);
 reset(); vector_error=-EIO; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EIO);
 core_resume(); assert(!flushes);
 reset(); cirq_error=-EAGAIN; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EAGAIN);
 core_resume(); assert(!flushes);
 reset(); assert(!y2_spm_enter(PM_SUSPEND_MEM)); timer_ok=false;
 core_resume(); assert(spm_broken && last_result==-EIO && journal_error==-EIO);
 reset(); assert(!y2_spm_enter(PM_SUSPEND_MEM)); cirq_ok=false;
 core_resume(); assert(spm_broken && last_result==-EIO && journal_error==-EIO);
 /* Retained mode (no INFRA save available): no restore or observation runs. */
 reset(); assert(!y2_spm_enter(PM_SUSPEND_MEM)); core_resume();
 assert(!arm_infra && infra_saves==1 && !infra_restores && !suspend_infra_pdn);
 /* INFRA power-down: saved before arming, restored right after the CPU
  * returns. */
 reset(); infra_save=0; assert(!y2_spm_enter(PM_SUSPEND_MEM)); core_resume();
 assert(arm_infra==1 && infra_saves==1 && infra_restores==1);
 assert(!suspend_infra_pdn && !spm_broken && infra_pdn_entries==1);
 /* A refused arm never leaves a pending restore or observation behind. */
 reset(); infra_save=0; arm_error=-EBUSY; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 assert(!infra_restores && !suspend_infra_pdn); core_resume();
 /* An abort after a power-down request still verifies INFRA state. */
 reset(); infra_save=0; return_error=-EBUSY; assert(y2_spm_enter(PM_SUSPEND_MEM)==-EBUSY);
 assert(infra_restores==1 && !spm_broken); core_resume();
 /* A restore the hardware did not take is a hard fault for this boot. */
 reset(); infra_save=0; infra_restore=-EIO; suspend_restore_failures=infra_pdn_restore_failures=0;
 assert(y2_spm_enter(PM_SUSPEND_MEM)==-EIO);
 assert(spm_broken && infra_pdn_restore_failures==1 && suspend_restore_failures==1 && last_result==-EIO);
 core_resume(); assert(flushes==1);
}
''')

    def test_matching_diagnostic_preserves_outer_context_and_failure_balance(self):
        source = (ROOT / 'tools/development/y2-pm-stack-probe.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#define NOTIFY_OK 1
#define NOTIFY_STOP 0x8001
#define NOTIFY_BAD 0x8002
''' + function(source, 'boundary') + r'''
int main(void) {
 unsigned depth=0, saved=0, live=0x713421, original=live;
 /* Model the real order: core save, CIRQ mask, platform duplicate save,
  * platform restore, CIRQ replay, core restore. Only one owner may save. */
 if (boundary(&depth, true, true)==NOTIFY_OK) saved=live;
 live=0x200000;
 assert(boundary(&depth, true, true)==NOTIFY_STOP);
 assert(depth==2 && saved==original);
 assert(boundary(&depth, false, true)==NOTIFY_STOP);
 live=original; /* CIRQ replay */
 assert(boundary(&depth, false, true)==NOTIFY_OK);
 live=saved;
 assert(live==original && depth==0);
 /* Read-only mode observes the defect without altering notifier delivery. */
 assert(boundary(&depth, true, false)==NOTIFY_OK);
 assert(boundary(&depth, true, false)==NOTIFY_OK);
 assert(boundary(&depth, false, false)==NOTIFY_OK);
 assert(boundary(&depth, false, false)==NOTIFY_OK && depth==0);
 /* The robust chain unwinds the outer ENTER if another notifier refuses. */
 assert(boundary(&depth, true, true)==NOTIFY_OK);
 assert(boundary(&depth, false, true)==NOTIFY_OK && depth==0);
 assert(boundary(&depth, false, true)==NOTIFY_BAD && depth==0);
 assert(boundary(&depth, true, true)==NOTIFY_OK);
 assert(boundary(&depth, true, true)==NOTIFY_STOP);
 assert(boundary(&depth, true, true)==NOTIFY_BAD && depth==2);
}
''')


if __name__ == '__main__':
    unittest.main()
