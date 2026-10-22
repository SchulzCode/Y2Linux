"""Exercise the codec overlay's clock start (#48, #51, #52, #54) against a register model. The crystal runs as stock
runs it: it starts when the codec powers up, stops only with the rails, a stream waits for its ready flag until it
has run 1 s and then uses it anyway (stock never reads the flag), flags seen later are counted and never restart it,
the running clock stays selected between streams (#52: stock never switches), a PLL that never locks fails hw_params and
leaves the codec on RCO with the crystal running, and the per-DAI clock requests stay balanced across ALSA's hw_params/hw_free sequences."""
import unittest
from test_audio import function
from test_cpu_fix02 import overlay
from test_power import run_c


class CodecClock(unittest.TestCase):
    def test_crystal_runs_as_stock_and_balanced_requests(self):
        source = overlay('sound/soc/codecs/cs43130.c')
        # The upstream order recorded the new source before the crystal started.
        change = function(source, 'cs43130_change_clksrc')
        ext = change[change.index('case CS43130_MCLK_SRC_EXT:'):change.index('case CS43130_MCLK_SRC_PLL:')]
        self.assertLess(ext.index('y2_xtal_start(cs43130);'), ext.index('cs43130->mclk_int_src = CS43130_MCLK_SRC_EXT;'))
        for name in ('cs43130_hw_params', 'cs43130_dsd_hw_params'):
            body = function(source, name)
            self.assertIn('ret = y2_clk_get(component, dai->id, params_rate(params));\n\tif (ret)\n\t\treturn ret;', body)
            self.assertNotIn('clk_req++', body)
        self.assertIn('y2_clk_put(component, dai->id);', function(source, 'cs43130_hw_free'))
        # Runtime suspend stops the crystal with the rails and leaves it down in the cache; runtime resume starts it
        # after the cache replay, as stock cs43131_hw_init powers it with the codec.
        suspend = function(source, 'cs43130_runtime_suspend')
        self.assertLess(suspend.index('y2_xtal_on = false;'), suspend.index('regulator_bulk_disable'))
        resume = function(source, 'cs43130_runtime_resume')
        self.assertLess(resume.index('cs43130_restore_cache(cs43130);'), resume.index('y2_xtal_power_on(cs43130);'))
        # #52: the card keeps ASoC's 5-s power-down delay; skips no longer cycle the amplifier and charge pump.
        from pathlib import Path
        card = (Path(__file__).resolve().parents[1] / 'kernel/audio/y2-cs43131.c').read_text()
        self.assertNotIn('ignore_pmdown_time = 1', card)
        # The codec keeps power for 5 s after a stream (track changes), not 100 ms.
        self.assertIn('pm_runtime_set_autosuspend_delay(cs43130->dev, 5000);', source)
        # Nothing but runtime suspend powers the crystal down any more, and nothing cycles the rails.
        self.assertEqual(source.count('1 << CS43130_PDN_XTAL_SHIFT'), 3)  # two XTAL_UNUSED branches, runtime suspend
        for gone in ('y2_codec_cycle', 'y2_rails_off', 'y2_xtal_try', 'y2_xtal_may_cycle'):
            self.assertNotIn(gone, source)
        preamble = source[source.index('#define Y2_XTAL_WAIT_MS'):source.index('static void y2_xtal_boost(')]
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define CS43130_SYS_CLK_CTL_1 0
#define CS43130_PWDN_CTL 1
#define CS43130_INT_MASK_1 2
#define CS43130_INT_STATUS_1 3
#define CS43130_CRYSTAL_SET 4
#define CS43130_MCLK_SRC_SEL_MASK 0x03
#define CS43130_MCLK_SRC_SEL_SHIFT 0
#define CS43130_MCLK_INT_MASK 0x04
#define CS43130_MCLK_INT_SHIFT 2
#define CS43130_XTAL_RDY_INT_MASK 0x10
#define CS43130_XTAL_RDY_INT_SHIFT 4
#define CS43130_XTAL_RDY_INT 0x10
#define CS43130_XTAL_ERR_INT 0x08
#define CS43130_PLL_RDY_INT_MASK 0x04
#define CS43130_PLL_RDY_INT_SHIFT 2
#define CS43130_PDN_XTAL_MASK 0x08
#define CS43130_PDN_XTAL_SHIFT 3
#define CS43130_PDN_PLL_MASK 0x04
#define CS43130_PDN_PLL_SHIFT 2
#define CS43130_XTAL_IBIAS_MASK 0x07
#define CS43130_XTAL_IBIAS_15UA 2
#define CS43130_XTAL_IBIAS_12_5UA 4
#define CS43130_MCLK_22M 22579200
#define CS43130_MCLK_24M 24576000
enum cs43130_mclk_src_sel { CS43130_MCLK_SRC_EXT = 0, CS43130_MCLK_SRC_PLL, CS43130_MCLK_SRC_RCO };
enum { CS43130_MCLK_24P5 = 0, CS43130_MCLK_22P5 };
#define CS43130_XTAL_UNUSED -1
static int warnings;
#define dev_err(d, ...) ((void)(d))
#define dev_warn(d, ...) ((void)(d), warnings++)
#define dev_warn_ratelimited(d, ...) ((void)(d), warnings++)
#define dev_dbg(d, ...) ((void)(d))
/* One jiffy per millisecond. */
static unsigned long jiffies;
static unsigned int jiffies_to_msecs(unsigned long j) { return j; }
struct completion { int unused; };
static void reinit_completion(struct completion *c) { (void)c; }
struct mutex { int held; };
static void mutex_lock(struct mutex *m) { assert(!m->held); m->held = 1; }
static void mutex_unlock(struct mutex *m) { assert(m->held); m->held = 0; }
static int test_bit(int bit, unsigned long *map) { return (*map >> bit) & 1; }
static void set_bit(int bit, unsigned long *map) { *map |= 1UL << bit; }
static int test_and_clear_bit(int bit, unsigned long *map) { int was = test_bit(bit, map); *map &= ~(1UL << bit); return was; }
struct regmap { unsigned reg[5]; };
struct cs43130_private {
 struct regmap *regmap; void *dev; struct mutex clk_mutex; int clk_req;
 enum cs43130_mclk_src_sel mclk_int_src; int pll_bypass, xtal_ibias; unsigned mclk, mclk_int;
 struct completion xtal_rdy, pll_rdy;
};
struct snd_soc_component { struct cs43130_private *priv; };
static struct regmap map;
static struct cs43130_private codec = { .regmap = &map, .mclk_int_src = CS43130_MCLK_SRC_RCO,
 .pll_bypass = 1, .xtal_ibias = CS43130_XTAL_IBIAS_12_5UA, .mclk = 22579200 };
static struct snd_soc_component component = { &codec };
static struct cs43130_private *snd_soc_component_get_drvdata(struct snd_soc_component *c) { return c->priv; }
/* The chip: when the crystal was powered, how long it takes to report ready, how often it was stopped. */
static unsigned long chip_on_at; static unsigned ready_after; static int stops, starts;
static int regmap_update_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 unsigned old = m->reg[r];
 m->reg[r] = (m->reg[r] & ~mask) | v;
 if (r == CS43130_PWDN_CTL) {
  if (!(old & CS43130_PDN_XTAL_MASK) && (m->reg[r] & CS43130_PDN_XTAL_MASK)) stops++;
  if ((old & CS43130_PDN_XTAL_MASK) && !(m->reg[r] & CS43130_PDN_XTAL_MASK)) { starts++; chip_on_at = jiffies; }
 }
 return 0;
}
static int regmap_write_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 /* The bias changes only with the crystal down. */
 if (r == CS43130_CRYSTAL_SET) assert(m->reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK);
 return regmap_update_bits(m, r, mask, v);
}
static int regmap_read(struct regmap *m, unsigned r, unsigned *v) {
 *v = m->reg[r]; if (r == CS43130_INT_STATUS_1) m->reg[r] = 0; return 0;
}
static int regmap_read_bypassed(struct regmap *m, unsigned r, unsigned *v) { *v = m->reg[r]; return 0; }
static int mix, xtal_waits, budget, pll_fail;
static void usleep_range(unsigned long lo, unsigned long hi) { (void)lo; (void)hi; }
static void cs43130_pcm_dsd_mix(bool on, struct regmap *m) { (void)m; mix = on; }
/* The crystal reports ready `ready_after` ms after it was powered; a wait advances the clock. */
static int cs43130_wait_for_completion(struct cs43130_private *c, struct completion *to_poll, int time) {
 if (to_poll == &c->pll_rdy) return pll_fail ? -ETIMEDOUT : 0;
 assert(!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK));
 budget = time; xtal_waits++;
 if (jiffies - chip_on_at + time >= ready_after) {
  if (jiffies - chip_on_at < ready_after) jiffies = chip_on_at + ready_after;
  return 0;
 }
 jiffies += time; return -ETIMEDOUT;
}
static int use_pll, pll_sets;
static int cs43130_set_pll(struct snd_soc_component *component, int pll_id, int source,
                           unsigned int freq_in, unsigned int freq_out) {
 (void)pll_id; (void)source; (void)freq_in; pll_sets++;
 component->priv->mclk_int = freq_out; component->priv->pll_bypass = !use_pll; return 0;
}
static int cs43130_change_clksrc(struct snd_soc_component *component, enum cs43130_mclk_src_sel src);
static unsigned long y2_clk_holders;
''' + preamble + function(source, 'y2_xtal_boost') + function(source, 'y2_xtal_power_on')
           + function(source, 'y2_xtal_ran_ms') + function(source, 'y2_xtal_report') + function(source, 'y2_xtal_start')
           + function(source, 'y2_clksrc_failed') + change
           + function(source, 'y2_clk_put') + function(source, 'y2_clk_get') + r'''
static unsigned sel(void) { return map.reg[CS43130_SYS_CLK_CTL_1] & CS43130_MCLK_SRC_SEL_MASK; }
static int xtal_off(void) { return !!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK); }
static void on_rco(void) { assert(codec.mclk_int_src == CS43130_MCLK_SRC_RCO && sel() == CS43130_MCLK_SRC_RCO); }
static void on_ext(void) { assert(codec.mclk_int_src == CS43130_MCLK_SRC_EXT && sel() == CS43130_MCLK_SRC_EXT); }
static void hw_free(int dai) { mutex_lock(&codec.clk_mutex); y2_clk_put(&component, dai); mutex_unlock(&codec.clk_mutex); }
/* What runtime PM does to the crystal (the rails themselves are not modelled). */
static void runtime_suspend(void) {
 y2_xtal_on = false; regmap_update_bits(&map, CS43130_PWDN_CTL, CS43130_PDN_XTAL_MASK, 1 << CS43130_PDN_XTAL_SHIFT);
 regmap_update_bits(&map, CS43130_PWDN_CTL, CS43130_PDN_PLL_MASK, 1 << CS43130_PDN_PLL_SHIFT);
 map.reg[CS43130_SYS_CLK_CTL_1] = CS43130_MCLK_SRC_RCO | CS43130_MCLK_22P5 << CS43130_MCLK_INT_SHIFT;
 codec.mclk_int_src = CS43130_MCLK_SRC_RCO; map.reg[CS43130_INT_STATUS_1] = 0; }
static void runtime_resume(void) { y2_xtal_power_on(&codec); }
int main(void) {
 map.reg[CS43130_SYS_CLK_CTL_1] = CS43130_MCLK_SRC_RCO; map.reg[CS43130_PWDN_CTL] = 0xff;
 /* The codec kept power since probe: the first stream powers the crystal and waits for it. */
 ready_after = 20;
 assert(!y2_clk_get(&component, 0, 44100));
 assert(codec.clk_req == 1 && codec.mclk_int_src == CS43130_MCLK_SRC_EXT && sel() == CS43130_MCLK_SRC_EXT && !xtal_off());
 assert(starts == 1 && xtal_waits == 1 && budget == 1000 && y2_xtal_ready && !y2_xtal_unconfirmed);
 /* A crystal that starts keeps the stock bias. */
 assert(!y2_xtal_boosted && map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_12_5UA);
 /* #51/#52: close and reopen (Next) keep the crystal running and selected: no clock work at all. */
 hw_free(0); on_ext(); assert(codec.clk_req == 0 && !xtal_off() && y2_xtal_on);
 pll_sets = 0;
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 1 && pll_sets == 0); on_ext();
 hw_free(0);
 /* #52: a sticky crystal error while it runs is counted, never acted on (stock masks it). */
 map.reg[CS43130_INT_STATUS_1] = CS43130_XTAL_ERR_INT | 0x41;
 assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_errors == 1 && starts == 1 && !stops && !xtal_off());
 assert(sel() == CS43130_MCLK_SRC_EXT && xtal_waits == 1);
 hw_free(0);
 /* Runtime suspend stops the crystal; runtime resume starts it before the stream, which waits only for the rest. */
 runtime_suspend(); assert(stops == 1);
 runtime_resume(); assert(starts == 2 && !xtal_off() && !y2_xtal_ready);
 jiffies += 600; ready_after = 700;
 assert(!y2_clk_get(&component, 0, 44100) && budget == 400 && y2_xtal_ready && !y2_xtal_unconfirmed);
 hw_free(0);
 /* #54: a crystal that does not report ready within 1 s is used anyway, as stock; it is never stopped. */
 runtime_suspend(); runtime_resume(); xtal_waits = 0; ready_after = 5000; warnings = 0;
 assert(!y2_clk_get(&component, 0, 44100));
 assert(xtal_waits == 1 && budget == 1000 && y2_xtal_assumed && !y2_xtal_ready && y2_xtal_unconfirmed == 1 && warnings == 1);
 assert(sel() == CS43130_MCLK_SRC_EXT && codec.clk_req == 1 && !xtal_off() && stops == 2);
 hw_free(0);
 /* The next open does not wait or report again; a ready flag that comes later is recorded. */
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 1 && y2_xtal_unconfirmed == 1 && warnings == 1);
 hw_free(0);
 map.reg[CS43130_INT_STATUS_1] = CS43130_XTAL_RDY_INT;
 assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_ready && warnings == 2 && xtal_waits == 1);
 hw_free(0);
 /* A fresh power-up waits again (a new crystal start). */
 runtime_suspend(); runtime_resume(); ready_after = 3;
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 2 && y2_xtal_ready && !y2_xtal_assumed);
 hw_free(0);
 /* Test boost: the bias changes at the next power-up, never under a running crystal. */
 y2_xtal_boost(&codec); assert(y2_xtal_boosted && map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_12_5UA);
 runtime_suspend(); runtime_resume(); assert(map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_15UA);
 /* 48 kHz through the PLL; a PLL that never locks fails hw_params, leaves RCO, keeps the crystal, holds nothing. */
 use_pll = 1; pll_fail = 1; stops = 0;
 assert(y2_clk_get(&component, 0, 48000) == -ETIMEDOUT);
 on_rco(); assert((map.reg[CS43130_PWDN_CTL] & CS43130_PDN_PLL_MASK) && !xtal_off() && !stops && y2_xtal_on);
 assert(codec.clk_req == 0 && !y2_clk_holders);
 hw_free(0); /* ALSA's hw_free after the failure must not take a request it never got */
 assert(codec.clk_req == 0);
 /* The retry locks. */
 pll_fail = 0;
 assert(!y2_clk_get(&component, 0, 48000) && codec.mclk_int_src == CS43130_MCLK_SRC_PLL && codec.mclk_int == CS43130_MCLK_24M);
 /* The PLL keeps running after the stream; the next 48 kHz stream reuses it. */
 hw_free(0); assert(codec.mclk_int_src == CS43130_MCLK_SRC_PLL && !xtal_off());
 pll_sets = 0;
 assert(!y2_clk_get(&component, 0, 48000) && pll_sets == 0 && codec.mclk_int_src == CS43130_MCLK_SRC_PLL);
 hw_free(0);
 /* A 44.1 kHz stream after it switches back to the crystal. */
 use_pll = 0;
 assert(!y2_clk_get(&component, 0, 44100) && pll_sets == 1); on_ext();
 hw_free(0);
 /* Runtime suspend returns the codec to RCO with the PLL off; the next open selects the crystal again. */
 runtime_suspend(); on_rco(); runtime_resume();
 assert(!y2_clk_get(&component, 0, 44100)); on_ext();
 hw_free(0);
 /* A repeated hw_params at a fitting rate keeps the clock; a new family switches. */
 assert(!y2_clk_get(&component, 0, 48000) && codec.clk_req == 1);
 assert(!y2_clk_get(&component, 0, 96000) && codec.clk_req == 1);
 assert(!y2_clk_get(&component, 0, 44100) && codec.clk_req == 1 && codec.mclk_int == CS43130_MCLK_22M);
 /* A second DAI shares the clock; the last hw_free returns to RCO. */
 assert(!y2_clk_get(&component, 1, 44100) && codec.clk_req == 2 && mix);
 hw_free(0); assert(codec.clk_req == 1 && sel() == CS43130_MCLK_SRC_EXT);
 hw_free(0); assert(codec.clk_req == 1);
 hw_free(1); on_ext(); assert(codec.clk_req == 0 && !mix && !y2_clk_holders && !xtal_off());
 puts("codec clock ok");
 return 0;
}''')


if __name__ == '__main__':
    unittest.main()
