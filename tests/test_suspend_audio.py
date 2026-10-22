"""Exercise the real codec PM callbacks against reset/rail/cache fault models."""
import unittest
from test_audio import function
from test_cpu_fix02 import overlay
from test_power import run_c


class CodecSleepUnwind(unittest.TestCase):
    def test_reset_state_and_rail_references_survive_suspend_refusal(self):
        source = overlay('sound/soc/codecs/cs43130.c')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#include <string.h>
#define CS43130_NUM_SUPPLIES 5
#define CS43130_XTAL_UNUSED 0
#define CS43130_INT_MASK_1 0
#define CS43130_XTAL_ERR_INT 1
#define CS43130_XTAL_ERR_INT_SHIFT 0
#define PM_SUSPEND_ON 0
#define CS43130_PWDN_CTL 3
#define CS43130_PDN_XTAL_MASK 0x08
#define CS43130_PDN_XTAL_SHIFT 3
static bool y2_xtal_on;
static unsigned xtal_starts;
static unsigned pm_suspend_target_state = 3;
static unsigned long jiffies = 100;
struct regmap { unsigned cache[4], hw[4]; bool only, dirty; };
struct cs43130_private { struct regmap *regmap; void *reset_gpio; unsigned xtal_ibias; int supplies[5]; };
struct device { struct cs43130_private *data; };
static struct regmap map;
static struct cs43130_private codec = { &map, 0, 1, {1,1,1,1,1} };
static struct device dev = { &codec };
static bool reset_high;
static int enable_error, disable_error, sync_error, xtal_error;
static unsigned enables, disables, syncs, notes;
static bool cleanup_noted;
static char last_note[32]; static int last_error;
static void *dev_get_drvdata(struct device *d) { return d->data; }
static void y2_pm_note(const char *s,int e) {
 notes++;strcpy(last_note,s);last_error=e;
 if(!strcmp(s,"cs_cleanup_fail") && e==-EBUSY)cleanup_noted=true;
}
#define dev_err(...) do {} while(0)
static void regcache_cache_only(struct regmap *m,bool v) { m->only=v; }
static void regcache_mark_dirty(struct regmap *m) { m->dirty=true; }
static void gpiod_set_value_cansleep(void *g,int v) {
 if(!g)return;
 reset_high=v;if(!v)memset(map.hw,0,sizeof(map.hw));
}
static void usleep_range(unsigned a,unsigned b) { assert(reset_high && a==2000 && b==2050); }
static int regcache_sync(struct regmap *m) {
 syncs++;assert(!m->only && reset_high);
 if(sync_error)return sync_error;
 memcpy(m->hw,m->cache,sizeof(m->hw));m->dirty=false;return 0;
}
static int regmap_update_bits(struct regmap *m,unsigned r,unsigned mask,unsigned val) {
 if(!val && xtal_error)return xtal_error;
 m->cache[r]=(m->cache[r]&~mask)|val;
 if(!m->only)m->hw[r]=m->cache[r];return 0;
}
static int regulator_bulk_enable(unsigned n,int *rails) {
 enables++;if(enable_error)return enable_error;
 for(unsigned i=0;i<n;i++)rails[i]++;return 0;
}
static int regulator_bulk_disable(unsigned n,int *rails) {
 disables++;if(disable_error)return disable_error; /* core rolls back partial disable */
 for(unsigned i=0;i<n;i++){assert(rails[i]);rails[i]--;}return 0;
}
/* As the overlay's: clears PDN_XTAL on the chip (after the replay). */
static void y2_xtal_power_on(struct cs43130_private *c) {
 assert(!c->regmap->only && reset_high);
 xtal_starts++;y2_xtal_on=true;c->regmap->cache[CS43130_PWDN_CTL]&=~CS43130_PDN_XTAL_MASK;
 c->regmap->hw[CS43130_PWDN_CTL]=c->regmap->cache[CS43130_PWDN_CTL];
}
static int pm_runtime_force_suspend(struct device *d);
static int pm_runtime_force_resume(struct device *d);
''' + ''.join(function(source, n) for n in ('cs43130_pm_note',
            'cs43130_restore_cache', 'cs43130_runtime_suspend', 'cs43130_runtime_resume',
            'cs43130_system_suspend', 'cs43130_system_resume')) + r'''
static int pm_runtime_force_suspend(struct device *d){return cs43130_runtime_suspend(d);}
static int pm_runtime_force_resume(struct device *d){return cs43130_runtime_resume(d);}
static void init(void) {
 memset(&map,0,sizeof(map));map.cache[1]=map.hw[1]=0x5a;map.cache[2]=map.hw[2]=0x86;
 for(unsigned i=0;i<5;i++)codec.supplies[i]=1;
 reset_high=true;codec.reset_gpio=&reset_high;
 enable_error=disable_error=sync_error=xtal_error=0;cleanup_noted=false;
 enables=disables=syncs=notes=xtal_starts=0;pm_suspend_target_state=3;
}
int main(void) {
 init();assert(!cs43130_system_suspend(&dev));
 assert(!reset_high && map.only && map.dirty);
 for(unsigned i=0;i<5;i++)assert(!codec.supplies[i]);
 assert(!cs43130_system_resume(&dev));
 assert(reset_high && !map.only && map.hw[1]==0x5a && map.hw[2]==0x86 && !(map.hw[0]&1));
 for(unsigned i=0;i<5;i++)assert(codec.supplies[i]==1);
 assert(!strcmp(last_note,"cs_force_done") && !last_error);

 init();disable_error=-EBUSY;
 assert(cs43130_system_suspend(&dev)==-EBUSY);
 assert(reset_high && !map.only && syncs==1 && map.hw[1]==0x5a && !(map.hw[0]&1));
 assert(enables==0); /* rollback must not add a second rail reference */
 for(unsigned i=0;i<5;i++)assert(codec.supplies[i]==1);
 assert(!strcmp(last_note,"cs_sleep_done") && last_error==-EBUSY);

 init();disable_error=-EBUSY;sync_error=-EIO;
 assert(cs43130_system_suspend(&dev)==-EIO && last_error==-EIO);
 assert(!enables);for(unsigned i=0;i<5;i++)assert(codec.supplies[i]==1);

 for(unsigned fault=0;fault<3;fault++) {
  init();assert(!cs43130_system_suspend(&dev));
  if(fault==0)enable_error=-EIO;else if(fault==1)sync_error=-EIO;else xtal_error=-EIO;
  assert(cs43130_system_resume(&dev)==-EIO);
  assert(!reset_high && map.only && last_error==-EIO);
  for(unsigned i=0;i<5;i++)assert(!codec.supplies[i]);
  assert(!xtal_starts); /* a failed resume leaves the crystal down */
 }
 init();assert(!cs43130_system_suspend(&dev));
 sync_error=-EIO;disable_error=-EBUSY;
 assert(cs43130_system_resume(&dev)==-EIO && cleanup_noted);
 assert(!reset_high && map.only && last_error==-EIO);
 for(unsigned i=0;i<5;i++)assert(codec.supplies[i]==1);
 init();pm_suspend_target_state=PM_SUSPEND_ON;y2_xtal_on=true;
 assert(!cs43130_runtime_suspend(&dev));
 /* #51: the crystal stops with the rails and stays down in the replay... */
 assert(!y2_xtal_on && (map.cache[CS43130_PWDN_CTL]&CS43130_PDN_XTAL_MASK));
 /* ...and, as stock, starts with the codec once the replay is done (#54). */
 assert(!cs43130_runtime_resume(&dev) && xtal_starts==1 && y2_xtal_on && !(map.hw[CS43130_PWDN_CTL]&CS43130_PDN_XTAL_MASK));
 assert(!notes); /* ordinary audio runtime traffic does not write sleep traces */
 /* A codec without a crystal never starts one. */
 init();pm_suspend_target_state=PM_SUSPEND_ON;codec.xtal_ibias=CS43130_XTAL_UNUSED;
 assert(!cs43130_runtime_suspend(&dev) && !cs43130_runtime_resume(&dev) && !xtal_starts);
 codec.xtal_ibias=1;
}
''')
