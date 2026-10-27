"""Y2 card policy and pinned DAPM runtime-reference transitions, without hardware."""
from pathlib import Path
import unittest
from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]


class AudioIdle(unittest.TestCase):
    def test_late_probe_preserves_links_pin_and_applies_codec_only_policy(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
struct device { int unused; };
struct snd_soc_dapm_context { bool idle; };
struct snd_soc_component { struct snd_soc_dapm_context dapm; };
struct snd_soc_dai { struct device *dev; struct snd_soc_component *component; };
struct snd_soc_card { struct device *dev; struct snd_soc_dapm_context dapm; };
struct y2_audio { struct snd_soc_card card; int links[2]; };
struct snd_soc_pcm_runtime { struct snd_soc_dai cpu, codec; };
#define container_of(p,type,member) ((type *)((char *)(p)-offsetof(type,member)))
#define DL_FLAG_AUTOREMOVE_CONSUMER 1
static struct device carddev, afedev, codecdev;
static struct snd_soc_component component;
static struct snd_soc_pcm_runtime rtd = { { &afedev, NULL }, { &codecdev, &component } };
static int no_rtd, link_fail, links, disabled, pin_error;
static struct snd_soc_pcm_runtime *snd_soc_get_pcm_runtime(struct snd_soc_card *c,int *l) {
 assert(l==&container_of(c,struct y2_audio,card)->links[1]);return no_rtd?NULL:&rtd;
}
static struct snd_soc_dai *snd_soc_rtd_to_cpu(struct snd_soc_pcm_runtime *r,unsigned i){assert(!i);return &r->cpu;}
static struct snd_soc_dai *snd_soc_rtd_to_codec(struct snd_soc_pcm_runtime *r,unsigned i){assert(!i);return &r->codec;}
static struct snd_soc_dapm_context *snd_soc_component_get_dapm(struct snd_soc_component *c){return &c->dapm;}
static void snd_soc_dapm_set_idle_bias(struct snd_soc_dapm_context *d,bool b){assert(d==&component.dapm);d->idle=b;}
static void *device_link_add(struct device *consumer,struct device *supplier,unsigned flags){
 assert(consumer==&carddev && flags==DL_FLAG_AUTOREMOVE_CONSUMER);
 assert(supplier==((links%2)?&codecdev:&afedev));return ++links==link_fail?NULL:supplier;
}
static int snd_soc_dapm_disable_pin(struct snd_soc_dapm_context *d,const char *name){
 /* Headphone first, then the speaker (#28 speaker card). */
 assert(d && name[0]==(disabled?'S':'H'));disabled++;return pin_error;
}
''' + function((ROOT/'kernel/audio/y2-cs43131.c').read_text(), 'y2_late_probe') + r'''
int main(void){
 struct y2_audio y={.card={.dev=&carddev,.dapm={true}}};
 for(int fault=0;fault<=3;fault++) {
  no_rtd=fault==1;link_fail=fault==2?1:fault==3?2:0;links=disabled=0;
  component.dapm.idle=true;int rc=y2_late_probe(&y.card);
  if(fault){assert(rc==-ENOMEM && component.dapm.idle && !disabled);}
  else{assert(!rc && links==2 && !component.dapm.idle && disabled==2 && y.card.dapm.idle);}
 }
 no_rtd=link_fail=links=disabled=0;pin_error=-EINVAL;
 assert(y2_late_probe(&y.card)==-EINVAL && disabled==1);
}
''')

    def test_real_dapm_sequence_balances_stream_and_standby_pm_references(self):
        path = ROOT/'.cache/sources/linux-6.18/sound/soc/soc-dapm.c'
        if not path.exists():
            self.skipTest('pinned Linux 6.18 source is not prepared')
        source = path.read_text().replace('void snd_soc_dapm_set_idle_bias(',
                                          'static void snd_soc_dapm_set_idle_bias(')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
typedef unsigned async_cookie_t;
enum { SND_SOC_BIAS_OFF, SND_SOC_BIAS_STANDBY, SND_SOC_BIAS_PREPARE, SND_SOC_BIAS_ON };
struct device { int refs, gets, puts; };
struct snd_soc_dapm_context { struct device *dev; int bias_level,target_bias_level; bool idle_bias; };
static int pm_runtime_get_sync(struct device *d){d->refs++;d->gets++;return 0;}
static void pm_runtime_put(struct device *d){assert(d->refs>0);d->refs--;d->puts++;}
static int snd_soc_dapm_set_bias_level(struct snd_soc_dapm_context *d,int b){d->bias_level=b;return 0;}
#define dev_err(...) do {} while(0)
''' + ''.join(function(source, name) for name in ('snd_soc_dapm_set_idle_bias',
            'dapm_pre_sequence_async', 'dapm_post_sequence_async')) + r'''
static void transition(struct snd_soc_dapm_context *d,int target) {
 d->target_bias_level=target;dapm_pre_sequence_async(d,1);dapm_post_sequence_async(d,1);
 assert(d->bias_level==target);
}
int main(void){
 struct device dev={0};struct snd_soc_dapm_context d={.dev=&dev,.idle_bias=true};
 /* Existing standby holds a runtime reference with no active audio widget. */
 transition(&d,SND_SOC_BIAS_STANDBY);assert(dev.refs==1);
 snd_soc_dapm_set_idle_bias(&d,false);assert(!d.idle_bias);
 transition(&d,SND_SOC_BIAS_OFF);assert(dev.refs==0 && dev.puts==1);
 /* Initial OFF does not underflow; each later stream owns one reference. */
 transition(&d,SND_SOC_BIAS_OFF);assert(dev.refs==0 && dev.puts==1);
 for(unsigned cycle=0;cycle<100;cycle++) {
  transition(&d,SND_SOC_BIAS_ON);assert(dev.refs==1);
  transition(&d,SND_SOC_BIAS_ON);assert(dev.refs==1);
  transition(&d,SND_SOC_BIAS_OFF);assert(dev.refs==0);
 }
 assert(dev.gets==101 && dev.puts==101);
}
''')
