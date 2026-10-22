"""Exercise the codec overlay's clock start (#48, #51, #52, #54) against a register model. The crystal runs as stock
runs it: it starts when the codec powers up, stops only with the rails, a stream waits for its ready flag until it
has run 1 s and then uses it anyway (stock never reads the flag), flags seen later are counted and never restart it,
the running clock stays selected between streams (#52: stock never switches), power-up selects the crystal with
the PLL and CLKOUT on as stock cs43131_hw_init does (#54), rates the crystal does not clock are refused (the PLL is never
used, as stock: #52's crystal stopped at every 48 kHz start), a crystal error at a stream start power-cycles the codec,
a probe that finds the rails on starts the codec cold, and the per-DAI clock requests stay balanced across ALSA's
hw_params/hw_free sequences. #52/#54 on 8.24: each power-up starts the crystal with the whole chip powered (stock
PWDN_CTL=0) and cuts the rails for another try when it flags an error or no ready flag, three tries in all; the first
stream after a failed power-up adds one cold start, bounded."""
import re
import unittest
from pathlib import Path
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
        self.assertLess(resume.index('y2_codec_up(cs43130);'), resume.index('y2_xtal_power_on(cs43130, true);'))
        self.assertIn('cs43130_restore_cache(cs43130);', function(source, 'y2_codec_up'))
        # #52/#54, as stock: the crystal starts at probe, and the codec keeps power two idle minutes.
        self.assertIn('pm_runtime_set_autosuspend_delay(cs43130->dev, 120000);', source)
        probe = function(source, 'cs43130_i2c_probe')
        self.assertLess(probe.index('y2_xtal_power_on(cs43130, true);'), probe.index('pm_runtime_enable(cs43130->dev);'))
        # #52/#54: y2_hp/hold keeps the codec (and its crystal) powered while Reborn's screen is on, as stock keeps
        # it from screen-on to screen-off; one runtime PM reference at most, released through autosuspend.
        hold = function(source, 'hold_store')
        self.assertIn('if (on && !y2_hold) {\n\t\tret = pm_runtime_resume_and_get(dev);\n\t\ty2_hold = !ret;', hold)
        self.assertIn('} else if (!on && y2_hold) {\n\t\tpm_runtime_mark_last_busy(dev);\n\t\tpm_runtime_put_autosuspend(dev);', hold)
        self.assertIn('&dev_attr_hold.attr,', source)
        # The rails come up as stock cs43131_on: GPIO20, 18, 15 back to back, then one 50 ms settle.
        dts = (Path(__file__).resolve().parents[1]/'kernel/dts/innioasis-y2.dts').read_text()
        delays = re.findall(r'regulator-name = "cs43131-en(\d+)";.*?startup-delay-us = <(\d+)>', dts, re.S)
        self.assertEqual(delays, [('20', '0'), ('18', '0'), ('15', '50000')])
        # The ready flag is read every 20 ms, not every 1 ms (stock never reads it during the start).
        self.assertIn('Y2_XTAL_POLL_US, ms * 1000);', function(source, 'y2_xtal_wait'))
        # Nothing but runtime suspend powers the crystal down (also inside the #52 power cycle).
        self.assertEqual(source.count('1 << CS43130_PDN_XTAL_SHIFT'), 3)  # two XTAL_UNUSED branches, runtime suspend
        for gone in ('y2_rails_off', 'y2_xtal_try', 'y2_xtal_may_cycle'):
            self.assertNotIn(gone, source)
        rails = function(source, 'y2_rails_cycle')
        self.assertLess(rails.index('cs43130_runtime_suspend(cs43130->dev);'), rails.index('msleep(Y2_CODEC_OFF_MS);'))
        self.assertLess(rails.index('msleep(Y2_CODEC_OFF_MS);'), rails.index('y2_codec_up(cs43130);'))
        cycle = function(source, 'y2_codec_cycle')
        self.assertLess(cycle.index('y2_rails_cycle(cs43130);'), cycle.index('y2_xtal_power_on(cs43130, true);'))
        # The start-rate test switches and the cycle trigger exist for the reporters.
        self.assertIn('module_param(y2_xtal_full_power, bool, 0644);', source)
        self.assertIn('module_param(y2_xtal_quiet_start, bool, 0644);', source)
        self.assertIn('if (!sysfs_streq(buf, "cycle"))', function(source, 'xtal_store'))
        # #52: a restart resets the SoC, not the PMIC: rails found on at probe go off for Y2_CODEC_OFF_MS first.
        cold = probe[probe.index('y2_cold_start = regulator_is_enabled(cs43130->supplies[0].consumer) > 0;'):]
        order = ['regulator_bulk_enable(', 'if (y2_cold_start) {', 'regulator_bulk_disable(', 'msleep(Y2_CODEC_OFF_MS);',
                 'regulator_bulk_enable(', 'cs43130->reset_gpio = devm_gpiod_get_optional(']
        at = 0
        for step in order:
            at = cold.index(step, at) + 1
        # #52: no PLL, ever: rates the crystal does not clock fail before any clock work.
        get = function(source, 'y2_clk_get')
        self.assertLess(get.index('if (CS43130_MCLK_22M % rate) {'), get.index('mutex_lock(&cs43130->clk_mutex);'))
        afe = (Path(__file__).resolve().parents[1]/'kernel/audio/mt6582-afe.c').read_text()
        self.assertNotIn('SNDRV_PCM_RATE_48000', afe)
        self.assertIn('.rate_min = 44100, .rate_max = 44100,', afe)
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
#define CS43130_PDN_CLKOUT_MASK 0x02
#define CS43130_XTAL_IBIAS_MASK 0x07
#define CS43130_XTAL_IBIAS_15UA 2
#define CS43130_XTAL_IBIAS_12_5UA 4
#define CS43130_MCLK_22M 22579200
#define CS43130_MCLK_24M 24576000
enum cs43130_mclk_src_sel { CS43130_MCLK_SRC_EXT = 0, CS43130_MCLK_SRC_PLL, CS43130_MCLK_SRC_RCO };
enum { CS43130_MCLK_24P5 = 0, CS43130_MCLK_22P5 };
#define CS43130_XTAL_UNUSED -1
static int warnings;
#define module_param(name, type, perm)
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
struct cs43130_private;
static int y2_rails_cycle(struct cs43130_private *cs43130);
struct cs43130_private {
 struct regmap *regmap; void *dev; struct mutex clk_mutex; int clk_req; bool has_irq_line;
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
/* The next `bad_starts` crystal starts flag XTAL_ERR and never report ready (#52/#54). */
static int bad_starts, this_bad; static unsigned pwdn_at_start; static int bias_writes;
static int regmap_update_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 unsigned old = m->reg[r];
 m->reg[r] = (m->reg[r] & ~mask) | v;
 if (r == CS43130_PWDN_CTL) {
  if (!(old & CS43130_PDN_XTAL_MASK) && (m->reg[r] & CS43130_PDN_XTAL_MASK)) stops++;
  if ((old & CS43130_PDN_XTAL_MASK) && !(m->reg[r] & CS43130_PDN_XTAL_MASK)) {
   starts++; chip_on_at = jiffies; pwdn_at_start = m->reg[r];
   this_bad = bad_starts > 0; if (this_bad) bad_starts--;
  }
 }
 return 0;
}
static int regmap_write_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 /* The bias changes only with the crystal down. */
 if (r == CS43130_CRYSTAL_SET) { assert(m->reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK); bias_writes++; }
 return regmap_update_bits(m, r, mask, v);
}
static int regmap_write(struct regmap *m, unsigned r, unsigned v) { return regmap_update_bits(m, r, 0xff, v); }
static int regmap_read(struct regmap *m, unsigned r, unsigned *v) {
 *v = m->reg[r]; if (r == CS43130_INT_STATUS_1) m->reg[r] = 0; return 0;
}
static int regmap_read_bypassed(struct regmap *m, unsigned r, unsigned *v) { *v = m->reg[r]; return 0; }
static int mix, xtal_waits, budget, pll_fail;
static void usleep_range(unsigned long lo, unsigned long hi) { (void)lo; (void)hi; }
static void msleep(unsigned ms) { jiffies += ms; }
static void cs43130_pcm_dsd_mix(bool on, struct regmap *m) { (void)m; mix = on; }
static int cs43130_wait_for_completion(struct cs43130_private *c, struct completion *to_poll, int time) {
 (void)c; (void)time; assert(to_poll == &c->pll_rdy); return pll_fail ? -ETIMEDOUT : 0;
}
static unsigned int y2_xtal_errors;
/* The crystal reports ready `ready_after` ms after it was powered, or an error at once on a bad start (counted, as
 * the real wait does); a wait advances the clock. */
static int y2_xtal_wait(struct cs43130_private *c, int time) {
 (void)c;
 assert(!(map.reg[CS43130_PWDN_CTL] & CS43130_PDN_XTAL_MASK));
 budget = time; xtal_waits++;
 if (this_bad) { y2_xtal_errors++; return -EIO; }
 if (jiffies - chip_on_at + time >= ready_after) {
  if (jiffies - chip_on_at < ready_after) jiffies = chip_on_at + ready_after;
  return 0;
 }
 jiffies += time; return -ETIMEDOUT;
}
static int use_pll, pll_sets;
static unsigned sel(void);
static int cs43130_set_pll(struct snd_soc_component *component, int pll_id, int source,
                           unsigned int freq_in, unsigned int freq_out) {
 (void)pll_id; (void)source; (void)freq_in; pll_sets++;
 /* set_pll() stops the PLL: never while it is the selected MCLK (control port down, 8.22 test build). */
 assert(sel() != CS43130_MCLK_SRC_PLL);
 component->priv->mclk_int = freq_out; component->priv->pll_bypass = !use_pll; return 0;
}
static int cs43130_change_clksrc(struct snd_soc_component *component, enum cs43130_mclk_src_sel src);
static unsigned long y2_clk_holders;
static void runtime_suspend(void);
static void runtime_resume(void);
static int cs43130_runtime_suspend(void *dev) { (void)dev; runtime_suspend(); return 0; }
static int y2_codec_up(struct cs43130_private *c) { (void)c; return 0; }
''' + preamble + function(source, 'y2_xtal_boost') + function(source, 'y2_xtal_ran_ms')
           + function(source, 'y2_xtal_attempt') + function(source, 'y2_xtal_power_on')
           + function(source, 'y2_xtal_report') + function(source, 'y2_rails_cycle')
           + function(source, 'y2_codec_cycle') + function(source, 'y2_xtal_start')
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
static void runtime_resume(void) { y2_xtal_stream_cycles = 0; y2_xtal_power_on(&codec, true); }
int main(void) {
 (void)y2_cold_start; /* probe only */
 map.reg[CS43130_SYS_CLK_CTL_1] = CS43130_MCLK_SRC_RCO; map.reg[CS43130_PWDN_CTL] = 0xfe; /* reset values */
 /* The codec kept power since probe: the first stream powers the crystal, as stock with the whole chip on, waits for
  * it, and hands the other blocks back to DAPM. */
 ready_after = 20;
 assert(!y2_clk_get(&component, 0, 44100));
 assert(codec.clk_req == 1 && codec.mclk_int_src == CS43130_MCLK_SRC_EXT && sel() == CS43130_MCLK_SRC_EXT && !xtal_off());
 assert(starts == 1 && pwdn_at_start == 0 && (map.reg[CS43130_PWDN_CTL] & 0xf0) == 0xf0);
 assert(xtal_waits == 1 && budget == 80 && y2_xtal_ready && !y2_xtal_unconfirmed && y2_xtal_ready_ms == 20);
 assert(y2_xtal_tries == 1 && y2_xtal_start_ms == 20 && !warnings);
 /* A crystal that starts keeps the stock bias. */
 assert(!y2_xtal_boosted && map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_12_5UA);
 /* #51/#52: close and reopen (Next) keep the crystal running and selected: no clock work at all. */
 hw_free(0); on_ext(); assert(codec.clk_req == 0 && !xtal_off() && y2_xtal_on);
 pll_sets = 0;
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 1 && pll_sets == 0); on_ext();
 hw_free(0);
 /* #52: a crystal error seen at a stream start power-cycles the codec (rails off 500 ms, as from a power-on);
  * the stream waits for the restarted crystal on the crystal clock. */
 map.reg[CS43130_INT_STATUS_1] = CS43130_XTAL_ERR_INT | 0x41; ready_after = 30;
 {
  unsigned long before = jiffies;
  assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_errors == 1 && y2_codec_cycles == 1);
  assert(stops == 1 && starts == 2 && !xtal_off() && y2_xtal_ready && y2_xtal_ready_ms == 30 && xtal_waits == 2);
  assert(sel() == CS43130_MCLK_SRC_EXT && codec.clk_req == 1 && jiffies - before >= 500 + 30);
 }
 hw_free(0);
 /* A clean status at the next start changes nothing. */
 assert(!y2_clk_get(&component, 0, 44100) && y2_codec_cycles == 1 && xtal_waits == 2); hw_free(0);
 /* Runtime suspend stops the crystal; runtime resume starts it as stock cs43131_hw_init and selects it. */
 runtime_suspend(); assert(stops == 2);
 runtime_resume(); assert(starts == 3 && !xtal_off() && y2_xtal_ready && y2_xtal_tries == 1); on_ext();
 assert(!(map.reg[CS43130_PWDN_CTL] & (CS43130_PDN_PLL_MASK | CS43130_PDN_CLKOUT_MASK)));
 assert(codec.mclk_int == CS43130_MCLK_22M && codec.pll_bypass && y2_xtal_ready_ms == 30);
 assert(!y2_clk_get(&component, 0, 44100) && xtal_waits == 3); hw_free(0);
 /* #52/#54 on 8.24: a power-up whose crystal flags an error gets a cold start, up to three tries. */
 {
  unsigned errors = y2_xtal_errors; int st = stops;
  runtime_suspend(); bad_starts = 2; warnings = 0;
  runtime_resume();
  assert(y2_xtal_ready && y2_xtal_tries == 3 && y2_xtal_errors == errors + 2 && stops == st + 3 && warnings == 2);
  on_ext(); assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_stream_cycles == 0); hw_free(0);
 }
 /* Three failed tries: the crystal is used anyway (stock never checks); the first stream adds one cold start, which
  * here succeeds. */
 {
  unsigned cycles = y2_codec_cycles;
  runtime_suspend(); bad_starts = 3; warnings = 0;
  runtime_resume();
  assert(!y2_xtal_ready && !y2_xtal_assumed && y2_xtal_tries == 3 && warnings == 3); on_ext();
  assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_stream_cycles == 1 && y2_codec_cycles == cycles + 1);
  assert(y2_xtal_ready && !y2_xtal_assumed && !y2_xtal_unconfirmed); on_ext();
  hw_free(0);
 }
 /* #54: a crystal that never starts: the stream's one cold start fails too, then it is used as stock, and later
  * streams neither wait nor cycle again until the codec powers up anew. Bounded: 3 + 3 starts. */
 {
  int st = starts; unsigned cycles = y2_codec_cycles;
  runtime_suspend(); bad_starts = 1000; warnings = 0;
  runtime_resume();
  assert(!y2_clk_get(&component, 0, 44100));
  assert(starts == st + 6 && y2_codec_cycles == cycles + 1 && y2_xtal_stream_cycles == 1);
  assert(y2_xtal_assumed && !y2_xtal_ready && y2_xtal_unconfirmed == 1); on_ext();
  hw_free(0);
  st = starts; { int waits = xtal_waits;
  assert(!y2_clk_get(&component, 0, 44100) && starts == st && xtal_waits == waits && y2_xtal_unconfirmed == 1); }
  hw_free(0);
  /* A ready flag that comes later is recorded. */
  bad_starts = 0; this_bad = 0; warnings = 0; map.reg[CS43130_INT_STATUS_1] = CS43130_XTAL_RDY_INT;
  assert(!y2_clk_get(&component, 0, 44100) && y2_xtal_ready && warnings == 1 && starts == st);
  hw_free(0);
 }
 /* An error latched by the first stream after a failed power-up cycles at once (the 8.24 gap). */
 {
  unsigned cycles = y2_codec_cycles;
  runtime_suspend(); bad_starts = 3; runtime_resume(); assert(!y2_xtal_ready);
  map.reg[CS43130_INT_STATUS_1] = CS43130_XTAL_ERR_INT | 0x40;
  assert(!y2_clk_get(&component, 0, 44100) && y2_codec_cycles == cycles + 1 && y2_xtal_ready && y2_xtal_stream_cycles == 1);
  hw_free(0);
 }
 /* Test switches: clocks-only start (8.24's 0xF0) and a quiet start without the bias write and status read. */
 runtime_suspend(); y2_xtal_full_power = false; runtime_resume();
 assert(pwdn_at_start == 0xf0 && y2_xtal_ready); y2_xtal_full_power = true;
 runtime_suspend(); bias_writes = 0; y2_xtal_quiet_start = true; runtime_resume();
 assert(!bias_writes && pwdn_at_start == 0 && y2_xtal_ready); y2_xtal_quiet_start = false;
 /* Test boost: the bias changes at the next power-up, never under a running crystal. */
 y2_xtal_boost(&codec); assert(y2_xtal_boosted && map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_12_5UA);
 runtime_suspend(); runtime_resume(); assert(map.reg[CS43130_CRYSTAL_SET] == CS43130_XTAL_IBIAS_15UA);
 /* #52: 48 kHz, and every rate the crystal does not clock, is refused before any clock work: no PLL, ever. */
 pll_sets = 0; stops = 0;
 assert(y2_clk_get(&component, 0, 48000) == -EINVAL && y2_clk_get(&component, 0, 96000) == -EINVAL);
 assert(y2_clk_get(&component, 0, 32000) == -EINVAL);
 assert(!pll_sets && codec.clk_req == 0 && !y2_clk_holders && !stops && !xtal_off()); on_ext();
 hw_free(0); /* ALSA's hw_free after the refusal must not take a request it never got */
 assert(codec.clk_req == 0);
 /* Runtime suspend returns the codec to RCO with the PLL off; the next open selects the crystal again. */
 runtime_suspend(); on_rco(); runtime_resume();
 assert(!y2_clk_get(&component, 0, 44100)); on_ext();
 hw_free(0);
 /* A repeated hw_params keeps the clock; 88.2 kHz runs from the crystal too; a refused rate keeps the request. */
 assert(!y2_clk_get(&component, 0, 44100) && codec.clk_req == 1);
 assert(!y2_clk_get(&component, 0, 88200) && codec.clk_req == 1 && codec.mclk_int == CS43130_MCLK_22M && !pll_sets);
 assert(y2_clk_get(&component, 0, 48000) == -EINVAL && codec.clk_req == 1);
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
