"""Regression tests for observed POWER-02 RTC and efuse failures."""
from pathlib import Path
import json
import unittest
from test_audio import function
from test_power import run_c
from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]


def source(path):
    specs = json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
    return prepare_overlay(ROOT, next(s for s in specs if s['path'] == path)).read_text()


class Power03(unittest.TestCase):
    def test_word_efuse_read_preserves_upper_lanes_and_bounds(self):
        run_c(r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>
struct mtk_efuse_priv { void *base; };
static uint32_t words[2]={0x91ab5b73,0x82123afd}; /* synthetic, not factory data */
static unsigned reads;
static uint32_t readl(void *p){assert(p==words || p==words+1);reads++;return *(uint32_t *)p;}
static void put_unaligned_le32(uint32_t v,void *p){memcpy(p,&v,4);}
''' + function(source('drivers/nvmem/mtk-efuse.c'), 'mt6582_reg_read') + r'''
int main(void){
 struct mtk_efuse_priv p={words};unsigned char b[10];memset(b,0xa5,sizeof b);
 assert(!mt6582_reg_read(&p,0,b+1,8) && reads==2);
 assert(!memcmp(b+1,words,8) && b[0]==0xa5 && b[9]==0xa5);
 assert(mt6582_reg_read(&p,1,b,4)==-EINVAL && reads==2);
 assert(mt6582_reg_read(&p,0,b,7)==-EINVAL && reads==2);
}
''')

    def test_rtc_stock_epoch_time_alarm_and_spare_preservation(self):
        rtc = source('drivers/rtc/rtc-mt6397.c')
        header = source('include/linux/mfd/mt6397/rtc.h')
        defines = '\n'.join(line for line in header.splitlines() if line.startswith('#define RTC_'))
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <string.h>
#define BIT(n) (1U<<(n))
typedef uint16_t u16;typedef unsigned u32;
struct rtc_time {int tm_sec,tm_min,tm_hour,tm_mday,tm_mon,tm_year,tm_wday;};
struct rtc_wkalrm {struct rtc_time time;int enabled,pending;};
struct mtk_rtc_data {unsigned year_offset;};
struct mt6397_rtc {void *regmap;unsigned addr_base;int lock;struct mtk_rtc_data *data;};
struct device {struct mt6397_rtc *rtc;};
static struct mt6397_rtc *dev_get_drvdata(struct device *d){return d->rtc;}
static u16 regs[32];static int locked,op,fail,triggers,rollover;
static void mutex_lock(int *p){assert(!locked);locked=1;}
static void mutex_unlock(int *p){assert(locked);locked=0;}
static int regmap_bulk_read(void *m,unsigned r,u16 *v,unsigned n){
 assert(locked);if(++op==fail)return -EIO;memcpy(v,regs+r/2,n*2);return 0;
}
static int regmap_bulk_write(void *m,unsigned r,u16 *v,unsigned n){
 assert(locked);if(++op==fail)return -EIO;memcpy(regs+r/2,v,n*2);return 0;
}
static int regmap_read(void *m,unsigned r,void *p){
 assert(locked);if(++op==fail)return -EIO;*(unsigned *)p=regs[r/2];
 if(rollover && r==10)*(unsigned *)p=0;
 return 0;
}
static int regmap_update_bits(void *m,unsigned r,unsigned mask,unsigned v){
 assert(locked);if(++op==fail)return -EIO;regs[r/2]=(regs[r/2]&~mask)|(v&mask);return 0;
}
static int mtk_rtc_write_trigger(struct mt6397_rtc *rtc){assert(locked);triggers++;return 0;}
''' + defines + '\n' + ''.join(function(rtc, n) for n in (
            '__mtk_rtc_read_time', 'mtk_rtc_read_time', 'mtk_rtc_set_time',
            'mtk_rtc_read_alarm', 'mtk_rtc_set_alarm')) + r'''
int main(void){
 struct mtk_rtc_data data={68};struct mt6397_rtc rtc={.data=&data};struct device dev={&rtc};
 for(unsigned year=68;year<=195;year++) {
  for(unsigned i=0;i<32;i++)regs[i]=0xa580;
  struct rtc_time tm={.tm_year=year,.tm_mon=7,.tm_mday=1,.tm_wday=1,.tm_hour=12,.tm_min=34,.tm_sec=56};
  struct rtc_time got;
  assert(!mtk_rtc_set_time(&dev,&tm));
  assert((regs[RTC_TC_SEC/2+6]&127)==year-68);
  assert(!mtk_rtc_read_time(&dev,&got) && got.tm_year==(int)year && got.tm_mon==7 && got.tm_sec==56);
  for(unsigned i=RTC_TC_SEC/2;i<RTC_TC_SEC/2+7;i++)assert((regs[i]&0xff80)==0xa580);
  assert(regs[RTC_PDN2/2]==0xa580 && regs[0x2c/2]==0xa580 && regs[0x30/2]==0xa580);
  struct rtc_wkalrm alarm={.time=got,.enabled=1},read_alarm;
  assert(!mtk_rtc_set_alarm(&dev,&alarm));
  assert((regs[RTC_AL_SEC/2+6]&127)==year-68);
  assert(!mtk_rtc_read_alarm(&dev,&read_alarm) && read_alarm.enabled);
  assert(read_alarm.time.tm_year==(int)year && read_alarm.time.tm_mon==7);
  for(unsigned i=RTC_AL_SEC/2;i<RTC_AL_SEC/2+7;i++)assert((regs[i]&0xff80)==0xa580);
  assert(regs[RTC_PDN2/2]==0xa580);
 }
 /* The actual old year-54 case is 2022, never 2082. */
 regs[RTC_TC_SEC/2+6]=54;struct rtc_time got;
 assert(!mtk_rtc_read_time(&dev,&got) && got.tm_year==122);
 for(unsigned n=1;n<=2;n++) {op=0;fail=n;assert(mtk_rtc_read_time(&dev,&got)==-EIO && !locked);}
 fail=0;rollover=1;op=0;
 assert(mtk_rtc_read_time(&dev,&got)==-EIO && op==6 && !locked); /* bounded inconsistent snapshot */
 rollover=0;op=0;fail=2;triggers=0;
 struct rtc_time tm={.tm_year=126,.tm_mon=8,.tm_mday=15};
 assert(mtk_rtc_set_time(&dev,&tm)==-EIO && !triggers && !locked);
}
''')
