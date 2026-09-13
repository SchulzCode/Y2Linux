"""M4 conversion, transport-failure and CPU transition checks; no hardware IO."""
from pathlib import Path
import json
import os
import struct
import subprocess
import tempfile
import unittest
from test_audio import function
from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]

def run_c(source):
    with tempfile.TemporaryDirectory() as directory:
        p = Path(directory)
        (p/'test.c').write_text(source)
        subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror',
                        '-Wno-unused-parameter', '-fsanitize=undefined', '-fsanitize-trap=undefined',
                        '-I'+str(ROOT/'kernel/platform'), str(p/'test.c'), '-o', str(p/'test')], check=True)
        subprocess.run([str(p/'test')], check=True)

class Power(unittest.TestCase):
    def test_conversion_units_calibration_and_protection(self):
        run_c(r'''
#include <assert.h>
#include "policy.h"
#include "power-math.h"
int main(void) {
 struct y2_pmic_cal p;
 struct y2_soc_cal s;
 assert(y2_pmic_calibrate(0x2023,0,0,&p));
 assert(y2_pmic_calibrate(0x9999,0x46da,0x800f,&p));
 /* Our PMIC fuses: reference point 27 C, not a live temperature sample. */
 assert(!y2_pmic_calibrate(0x2023,0x46da,0x800f,&p));
 assert(p.divisor==-171 && y2_pmic_mc(&p,3910+9102)==27000);
 for(unsigned r=1;r<32768;r++) assert(y2_pmic_mc(&p,r)<=y2_pmic_mc(&p,r-1));
 assert(y2_battery_uv(16384)==3600000);
 assert(y2_battery_uv(0)==0 && y2_battery_uv(32767)==7199780);
 assert(y2_soc_calibrate(0,0,&s));
 assert(y2_soc_calibrate(0xffffffff,0xffffffff,&s));
 /* Synthetic calibration words exercise both signs, ADC gain and offset.
  * They are test vectors only and are never used as device defaults. */
 for(unsigned gain=1;gain<1024;gain+=73) for(unsigned offset=0;offset<1024;offset+=97)
 for(unsigned sign=0;sign<2;sign++) {
  unsigned a=(31U<<26)|(240<<17)|(310<<8)|(sign<<7)|(50<<1)|1;
  unsigned b=(gain<<22)|(offset<<12)|(1<<9);
  assert(!y2_soc_calibrate(a,b,&s));
  assert(y2_soc_mc(&s,3590,0)==25000);
  assert(y2_soc_mc(&s,3660,1)==25000);
  for(unsigned r=1;r<4095;r++) assert(y2_soc_mc(&s,r+1,0)<=y2_soc_mc(&s,r,0));
 }
 assert(y2_charge_ua(12)==450000 && y2_charge_ua(6)==1000000);
 assert(y2_charge_ua(15)==70000 && y2_charge_uv(30)==4175000);
 /* No charge-enable, current/CV, watchdog, efuse or NVRAM write path. */
 assert(!y2_pmic_value_allowed(0,0x7b));
 assert(y2_pmic_value_allowed(0,0x63));
 for(unsigned r=2;r<0x30;r+=2) assert(!y2_pmic_write_mask(r));
 for(unsigned r=0x600;r<0x700;r+=2) assert(!y2_pmic_write_mask(r));
 assert(y2_pmic_value_allowed(0x8000,0x4300));
 assert(!y2_pmic_value_allowed(0x8000,0x4309));
 assert(y2_pmic_write_mask(0x802e)==0); /* boot state remains read-only */
 assert(!y2_pmic_write_mask(0x216) && !y2_pmic_write_mask(0x21e));
}
''')

    def test_actual_adc_transaction_failures_and_buffer_restore(self):
        src = (ROOT/'kernel/platform/mt6323-adc.c').read_text()
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#define BIT(n) (1U<<(n))
struct regmap { unsigned r[1024]; };
struct y2_adc { struct regmap *map; int lock; };
static int locked, op, fail, timeout;
static void mutex_lock(int *l){assert(!locked);locked=1;}
static void mutex_unlock(int *l){assert(locked);locked=0;}
static int regmap_read(struct regmap *m,unsigned reg,unsigned *v){assert(locked);if(++op==fail)return -EIO;*v=m->r[reg/2];return 0;}
static int regmap_update_bits(struct regmap *m,unsigned reg,unsigned mask,unsigned v){
 assert(locked);if(++op==fail)return -EIO;m->r[reg/2]=(m->r[reg/2]&~mask)|(v&mask);return 0;
}
static void usleep_range(unsigned a,unsigned b){assert(locked && a>=1000 && b>=a);}
#define regmap_read_poll_timeout(m,r,v,cond,delay,limit) \
 ({int x=regmap_read(m,r,&v); if(!x && (timeout || !(cond))) x=-ETIMEDOUT; x;})
''' + function(src, 'y2_adc_sample') + r'''
int main(void){
 struct regmap m={0};struct y2_adc a={&m,0};int raw;
 for(unsigned ch=3;ch<=7;ch+=4)for(unsigned vbuf=0;vbuf<=1;vbuf++)for(fail=0;fail<=6;fail++){
  memset(&m,0,sizeof(m));op=0;raw=-1;m.r[0x758/2]=0xa003|(vbuf<<4);
  m.r[0x76e/2]=0x5511;m.r[(ch==7?0x714:0x71c)/2]=0x8000|17000;
  int r=y2_adc_sample(&a,ch,&raw); assert(!locked);
  if(fail) assert(r==-EIO); else assert(r==0 && raw==17000);
  if(fail!=6) assert(m.r[0x758/2]==(0xa003|(vbuf<<4)));
  assert((m.r[0x76e/2]&~BIT(ch))==(0x5511&~BIT(ch)));
 }
 fail=0;op=0;timeout=1;raw=-1;
 assert(y2_adc_sample(&a,7,&raw)==-ETIMEDOUT && raw==-1 && !locked);
 assert(y2_adc_sample(&a,6,&raw)==-EINVAL && !locked);
}
''')

    def test_actual_shared_cpu_transition_and_rejection(self):
        src = (ROOT/'kernel/platform/clocks.c').read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <errno.h>
#include "clocks.h"
#include "policy.h"
#include "power-math.h"
#define BIT(n) (1U<<(n))
#define IS_ENABLED(c) 1
#define __iomem
struct clk_hw {int unused;};
struct y2_clock {struct clk_hw hw; void *pll,*infra;};
#define container_of(p,t,m) ((t *)((char *)(p)-offsetof(t,m)))
static unsigned pll[384], infra[16], writes, delay, fail_pcw;
static int voltage_error, y2_clk_lock;
static int y2_pmic_cpu_voltage_ready(void){return voltage_error;}
static unsigned readl(void *p){
 if(p==pll+0x204/4 && writes==3 && fail_pcw){fail_pcw=0;return 0;}
 return *(unsigned *)p;
}
static void writel(unsigned v,void *p){
 writes++;
 if(writes==1)assert(p==infra+2 && v==10);
 if(writes==2)assert(p==infra && (v&12)==8);
 if(p==pll+0x204/4)assert((infra[0]&12)==8 && infra[2]==10);
 *(unsigned *)p=v;
}
static void udelay(unsigned us){assert(us>=30);delay++;}
#define mb() ((void)0)
#define spin_lock_irqsave(p,f) do{assert(!*(p));*(p)=1;f=0;}while(0)
#define spin_unlock_irqrestore(p,f) do{assert(*(p));*(p)=0;(void)f;}while(0)
''' + function(src, 'y2_pll_rate') + function(src, 'y2_cpu_set') + r'''
int main(void){
 struct y2_clock c={.pll=pll,.infra=infra};
 unsigned rates[]={598000000,747500000,1040000000};
 pll[0x200/4]=pll[0x210/4]=1;pll[0x214/4]=0xa8000;
 for(unsigned from=0;from<3;from++)for(unsigned to=0;to<3;to++){
  pll[0x204/4]=y2_cpu_pcw(rates[from]);infra[0]=0xabcd0004;infra[2]=0;writes=delay=0;
  assert(!y2_cpu_set(&c.hw,rates[to],0));
  assert(y2_pll_rate(pll,0)==rates[to] && infra[0]==0xabcd0004 && infra[2]==0);
  assert(writes==5 && delay==1 && !y2_clk_lock);
 }
 for(unsigned error=0;error<4;error++){
  writes=0;voltage_error=error==0?-EIO:0;
  pll[0x14/4]=error==1;infra[2]=error==2?3:0;infra[0]=error==3?8:4;
  assert(y2_cpu_set(&c.hw,598000000,0)<0 && !writes && !y2_clk_lock);
 }
 pll[0x14/4]=0;infra[0]=4;infra[2]=0;voltage_error=0;
 assert(y2_cpu_set(&c.hw,1300000000,0)==-EINVAL && !writes);
 pll[0x204/4]=y2_cpu_pcw(1040000000);writes=delay=0;fail_pcw=1;
 assert(y2_cpu_set(&c.hw,598000000,0)==-EIO && delay==2);
 assert(y2_pll_rate(pll,0)==1040000000 && infra[0]==4 && infra[2]==0);
}
''')

    def test_actual_pmic_wake_mask_failure_and_restore(self):
        spec = next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/mfd/mt6397-irq.c')
        source = prepare_overlay(ROOT,spec).read_text()
        source = source.replace('int mt6323_irq_suspend(', 'static int mt6323_irq_suspend(')
        source = source.replace('int mt6323_irq_resume(', 'static int mt6323_irq_resume(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
typedef uint16_t u16;
#define MT6323_CHIP_ID 0x23
struct regmap {unsigned r[512];};
struct device {void *data;};
struct mt6397_chip {struct regmap *regmap;unsigned chip_id;int irqlock,irq;
 u16 int_con[3],wake_mask[3],irq_masks_cur[3];bool irq_suspended,irq_wake_enabled;};
static void *dev_get_drvdata(struct device *d){return d->data;}
static int op,fail,wake_fail,depth,locked;
static void mutex_lock(int *l){assert(!locked);locked=1;}
static void mutex_unlock(int *l){assert(locked);locked=0;}
static int regmap_write(struct regmap *m,unsigned r,unsigned v){
 assert(locked);if(++op==fail)return -EIO;m->r[r/2]=v;return 0;
}
static int enable_irq_wake(int i){if(wake_fail)return -EINVAL;depth++;return 0;}
static int disable_irq_wake(int i){assert(depth==1);depth--;return 0;}
#define dev_err(...) ((void)0)
''' + ''.join(function(source,n) for n in ('mt6323_write_masks','mt6323_irq_suspend','mt6323_irq_resume')) + r'''
int main(void){
 struct regmap m={0};struct mt6397_chip c={.regmap=&m,.chip_id=0x23,.irq=51,
 .int_con={0x160,0x166,0},.irq_masks_cur={0x420,0x10,0}};struct device d={&c};
 /* No unselected IRQ is made a wake source. */
 assert(!mt6323_irq_suspend(&d) && !depth && c.irq_suspended);
 assert(m.r[0x160/2]==0 && m.r[0x166/2]==0);
 assert(!mt6323_irq_resume(&d));
 /* Children select Power + RTC before the MFD late callback runs. */
 c.wake_mask[0]=0x20;c.wake_mask[1]=0x10;
 assert(!mt6323_irq_suspend(&d) && depth==1 && c.irq_suspended);
 assert(m.r[0x160/2]==0x20 && m.r[0x166/2]==0x10);
 assert(!mt6323_irq_resume(&d) && !depth && !c.irq_suspended);
 assert(m.r[0x160/2]==0x420 && m.r[0x166/2]==0x10);
 for(fail=1;fail<=2;fail++){
  op=0;assert(mt6323_irq_suspend(&d)==-EIO && !c.irq_suspended && !depth);
  assert(m.r[0x160/2]==0x420 && m.r[0x166/2]==0x10);
 }
 fail=0;wake_fail=1;
 assert(mt6323_irq_suspend(&d)==-EINVAL && !depth && !c.irq_suspended && !locked);
 assert(m.r[0x160/2]==0x420 && m.r[0x166/2]==0x10);
}
''')

    def test_actual_emmc_suspend_drains_without_losing_storage06_state(self):
        spec = next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/mmc/core/mmc.c')
        source = prepare_overlay(ROOT,spec).read_text()
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
typedef uint32_t u32;
#define IS_ENABLED(c) 1
#define EXT_CSD_POWER_OFF_SHORT 1
#define EXT_CSD_POWER_OFF_LONG 2
#define R1_READY_FOR_DATA (1U<<8)
#define R1_STATE_TRAN 4
#define R1_CURRENT_STATE(s) (((s)>>9)&15)
enum mmc_poweroff_type {MMC_POWEROFF_SHUTDOWN, MMC_POWEROFF_SUSPEND, MMC_POWEROFF_UNDERVOLTAGE};
struct mmc_card {int suspended;};
struct device {int of_node;};
struct mmc_host {struct mmc_card *card;struct device *parent;};
static int claimed,flush_error,status_error,flushed,queried;
static unsigned response;
static void mmc_claim_host(struct mmc_host *h){assert(!claimed);claimed=1;}
static void mmc_release_host(struct mmc_host *h){assert(claimed);claimed=0;}
static bool mmc_card_suspended(struct mmc_card *c){return c->suspended;}
static int _mmc_flush_cache(struct mmc_host *h){assert(claimed);flushed++;return flush_error;}
static bool of_device_is_compatible(int n,const char *s){return n==1;}
static int mmc_send_status(struct mmc_card *c,unsigned *s){assert(claimed && flushed);queried++;*s=response;return status_error;}
#define mmc_card_can_poweroff_notify(c) 1
#define mmc_host_can_poweroff_notify(h,p) 1
static int mmc_poweroff_notify(struct mmc_card *c,unsigned t){assert(0);return 0;}
#define mmc_card_can_sleep(c) 1
static int mmc_sleep(struct mmc_host *h){assert(0);return 0;}
#define mmc_host_is_spi(h) 0
static int mmc_deselect_cards(struct mmc_host *h){assert(0);return 0;}
static void mmc_power_off(struct mmc_host *h){assert(0);}
static void mmc_card_set_suspended(struct mmc_card *c){assert(0);}
''' + function(source, '_mmc_suspend') + r'''
int main(void){
 struct mmc_card c={0};struct device d={1};struct mmc_host h={&c,&d};
 for(int mode=0;mode<2;mode++)for(int fault=0;fault<5;fault++){
  flushed=queried=0;flush_error=fault==1?-EIO:0;status_error=fault==2?-ETIMEDOUT:0;
  response=(fault==3?0:R1_READY_FOR_DATA)|((fault==4?7:R1_STATE_TRAN)<<9);
  int r=_mmc_suspend(&h,mode);
  assert(r==(fault==1?-EIO:fault==2?-ETIMEDOUT:fault>=3?-EBUSY:0));
  assert(flushed==1 && queried==(fault!=1) && !claimed && !c.suspended);
 }
}
''')

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'), 'requires emitted integrated DT')
    def test_power_dt_rejects_unreviewed_voltage_wake_and_calibration(self):
        from tools.validation.dev_dtb import check
        root = Path(os.environ['Y2_ARTIFACT_TEST_ROOT'])
        raw = (root/'y2.dtb').read_bytes()
        size = (root/'initramfs.cpio.gz').stat().st_size
        check(raw,size)
        for before,after in [(1150000,1250000),(1040000000,1300000000),(150000,200000),
                              (120000,150000),(0x10206100,0x10206108)]:
            with self.subTest(before=before), self.assertRaises((ValueError,KeyError)):
                check(raw.replace(struct.pack('>I',before),struct.pack('>I',after)),size)
