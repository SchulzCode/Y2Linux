from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c


class SlowIdle(unittest.TestCase):
    def test_actual_clock_owner_preconditions_and_restore(self):
        source = (Path(__file__).resolve().parents[1] / 'kernel/platform/clocks.c').read_text()
        source = source.replace('int y2_ccf_slow_idle(', 'static int y2_ccf_slow_idle(')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#define __iomem
static unsigned top[64],peri[64],online=1,cpu,entries,writes,fail,slow_blockers;
static void *y2_clock_bases[]={top,peri};static int y2_clk_lock;
static unsigned num_online_cpus(void){return online;}
static unsigned smp_processor_id(void){return cpu;}
static int spin_trylock(int *p){if(*p)return 0;*p=1;return 1;}
static void spin_unlock(int *p){assert(*p);*p=0;}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){writes++;if(fail==writes)return;*(unsigned *)p=v;}
#define dsb(x) do{}while(0)
static void cpu_do_idle(void){assert(y2_clk_lock && top[1]==0x8f);entries++;}
''' + function(source, 'y2_ccf_slow_idle') + r'''
int main(void){
 top[1]=15;peri[0x18/4]=~0U;
 assert(!y2_ccf_slow_idle() && entries==1 && top[1]==15 && !y2_clk_lock);
 online=4;assert(y2_ccf_slow_idle()==-EBUSY);online=1;cpu=1;
 assert(y2_ccf_slow_idle()==-EBUSY);cpu=0;
 for(unsigned b=0;b<24;b++)if((0x00f00800|0x00007800)&(1U<<b)){
  peri[0x18/4]=~(1U<<b);assert(y2_ccf_slow_idle()==-EBUSY && top[1]==15);
 }
 peri[0x18/4]=~0U;top[1]=0x1234;
 assert(y2_ccf_slow_idle()==-EBUSY && top[1]==0x1234);
 top[1]=15;writes=0;fail=1;assert(y2_ccf_slow_idle()==-EIO && top[1]==15);
 writes=0;fail=2;assert(y2_ccf_slow_idle()==-EIO && top[1]==0x8f);
}
''')
