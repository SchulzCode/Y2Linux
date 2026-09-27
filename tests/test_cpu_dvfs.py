"""Stock-bin DVFS sequencing and fault containment; no hardware operations."""
from pathlib import Path
import json
import unittest
from test_audio import function
from test_power import run_c
from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]


class CpuDvfs(unittest.TestCase):
    def test_cpufreq_probe_defers_before_governor_without_affecting_other_boards(self):
        spec = next(x for x in json.loads((ROOT / 'kernel/patches/manifest.json').read_text())['overlays']
                    if x['path'] == 'drivers/cpufreq/cpufreq-dt.c')
        source = prepare_overlay(ROOT, spec).read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#define IS_ENABLED(x) 1
#define CPUFREQ_HAVE_GOVERNOR_PER_POLICY 1
#define for_each_present_cpu(cpu) for(cpu=0;cpu<4;cpu++)
struct device{int unused;};struct platform_device{struct device dev;};
struct cpufreq_dt_platform_data{int have_governor_per_policy,resume,suspend,get_intermediate,target_intermediate;};
static struct {int flags,resume,suspend,get_intermediate,target_intermediate;} dt_cpufreq_driver;
static int own_board,voltage_result,registered,initialized;
static void *dev_get_platdata(struct device *d){return NULL;}
static int of_machine_is_compatible(const char *s){return own_board;}
static int y2_pmic_cpu_voltage_get(void){return voltage_result;}
static int dev_err_probe(struct device *d,int ret,const char *s){return ret;}
static void dev_err(struct device *d,const char *s,int ret){}
static int dt_cpufreq_early_init(struct device *d,int cpu){initialized++;return 0;}
static int cpufreq_register_driver(void *d){registered++;return 0;}
static void dt_cpufreq_release(void){}
''' + function(source, 'dt_cpufreq_probe') + r'''
int main(void){struct platform_device p={0};
 own_board=1;voltage_result=-517;
 assert(dt_cpufreq_probe(&p)==-517 && !registered && !initialized);
 voltage_result=-EIO;assert(dt_cpufreq_probe(&p)==-EIO && !registered && !initialized);
 voltage_result=72;assert(!dt_cpufreq_probe(&p) && registered==1 && initialized==4);
 own_board=0;voltage_result=-517;registered=initialized=0;
 assert(!dt_cpufreq_probe(&p) && registered==1 && initialized==4);
}
''')

    def test_runtime_admission_default_ceiling_and_failed_qos_rollback(self):
        source = (ROOT / 'kernel/platform/cpu-dvfs.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdlib.h>
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
struct kernel_param{int unused;};struct work_struct{int unused;};
struct freq_qos_request{bool active;};
static struct freq_qos_request ceiling;
static unsigned qualification_max_khz=1040000,hardware_prepares,qos_value;
static bool bin_supported,voltage_fault;
static bool y2_dvfs_disabled(void){return false;}
static bool y2_cpu_safe(void){return false;}
static int qualification_lock,prepare_error,qos_error;
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){assert(*p);*p=0;}
static int kstrtouint(const char *s,unsigned base,unsigned *out){*out=strtoul(s,0,base);return 0;}
static bool freq_qos_request_active(struct freq_qos_request *q){return q->active;}
static int freq_qos_update_request(struct freq_qos_request *q,unsigned value){
 assert(qualification_lock && q->active);if(qos_error)return qos_error;qos_value=value;return 0;
}
static int y2_pmic_cpu_dvfs_prepare(void){hardware_prepares++;return prepare_error;}
''' + function(source, 'qualification_set') + function(source, 'fault_ceiling_work') + r'''
int main(void){
 assert(qualification_set("1300000",0)==-EAGAIN && qualification_max_khz==1040000 && !hardware_prepares);
 ceiling.active=true;
 assert(qualification_set("1300000",0)==-EOPNOTSUPP && !hardware_prepares);
 bin_supported=true;prepare_error=-EIO;
 assert(qualification_set("1300000",0)==-EIO && qualification_max_khz==1040000);
 prepare_error=0;assert(!qualification_set("1196000",0) && qos_value==1196000 && qualification_max_khz==1196000);
 qos_error=-EINVAL;assert(qualification_set("1300000",0)==-EINVAL && qualification_max_khz==1196000);
 qos_error=0;voltage_fault=true;
 assert(qualification_set("1300000",0)==-EOPNOTSUPP);
 fault_ceiling_work(0);assert(qos_value==1040000 && qualification_max_khz==1040000);
 assert(qualification_set("1400000",0)==-EINVAL);
 hardware_prepares=0;assert(!qualification_set("1040000",0) && !hardware_prepares);
}
''')

    def test_actual_wrapper_owns_slots_preserves_sleep_and_checks_voltage(self):
        source = (ROOT / 'kernel/platform/pwrap.c').read_text()
        names = ('y2_pmic_cpu_selector', 'y2_pmic_cpu_voltage_get',
                 'y2_pmic_cpu_dvfs_prepare', 'y2_pmic_cpu_voltage_set')
        for name in names[1:]:
            source = source.replace('int ' + name + '(', 'static int ' + name + '(')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#define EPROBE_DEFER 517
#define BIT(n) (1U<<(n))
static unsigned wrapper[128],pmic[1024],operations,fail_at,writes,requests,faults,settled;
static unsigned corrupt_slot,request_failure,readback_failure;
static int y2_wrap_lock;
static bool cpu_dvfs_prepared;
struct y2_wrap {void *base,*map;};
static struct y2_wrap device={wrapper,pmic},*y2_wrap=&device;
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){assert(*p);*p=0;}
static int regmap_read(void *p,unsigned reg,unsigned *value){
 assert(y2_wrap_lock);if(++operations==fail_at)return -EIO;
 *value=pmic[reg/2];return 0;
}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned value,void *p){
 unsigned offset=(unsigned *)p-wrapper;
 assert(y2_wrap_lock && offset>=0xe4/4 && offset<=0xf8/4);
 writes++;*(unsigned *)p=value^(corrupt_slot?1:0);
}
static int y2_spm_cpu_voltage_request(unsigned slot){
 assert(y2_wrap_lock && slot<=2);requests++;
 if(request_failure)return -ETIMEDOUT;
 assert(wrapper[(0xe4+8*slot)/4]==0x220);
 pmic[0x220/2]=wrapper[(0xe8+8*slot)/4]^(readback_failure?1:0);
 return 0;
}
static void y2_cpu_dvfs_fault(void){faults++;}
static void udelay(unsigned n){assert(n==40);settled++;}
''' + ''.join(function(source, name) for name in names) + r'''
static void reset(void){
 memset(wrapper,0,sizeof wrapper);memset(pmic,0,sizeof pmic);
 wrapper[4/4]=1;wrapper[0x50/4]=0x1ff;
 pmic[0x216/2]=2;pmic[0x220/2]=pmic[0x21e/2]=72;
 for(unsigned r=0x10c/4;r<=0x120/4;r++)wrapper[r]=0xdeadbeef;
 operations=fail_at=writes=requests=faults=settled=0;
 corrupt_slot=request_failure=readback_failure=cpu_dvfs_prepared=0;
}
int main(void){
 reset();assert(y2_pmic_cpu_voltage_get()==72);
 assert(!y2_pmic_cpu_dvfs_prepare() && cpu_dvfs_prepared && requests==1);
 for(unsigned r=0x10c/4;r<=0x120/4;r++)assert(wrapper[r]==0xdeadbeef);
 for(unsigned n=0;n<3;n++){
  unsigned selectors[]={88,80,72};unsigned v=selectors[n];
  assert(!y2_pmic_cpu_voltage_set(v) && y2_pmic_cpu_voltage_get()==(int)v);
 }
 assert(settled==3 && writes==6);
 assert(y2_pmic_cpu_voltage_set(71)==-EINVAL);
 unsigned count;
 reset();assert(!y2_pmic_cpu_dvfs_prepare());count=operations;
 for(unsigned f=1;f<=count;f++){
  reset();fail_at=f;assert(y2_pmic_cpu_dvfs_prepare()<0 && !cpu_dvfs_prepared);
  assert(pmic[0x220/2]==72 && !y2_wrap_lock);
 }
 reset();pmic[0x216/2]=0;assert(y2_pmic_cpu_voltage_get()==72);
 assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !writes && !requests);
 reset();pmic[0x220/2]=80;assert(y2_pmic_cpu_dvfs_prepare()==-EBUSY && !writes);
 reset();pmic[0x220/2]=0x148;assert(y2_pmic_cpu_voltage_get()==72);
 assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !writes);
 reset();wrapper[0x50/4]=8;assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !writes);
 reset();corrupt_slot=1;assert(y2_pmic_cpu_dvfs_prepare()==-EIO && !requests && faults==1);
 reset();request_failure=1;assert(y2_pmic_cpu_dvfs_prepare()==-ETIMEDOUT && !cpu_dvfs_prepared);
 reset();readback_failure=1;assert(y2_pmic_cpu_dvfs_prepare()==-EIO && !cpu_dvfs_prepared);
 reset();assert(y2_pmic_cpu_voltage_set(88)==-EACCES && !requests);
 reset();assert(!y2_pmic_cpu_dvfs_prepare());wrapper[0xe8/4]=89;
 assert(y2_pmic_cpu_voltage_set(88)==-EIO && requests==1 && pmic[0x220/2]==72);
 reset();assert(!y2_pmic_cpu_dvfs_prepare());pmic[0x216/2]=0;
 assert(y2_pmic_cpu_voltage_set(88)==-EOPNOTSUPP && requests==1);
 reset();assert(!y2_pmic_cpu_dvfs_prepare());request_failure=1;
 assert(y2_pmic_cpu_voltage_set(88)==-ETIMEDOUT && pmic[0x220/2]==72);
 reset();assert(!y2_pmic_cpu_dvfs_prepare());readback_failure=1;
 assert(y2_pmic_cpu_voltage_set(88)==-EIO);
}
''')

    def test_bin_admission_requires_complete_unique_exact_loader_tag(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "cpu-dvfs-policy.h"
static unsigned char b[256];
static void word(unsigned offset,unsigned value){for(unsigned n=0;n<4;n++)b[offset+n]=value>>(n*8);}
static void reset(void){memset(b,0,sizeof b);word(0,2);word(4,0x54410001);
 word(8,25);word(12,0x41000804);word(28,2);word(104,22);}
int main(void){
 reset();assert(y2_cpu_bin0(b,sizeof b));
 for(unsigned size=0;size<116;size++)assert(!y2_cpu_bin0(b,size));
 for(unsigned bin=0;bin<4;bin++)for(unsigned level=0;level<8;level++){
  reset();word(28,bin);word(76,level<<28);
  assert(y2_cpu_bin0(b,sizeof b)==(bin==2 && level==0));
 }
 reset();memcpy(b+108,b+8,100);assert(!y2_cpu_bin0(b,sizeof b));
 reset();word(104,21);assert(!y2_cpu_bin0(b,sizeof b));
 reset();word(8,0xffffffff);assert(!y2_cpu_bin0(b,sizeof b));
 reset();word(8,24);assert(!y2_cpu_bin0(b,sizeof b));
 reset();word(0,5);assert(!y2_cpu_bin0(b,sizeof b));
}
''')

    def test_voltage_frequency_order_and_faults_across_every_opp(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "cpu-dvfs-policy.h"
static unsigned voltage, writes, clocks, failed, fault, bad_readback;
static unsigned long frequency;
static int getv(void *c){return failed==1?-EIO:(int)voltage;}
static int setv(void *c,unsigned v){
 assert(v>=y2_cpu_selector(frequency));writes++;
 if(failed==2)return -ETIMEDOUT;
 voltage=v;return 0;
}
static int setf(void *c,unsigned long f){
 assert(voltage>=y2_cpu_selector(f));clocks++;
 if(failed==3)return -EIO; /* verified rollback retains previous rate */
 frequency=bad_readback?0:f;return 0;
}
static unsigned long getf(void *c){return frequency;}
static void broken(void *c){fault++;}
int main(void){
 struct y2_dvfs_io io={0,getv,setv,setf,getf,broken};
 unsigned long rates[]={598000000,747500000,1040000000,1196000000,1300000000};
 for(unsigned from=0;from<5;from++)for(unsigned to=0;to<5;to++)for(failed=0;failed<4;failed++){
  frequency=rates[from];voltage=y2_cpu_selector(frequency);writes=clocks=fault=0;
  int ret=y2_dvfs_transition(&io,rates[to],1300000000);
  assert(voltage>=y2_cpu_selector(frequency));
  if(!failed){assert(!ret && frequency==rates[to] && voltage==y2_cpu_selector(rates[to]) && !fault);}
  if(failed==1){assert(ret==-EIO && !writes && !clocks);}
  if(failed==2 && y2_cpu_selector(rates[to])>y2_cpu_selector(rates[from]))
   assert(ret==-ETIMEDOUT && !clocks && fault==1);
  if(failed==2 && y2_cpu_selector(rates[to])<y2_cpu_selector(rates[from]))
   assert(!ret && frequency==rates[to] && voltage==y2_cpu_selector(rates[from]) && fault==1);
  if(failed==3){assert(ret==-EIO && frequency==rates[from] && fault==1);}
 }
 failed=0;frequency=1040000000;voltage=72;writes=clocks=fault=0;
 assert(y2_dvfs_transition(&io,1300000000,1040000000)==-EINVAL && !writes && !clocks);
 assert(y2_dvfs_transition(&io,1400000000,1400000000)==-EINVAL && !writes && !clocks);
 frequency=0;assert(y2_dvfs_transition(&io,598000000,1040000000)==-EOPNOTSUPP && !writes);
 frequency=1300000000;voltage=72;
 assert(y2_dvfs_transition(&io,598000000,1040000000)==-ERANGE && !writes && !clocks);
 frequency=1040000000;voltage=72;bad_readback=1;
 assert(y2_dvfs_transition(&io,1300000000,1300000000)==-EIO && voltage==88 && fault==1);
}
''')

    def test_failed_decrease_checks_actual_voltage_and_contains_unknown_readback(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "cpu-dvfs-policy.h"
static int observed, restore_failure, reads;
static unsigned writes, faults;
static unsigned long rate;
static int getv(void *c){return reads++ ? observed : 88;}
static int setv(void *c,unsigned selector){
 writes++;
 if(selector==88 && !restore_failure){observed=88;return 0;}
 return -EIO;
}
static int setf(void *c,unsigned long hz){rate=hz;return 0;}
static unsigned long getf(void *c){return rate;}
static void broken(void *c){faults++;}
int main(void){
 struct y2_dvfs_io io={0,getv,setv,setf,getf,broken};
 int values[]={72,80,88,71,-EIO,89};
 for(unsigned i=0;i<6;i++)for(restore_failure=0;restore_failure<2;restore_failure++){
  rate=1300000000;observed=values[i];reads=writes=faults=0;
  int safe=observed==72 || observed==80 || observed==88;
  int ret=y2_dvfs_transition(&io,1040000000,1300000000);
  assert(faults==1);
  assert(writes==(safe?1:2));
  if(!safe && restore_failure){assert(ret==-ERANGE && rate==598000000);}
  else{assert(!ret && rate==1040000000 && observed>=72);}
 }
}
''')

    def test_actual_spm_request_bounds_and_ownership(self):
        source = (ROOT / 'kernel/platform/spm.c').read_text().replace(
            'int y2_spm_cpu_voltage_request(', 'static int y2_spm_cpu_voltage_request(')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#define EPROBE_DEFER 517
#include "spm-regs.h"
#define BIT(n) (1U<<(n))
#define READ_ONCE(x) (x)
#define smp_load_acquire(p) (*(p))
static unsigned regs[1024], delays, writes, timeout, corrupt, spm_lock;
static void *spm_base=regs;
static bool spm_broken;
static unsigned normal_address=0x90000000,pcm_address=0x90001000;
static unsigned spm_read(void *p,unsigned reg){return regs[reg/4];}
static void spm_write(void *p,unsigned reg,unsigned value){assert(spm_lock);regs[reg/4]=value;writes++;}
static void udelay(unsigned n){assert(n==5 && spm_lock);delays++;
 if(!timeout && delays==3)regs[0x604/4]|=BIT(31);
 if(corrupt)regs[0x604/4]^=1;
}
#define raw_spin_lock_irqsave(p,f) do{assert(!*(p));*(p)=1;f=0;}while(0)
#define raw_spin_unlock_irqrestore(p,f) do{assert(*(p));*(p)=0;(void)f;}while(0)
''' + function(source, 'y2_spm_cpu_voltage_request') + r'''
static void reset(void){memset(regs,0,sizeof regs);regs[SPM_PCM_IM_PTR/4]=normal_address;
 regs[SPM_PCM_IM_LEN/4]=27;regs[0x604/4]=0x55aa0002;delays=writes=timeout=corrupt=spm_broken=0;}
int main(void){
 for(unsigned slot=0;slot<3;slot++){
  reset();assert(!y2_spm_cpu_voltage_request(slot) && writes==1 && delays==3 && !spm_lock);
  assert((regs[0x604/4]&~(BIT(31)|7))==0x55aa0000);
 }
 reset();timeout=1;assert(y2_spm_cpu_voltage_request(0)==-ETIMEDOUT && delays==101 && !spm_lock);
 reset();corrupt=1;assert(y2_spm_cpu_voltage_request(0)==-EIO && delays==1);
 reset();assert(y2_spm_cpu_voltage_request(3)==-EINVAL && !writes);
 reset();spm_broken=1;assert(y2_spm_cpu_voltage_request(0)==-EBUSY && !writes);
 reset();regs[SPM_PCM_IM_PTR/4]+=4;assert(y2_spm_cpu_voltage_request(0)==-EBUSY && !writes);
 reset();regs[SPM_PCM_CON1/4]=CON1_PCM_TIMER_EN;
 assert(y2_spm_cpu_voltage_request(0)==-EBUSY && !writes);
 reset();regs[SPM_PCM_CON1/4]=CON1_PCM_WDT_EN;
 assert(y2_spm_cpu_voltage_request(0)==-EBUSY && !writes);
 reset();regs[SPM_PCM_IM_PTR/4]=pcm_address;regs[SPM_PCM_IM_LEN/4]=596;
 assert(!y2_spm_cpu_voltage_request(2));
 spm_base=0;assert(y2_spm_cpu_voltage_request(0)==-EPROBE_DEFER);
}
''')
