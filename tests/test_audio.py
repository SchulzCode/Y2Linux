"""Exercise production MT6582 trigger/IRQ code with MMIO fault injection."""
from pathlib import Path
import os, re, struct, subprocess, tempfile, unittest
ROOT=Path(__file__).resolve().parents[1]

def function(source,name):
    start=re.search(r'^static [^;{}]*\b'+name+r'\([^;{}]*\)\n\{',source,re.M).start()
    end=source.index('{',start); depth=1; end+=1
    while depth:
        depth+=(source[end]=='{')-(source[end]=='}');end+=1
    return source[start:end]+'\n'

class AFE(unittest.TestCase):
    def test_real_trigger_irq_and_dma_bounds(self):
        src=(ROOT/'kernel/audio/mt6582-afe.c').read_text()
        fixture=r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
#include "mt6582-afe.h"
#define BIT(n) (1U<<(n))
#define EINVAL 22
#define SNDRV_PCM_TRIGGER_START 1
#define SNDRV_PCM_TRIGGER_RESUME 2
#define SNDRV_PCM_TRIGGER_STOP 3
#define SNDRV_PCM_TRIGGER_SUSPEND 4
#define IRQ_NONE 0
#define IRQ_HANDLED 1
typedef int irqreturn_t;
struct regmap {unsigned r[0x600/4];};
struct snd_pcm_runtime {unsigned rate,period_size;};
struct snd_pcm_substream {struct snd_pcm_runtime *runtime;};
struct mt6582_afe {struct regmap *regmap; struct snd_pcm_substream *substream; int lock; bool running; unsigned long periods;};
struct snd_soc_dai {struct mt6582_afe *afe;};
static struct mt6582_afe *snd_soc_dai_get_drvdata(struct snd_soc_dai *d){return d->afe;}
static int locked, op, fail, elapsed, callback_stop;
static unsigned ack;
#define spin_lock_irqsave(p,f) do{(void)p; f=0; assert(!locked); locked=1;}while(0)
#define spin_unlock_irqrestore(p,f) do{(void)p;(void)f;assert(locked);locked=0;}while(0)
static int regmap_update_bits(struct regmap *m,unsigned r,unsigned mask,unsigned v) {
 if(++op==fail)return -5;
 m->r[r/4]=(m->r[r/4]&~mask)|(v&mask);return 0;
}
static int regmap_write(struct regmap *m,unsigned r,unsigned v) {
 if(++op==fail)return -5;
 if(r==AFE_IRQ_CLR){ack=v;m->r[AFE_IRQ_STATUS/4]&=~v;}
 else m->r[r/4]=v;
 return 0;
}
static int regmap_read(struct regmap *m,unsigned r,unsigned *v){*v=m->r[r/4];return 0;}
static void snd_pcm_period_elapsed(struct snd_pcm_substream *s);
'''
        fixture+=''.join(function(src,n) for n in ('mt6582_stop','mt6582_trigger','mt6582_irq'))
        fixture+=r'''
static struct snd_soc_dai *active_dai;
static void snd_pcm_period_elapsed(struct snd_pcm_substream *s) {
 assert(!locked); assert(!(active_dai->afe->regmap->r[AFE_IRQ_STATUS/4]&IRQ1));
 elapsed++; if(callback_stop)assert(!mt6582_trigger(s,SNDRV_PCM_TRIGGER_STOP,active_dai));
}
int main(void) {
 struct regmap map={0};struct snd_pcm_runtime rt={44100,1024};
 struct snd_pcm_substream sub={&rt};struct mt6582_afe afe={.regmap=&map,.substream=&sub};
 struct snd_soc_dai dai={&afe};active_dai=&dai;
 assert(mt6582_rate_code(44100)==9 && mt6582_rate_code(48000)==10);
 assert(mt6582_rate_code(96000)<0 && mt6582_rate_code(0)<0);
 assert(mt6582_dma_valid(0x84000000,262144));
 assert(mt6582_dma_valid(0xfffffc00ULL,1024));
 assert(!mt6582_dma_valid(0xfffffc00ULL,1040));
 assert(!mt6582_dma_valid(0x100000000ULL,1024));
 assert(!mt6582_dma_valid(0x84000001,1024));
 assert(!mt6582_dma_valid(0x84000000,1023));
 for(unsigned i=0;i<262144;i+=4)assert(mt6582_pointer_bytes(0x84000000,0x84000000+i,262144)==i);
 assert(!mt6582_pointer_bytes(0x84000000,0,262144));
 assert(!mt6582_pointer_bytes(0x84000000,0x83fffffc,262144));
 assert(!mt6582_pointer_bytes(0x84000000,0x84040000,262144));
 for(unsigned rate=44100;rate<=48000;rate+=3900)for(fail=0;fail<=5;fail++) {
  rt.rate=rate;op=0;memset(&map,0,sizeof(map));afe.running=false;
  int ret=mt6582_trigger(&sub,SNDRV_PCM_TRIGGER_START,&dai);
  if(fail){assert(ret==-5);assert(!afe.running);assert(!(map.r[AFE_DAC_CON0/4]&DL1_ON));assert(!(map.r[AFE_IRQ_CON/4]&IRQ1));continue;}
  assert(!ret&&afe.running);assert(map.r[AFE_IRQ_CNT1/4]==1024);
  assert(map.r[AFE_IRQ_CON/4]==((unsigned)mt6582_rate_code(rate)<<4|IRQ1));
  elapsed=0;callback_stop=1;map.r[AFE_IRQ_STATUS/4]=IRQ1;
  assert(mt6582_irq(1,&afe)==IRQ_HANDLED&&elapsed==1&&!afe.running);
  assert(mt6582_irq(1,&afe)==IRQ_NONE);
  map.r[AFE_IRQ_STATUS/4]=IRQ1;assert(mt6582_irq(1,&afe)==IRQ_HANDLED&&elapsed==1);
  afe.substream=NULL;map.r[AFE_IRQ_STATUS/4]=IRQ1;
  assert(mt6582_irq(1,&afe)==IRQ_HANDLED&&elapsed==1);afe.substream=&sub;
 }
 fail=0;rt.rate=96000;assert(mt6582_trigger(&sub,SNDRV_PCM_TRIGGER_START,&dai)==-EINVAL);
 rt.rate=44100;rt.period_size=0x40000;assert(mt6582_trigger(&sub,SNDRV_PCM_TRIGGER_START,&dai)==-EINVAL);
 assert(mt6582_trigger(&sub,100,&dai)==-EINVAL);
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'test.c').write_text(fixture)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror','-Wno-unused-parameter',
                            '-I'+str(ROOT/'kernel/audio'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'),'needs built DT')
    def test_resolved_audio_dt_rejects_unsafe_rails(self):
        from tools.validation.dev_dtb import check
        root=Path(os.environ['Y2_ARTIFACT_TEST_ROOT']);data=(root/'y2.dtb').read_bytes()
        size=(root/'initramfs.cpio.gz').stat().st_size
        check(data,size)
        for old,new in [(struct.pack('>I',1800000),struct.pack('>I',2500000)),
                        (b'cirrus,cs43131\0',b'cirrus,cs43130\0'),
                        (struct.pack('>III',0,104,8),struct.pack('>III',0,105,8)),
                        (b'output-low\0',b'output-hig\0')]:
            with self.subTest(old=old):
                self.assertIn(old,data)
                with self.assertRaises((ValueError,KeyError)):
                    check(data.replace(old,new,1),size)
