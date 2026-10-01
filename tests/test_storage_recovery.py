from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c
from test_hardware_ceiling import source


class StorageRecovery(unittest.TestCase):
    def test_recovery_is_claimed_between_requests_and_teardown_can_abort_claim(self):
        s = source('drivers/mmc/host/mtk-sd.c')
        run_c(r'''
#include <assert.h>
#include <stddef.h>
#include <stdbool.h>
typedef unsigned u32;
#define READ_ONCE(x) (x)
#define container_of(p,t,m) ((t *)((char *)(p)-offsetof(t,m)))
struct work_struct{int unused;};struct delayed_work{struct work_struct work;};
#define to_delayed_work(p) container_of(p,struct delayed_work,work)
typedef struct{int counter;}atomic_t;
static int atomic_xchg(atomic_t *a,int v){int o=a->counter;a->counter=v;return o;}
enum y2_storage_event{Y2_EVENT_CRC,Y2_EVENT_TIMEOUT,Y2_EVENT_CONTROLLER};
struct mmc_host;
struct msdc_host {struct delayed_work y2_clock_recovery;struct mmc_host *mmc;int y2_removing,y2_recovery_abort,y2_clock_error;
 atomic_t y2_pending_event;unsigned y2_level_strikes;void *dev;};
struct mmc_host{struct{unsigned timing;}ios;};
static int claimed,claim_abort,pm_error,downgrades,gets,puts,last_event,last_reset;
static struct mmc_host *mmc_from_priv(struct msdc_host *h){return h->mmc;}
static int __mmc_claim_host(struct mmc_host *m,void *c,int *abort){if(*abort || claim_abort)return 1;assert(!claimed);claimed=1;return 0;}
static void mmc_release_host(struct mmc_host *m){assert(claimed);claimed=0;}
static int pm_runtime_resume_and_get(void *d){gets++;return pm_error;}
static void pm_runtime_mark_last_busy(void *d){}
static void pm_runtime_put_autosuspend(void *d){puts++;}
static bool y2_tuned_timing(unsigned t){return t==9;}
static void y2_downgrade(struct msdc_host *h,enum y2_storage_event e,bool reset){assert(claimed);downgrades++;last_event=e;last_reset=reset;}
''' + function(s, 'y2_msdc_clock_recovery') + r'''
int main(void){
 struct mmc_host mmc={.ios={1}};struct msdc_host h={.mmc=&mmc};
 /* Nothing pending: no claim, no power reference. */
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(!gets && !downgrades);
 /* An untuned CRC fault steps down under the claim, PM balanced. */
 h.y2_pending_event.counter=Y2_EVENT_CRC+1;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);
 assert(downgrades==1 && last_event==Y2_EVENT_CRC && last_reset && !claimed && gets==1 && puts==1 && !h.y2_pending_event.counter);
 /* A failed clock change blocks further changes until reboot. */
 h.y2_clock_error=-5;h.y2_pending_event.counter=Y2_EVENT_CONTROLLER+1;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(downgrades==1 && !claimed && gets==puts);
 h.y2_clock_error=0;
 /* Runtime resume failure releases the claim without a put. */
 pm_error=-5;h.y2_pending_event.counter=Y2_EVENT_TIMEOUT+1;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(downgrades==1 && !claimed && gets==puts+1);
 pm_error=0;puts++;
 /* Teardown aborts the claim and never takes a power reference. */
 h.y2_recovery_abort=1;h.y2_pending_event.counter=Y2_EVENT_CRC+1;int previous=gets;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(gets==previous && !claimed && downgrades==1);
 h.y2_recovery_abort=0;h.y2_removing=1;h.y2_pending_event.counter=Y2_EVENT_CRC+1;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(gets==previous && h.y2_pending_event.counter);
}
''')
