"""Exercise the codec overlay's clock start (#48) against a register model: a crystal that is slow to
report ready gets a second try, a failed start fails hw_params and leaves the codec on RCO, and the
per-DAI clock requests stay balanced across ALSA's hw_params/hw_free sequences."""
import unittest
from test_audio import function
from test_cpu_fix02 import overlay
from test_power import run_c


class CodecClock(unittest.TestCase):
    def test_crystal_retry_failure_and_balanced_requests(self):
        source = overlay('sound/soc/codecs/cs43130.c')
        # The upstream order recorded the new source before the crystal started.
        change = function(source, 'cs43130_change_clksrc')
        ext = change[change.index('case CS43130_MCLK_SRC_EXT:'):change.index('case CS43130_MCLK_SRC_PLL:')]
        self.assertLess(ext.index('y2_xtal_start(cs43130)'), ext.index('cs43130->mclk_int_src = CS43130_MCLK_SRC_EXT;'))
        for name in ('cs43130_hw_params', 'cs43130_dsd_hw_params'):
            body = function(source, name)
            self.assertIn('ret = y2_clk_get(component, dai->id, params_rate(params));\n\tif (ret)\n\t\treturn ret;', body)
            self.assertNotIn('clk_req++', body)
        self.assertIn('y2_clk_put(component, dai->id);', function(source, 'cs43130_hw_free'))
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define CS43130_SYS_CLK_CTL_1 0
#define CS43130_PWDN_CTL 1
#define CS43130_INT_MASK_1 2
#define CS43130_MCLK_SRC_SEL_MASK 0x03
#define CS43130_MCLK_SRC_SEL_SHIFT 0
#define CS43130_MCLK_INT_MASK 0x04
#define CS43130_MCLK_INT_SHIFT 2
#define CS43130_XTAL_RDY_INT_MASK 0x10
#define CS43130_XTAL_RDY_INT_SHIFT 4
#define CS43130_PLL_RDY_INT_MASK 0x04
#define CS43130_PLL_RDY_INT_SHIFT 2
#define CS43130_PDN_XTAL_MASK 0x08
#define CS43130_PDN_XTAL_SHIFT 3
#define CS43130_PDN_PLL_MASK 0x04
#define CS43130_PDN_PLL_SHIFT 2
#define CS43130_MCLK_22M 22579200
#define CS43130_MCLK_24M 24576000
enum cs43130_mclk_src_sel { CS43130_MCLK_SRC_EXT = 0, CS43130_MCLK_SRC_PLL, CS43130_MCLK_SRC_RCO };
enum { CS43130_MCLK_24P5 = 0, CS43130_MCLK_22P5 };
#define CS43130_XTAL_UNUSED -1
#define dev_err(d, ...) ((void)(d))
#define dev_warn(d, ...) ((void)(d))
#define dev_dbg(d, ...) ((void)(d))
struct completion { int unused; };
static void reinit_completion(struct completion *c) { (void)c; }
struct mutex { int held; };
static void mutex_lock(struct mutex *m) { assert(!m->held); m->held = 1; }
static void mutex_unlock(struct mutex *m) { assert(m->held); m->held = 0; }
static int test_bit(int bit, unsigned long *map) { return (*map >> bit) & 1; }
static void set_bit(int bit, unsigned long *map) { *map |= 1UL << bit; }
static int test_and_clear_bit(int bit, unsigned long *map) { int was = test_bit(bit, map); *map &= ~(1UL << bit); return was; }
struct regmap { unsigned reg[3]; };
struct cs43130_private {
 struct regmap *regmap; void *dev; struct mutex clk_mutex; int clk_req;
 enum cs43130_mclk_src_sel mclk_int_src; int pll_bypass, xtal_ibias; unsigned mclk, mclk_int;
 struct completion xtal_rdy, pll_rdy;
};
struct snd_soc_component { struct cs43130_private *priv; };
static struct regmap map;
static struct cs43130_private codec = { .regmap = &map, .mclk_int_src = CS43130_MCLK_SRC_RCO,
 .pll_bypass = 1, .xtal_ibias = 2, .mclk = 22579200 };
static struct snd_soc_component component = { &codec };
static struct cs43130_private *snd_soc_component_get_drvdata(struct snd_soc_component *c) { return c->priv; }
static int regmap_update_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 m->reg[r] = (m->reg[r] & ~mask) | v; return 0;
}
static int xtal_slow, xtal_waits, mix, sleeps_5ms, rested;
static void usleep_range(unsigned long lo, unsigned long hi) { (void)hi; if (lo >= 5000) { sleeps_5ms++; rested = 1; } }
static void cs43130_pcm_dsd_mix(bool on, struct regmap *m) { (void)m; mix = on; }
/* The crystal only reports ready while it is powered; `xtal_slow` misses the wait that many times. */
static int cs43130_wait_for_completion(struct cs43130_private *c, struct completion *to_poll, int time) {
 if (to_poll != &c->xtal_rdy) return 0;
 assert(!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK));
 /* 100 ms first; the try after the powered-down rest gets 300 ms. */
 assert(time == (rested ? 300 : 100)); rested = 0;
 assert(!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK));
 xtal_waits++;
 if (xtal_slow) { xtal_slow--; return -ETIMEDOUT; }
 return 0;
}
static int cs43130_set_pll(struct snd_soc_component *component, int pll_id, int source,
                           unsigned int freq_in, unsigned int freq_out) {
 (void)pll_id; (void)source; (void)freq_in;
 component->priv->mclk_int = freq_out; component->priv->pll_bypass = 1; return 0;
}
static int cs43130_change_clksrc(struct snd_soc_component *component, enum cs43130_mclk_src_sel src);
static unsigned long y2_clk_holders;
''' + function(source, 'y2_xtal_start') + function(source, 'y2_clksrc_failed') + change
           + function(source, 'y2_clk_put') + function(source, 'y2_clk_get') + r'''
static unsigned sel(void) { return map.reg[CS43130_SYS_CLK_CTL_1] & CS43130_MCLK_SRC_SEL_MASK; }
static int xtal_off(void) { return !!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK); }
static void on_rco(void) { assert(codec.mclk_int_src == CS43130_MCLK_SRC_RCO && sel() == CS43130_MCLK_SRC_RCO && xtal_off()); }
/* ALSA after a failed hw_params, and at close: hw_free for the DAI either way. */
static void hw_free(int dai) { mutex_lock(&codec.clk_mutex); y2_clk_put(&component, dai); mutex_unlock(&codec.clk_mutex); }
int main(void) {
 map.reg[CS43130_SYS_CLK_CTL_1] = CS43130_MCLK_SRC_RCO; map.reg[CS43130_PWDN_CTL] = 0xff;
 /* Normal start: crystal on the first try. */
 assert(!y2_clk_get(&component, 0, 44100));
 assert(codec.clk_req == 1 && codec.mclk_int_src == CS43130_MCLK_SRC_EXT && sel() == CS43130_MCLK_SRC_EXT && !xtal_off());
 assert(xtal_waits == 1 && !sleeps_5ms);
 hw_free(0); on_rco(); assert(codec.clk_req == 0);
 /* #48: the first wait misses; power down, rest, second try with 300 ms. */
 xtal_waits = 0; xtal_slow = 1;
 assert(!y2_clk_get(&component, 0, 44100));
 assert(xtal_waits == 2 && sleeps_5ms == 1 && codec.mclk_int_src == CS43130_MCLK_SRC_EXT && sel() == CS43130_MCLK_SRC_EXT);
 hw_free(0); on_rco();
 /* Both tries miss: hw_params fails, the codec stays on RCO with XTAL/PLL down, nothing is held. */
 xtal_waits = 0; xtal_slow = 2;
 assert(y2_clk_get(&component, 0, 44100) == -ETIMEDOUT);
 on_rco(); assert(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_PLL_MASK);
 assert(codec.clk_req == 0 && !y2_clk_holders);
 hw_free(0); /* ALSA's hw_free after the failure must not take a request it never got */
 assert(codec.clk_req == 0);
 /* The next open starts the clock again (upstream: mclk_int_src said EXT, so it was skipped). */
 xtal_waits = 0;
 assert(!y2_clk_get(&component, 0, 48000));
 assert(codec.mclk_int == CS43130_MCLK_24M && sel() == CS43130_MCLK_SRC_EXT && xtal_waits == 1);
 /* A repeated hw_params at a fitting rate keeps the running clock; a new family restarts it. */
 assert(!y2_clk_get(&component, 0, 96000) && xtal_waits == 1 && codec.clk_req == 1);
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 2 && codec.clk_req == 1 && codec.mclk_int == CS43130_MCLK_22M);
 /* A second DAI shares the clock; the last hw_free returns to RCO. */
 assert(!y2_clk_get(&component, 1, 44100) && codec.clk_req == 2 && mix);
 hw_free(0); assert(codec.clk_req == 1 && sel() == CS43130_MCLK_SRC_EXT);
 hw_free(0); assert(codec.clk_req == 1);
 hw_free(1); on_rco(); assert(codec.clk_req == 0 && !mix && !y2_clk_holders);
 puts("codec clock ok");
 return 0;
}
''')


if __name__ == '__main__':
    unittest.main()
