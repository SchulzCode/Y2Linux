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
#define loff_t off_t
#define strscpy(a,b,n) snprintf(a,n,"%s",b)
static unsigned long jiffies;
#define __user
#define PM_QOS_DEFAULT_VALUE -1
struct freq_qos_request{int value;};
struct pm_qos_request{int value;};
struct delayed_work{unsigned delay;};
struct y2_workload{char name[32];unsigned long deadline;struct freq_qos_request minimum;struct pm_qos_request latency;struct delayed_work expiry;int lock;};
struct file{void *private_data;};
static int system_wq,fail;
static int y2_system_idle_restore(void){return 0;}
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
 assert(y2_workload_write(&file,"ArtworkDecode 1000",18,0)==18 && hint.minimum.value==747500);
 assert(y2_workload_write(&file,"PlaybackNormal 1000",19,0)==19 && !hint.minimum.value);
 assert(y2_workload_write(&file,"LibraryScan 30000",17,0)==17 && hint.minimum.value==747500);
 assert(y2_workload_write(&file,"PlaybackHeavy 3000\n",19,0)==19 && hint.minimum.value==747500);
 assert(y2_workload_write(&file,"NetworkTransfer 3000\n",21,0)==21 && hint.minimum.value==598000);
 assert(y2_workload_write(&file,"Maintenance 3000\n",17,0)==17 && hint.minimum.value==598000);
 fail=1;assert(y2_workload_write(&file,"Idle 100",8,0)==-EIO && hint.minimum.value==598000);
 assert(!hint.lock);
}
''')

    def test_close_or_process_exit_releases_only_the_owned_lease(self):
        source = (Path(__file__).resolve().parents[1] / 'kernel/platform/workload.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
struct request{bool active;};struct node{bool member;};
struct y2_workload{struct node node;bool expiry,latency,freed;struct request minimum,maximum;int *policy;};
struct inode{int unused;};struct file{void *private_data;};static int leases_lock;
static struct y2_workload *closing;
static void mutex_lock(int *l){assert(!*l);*l=1;}
static void mutex_unlock(int *l){assert(*l);*l=0;}
static void list_del(struct node *n){assert(leases_lock && n->member);n->member=false;}
static void cancel_delayed_work_sync(bool *work){assert(!closing->node.member && !leases_lock);*work=false;}
static void cpu_latency_qos_remove_request(bool *p){assert(!closing->expiry);*p=false;}
static void freq_qos_remove_request(struct request *p){assert(p->active);p->active=false;}
static void cpufreq_cpu_put(int *p){(*p)--;}
static void kfree(struct y2_workload *p){p->freed=true;}
''' + function(source,'y2_workload_release') + r'''
int main(void){
 int users=2;struct y2_workload a={{true},true,true,false,{true},{true},&users},b=a;
 struct file f={&a};struct inode inode={0};closing=&a;
 assert(!y2_workload_release(&inode,&f));
 assert(a.freed && !a.minimum.active && !a.maximum.active && !a.latency && !a.expiry && users==1);
 assert(b.node.member && b.minimum.active && b.maximum.active && b.latency && b.expiry);
 f.private_data=&b;closing=&b;assert(!y2_workload_release(&inode,&f) && users==0 && b.freed);
}
''')
