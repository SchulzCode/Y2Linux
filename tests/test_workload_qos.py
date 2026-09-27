"""Exercise the real kernel lease parser and update path without device access."""
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c


class WorkloadQos(unittest.TestCase):
    def test_bounds_expiry_and_failed_update(self):
        source = (Path(__file__).resolve().parents[1] / 'kernel/platform/workload.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/types.h>
#define __user
#define PM_QOS_DEFAULT_VALUE -1
struct freq_qos_request{int value;};
struct pm_qos_request{int value;};
struct delayed_work{unsigned delay;};
struct y2_workload{struct freq_qos_request minimum;struct pm_qos_request latency;struct delayed_work expiry;int lock;};
struct file{void *private_data;};
static int system_wq,fail;
static int copy_from_user(void *a,const void *b,unsigned n){memcpy(a,b,n);return 0;}
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){assert(*p);*p=0;}
static int freq_qos_update_request(struct freq_qos_request *p,int v){if(fail)return -EIO;p->value=v;return 0;}
static void cpu_latency_qos_update_request(struct pm_qos_request *p,int v){p->value=v;}
static unsigned msecs_to_jiffies(unsigned ms){return ms;}
static void mod_delayed_work(int q,struct delayed_work *p,unsigned ms){p->delay=ms;}
''' + function(source, 'y2_workload_idle') + function(source, 'y2_workload_write') + r'''
int main(void){
 struct y2_workload hint={0};struct file file={&hint};
 const char *invalid[]={"Interactive 501","Idle 30001","Idle 0","Idle 100 garbage","MHz 1040000","PlaybackHeavy -1"};
 for(unsigned n=0;n<sizeof(invalid)/sizeof(*invalid);n++)
  assert(y2_workload_write(&file,invalid[n],strlen(invalid[n]),0)==-EINVAL);
 assert(y2_workload_write(&file,"Interactive 250\n",16,0)==16);
 assert(hint.minimum.value==747500 && hint.latency.value==1000 && hint.expiry.delay==250);
 y2_workload_idle(&hint);assert(!hint.minimum.value && hint.latency.value==-1);
 assert(y2_workload_write(&file,"PlaybackNormal 1000",19,0)==19 && !hint.minimum.value);
 assert(y2_workload_write(&file,"LibraryScan 30000",17,0)==17 && hint.minimum.value==747500);
 fail=1;assert(y2_workload_write(&file,"Idle 100",8,0)==-EIO && hint.minimum.value==747500);
 assert(!hint.lock);
}
''')
