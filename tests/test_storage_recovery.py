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
struct mmc_host;
struct msdc_host {struct delayed_work y2_clock_recovery;struct mmc_host *mmc;int y2_removing,y2_recovery_abort,y2_clock_error;unsigned y2_clock_limit,y2_clock_fallbacks;void *dev;};
struct mmc_host{struct{unsigned timing,clock;}ios;unsigned actual_clock;};
static int claimed,claim_abort,pm_error,clock_error,programs,gets,puts;
static struct mmc_host *mmc_from_priv(struct msdc_host *h){return h->mmc;}
static int __mmc_claim_host(struct mmc_host *m,void *c,int *abort){if(*abort || claim_abort)return 1;assert(!claimed);claimed=1;return 0;}
static void mmc_release_host(struct mmc_host *m){assert(claimed);claimed=0;}
static int pm_runtime_resume_and_get(void *d){gets++;return pm_error;}
static void pm_runtime_mark_last_busy(void *d){}
static void pm_runtime_put_autosuspend(void *d){puts++;}
static int msdc_set_mclk(struct msdc_host *h,unsigned timing,unsigned clock){assert(claimed);programs++;return clock_error;}
#define dev_warn(...) do{}while(0)
''' + function(s, 'y2_msdc_clock_recovery') + r'''
int main(void){
 struct mmc_host mmc={.ios={1,50000000}};struct msdc_host h={.mmc=&mmc,.y2_clock_limit=50000000};
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);
 assert(h.y2_clock_limit==25000000 && h.y2_clock_fallbacks==1 && programs==1 && !claimed && gets==puts);
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);
 assert(h.y2_clock_limit==13000000 && h.y2_clock_fallbacks==2 && programs==2);
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(programs==2);
 h.y2_clock_limit=50000000;clock_error=-5;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(h.y2_clock_error==-5 && !claimed);
 h.y2_recovery_abort=1;int previous=gets;
 y2_msdc_clock_recovery(&h.y2_clock_recovery.work);assert(gets==previous && !claimed);
}
''')
