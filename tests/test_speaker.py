"""Internal speaker: AW87559 behind the CS43131, jack poll and output switching. Synthetic, no hardware."""
from pathlib import Path
import unittest
from test_audio import function
from test_hp_level import overlay
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
CARD = (ROOT / 'kernel/audio/y2-cs43131.c').read_text()


class OutputSwitch(unittest.TestCase):
    def test_outputs_are_exclusive_and_never_switch_under_a_stream(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
#define BIT(n) (1U << (n))
#define CS43130_HP_STATUS 0xd0001
#define container_of(p,type,member) ((type *)((char *)(p)-offsetof(type,member)))
struct regmap { int powered; unsigned status; };
struct snd_soc_component { struct regmap *regmap; int active; };
struct snd_soc_dai { struct snd_soc_component *component; };
struct snd_soc_pcm_runtime { struct snd_soc_dai codec; };
struct snd_soc_dapm_context { int hp, spk, syncs; };
struct snd_soc_card { struct snd_soc_dapm_context dapm; };
struct y2_audio { struct snd_soc_card card; int links[2]; };
struct snd_kcontrol { struct snd_soc_card *card; unsigned long private_value; };
struct snd_ctl_elem_value { struct { struct { long value[1]; } integer; } value; };
static struct regmap map; static struct snd_soc_component codec = { &map, 0 };
static struct snd_soc_pcm_runtime rtd = { { &codec } };
static struct snd_soc_pcm_runtime *snd_soc_get_pcm_runtime(struct snd_soc_card *c, int *l) {
 assert(l == &container_of(c, struct y2_audio, card)->links[1]); return &rtd; }
static struct snd_soc_dai *snd_soc_rtd_to_codec(struct snd_soc_pcm_runtime *r, unsigned i) { assert(!i); return &r->codec; }
static int snd_soc_component_active(struct snd_soc_component *c) { return c->active; }
static int regmap_read(struct regmap *m, unsigned r, unsigned *v) {
 assert(r == CS43130_HP_STATUS); if (!m->powered) return -EBUSY; *v = m->status; return 0; }
static struct snd_soc_card *snd_kcontrol_chip(struct snd_kcontrol *k) { return k->card; }
static int *pin(struct snd_soc_dapm_context *d, const char *p) {
 if (!strcmp(p, "Speaker")) return &d->spk; assert(!strcmp(p, "Headphone")); return &d->hp; }
static int snd_soc_dapm_get_pin_status(struct snd_soc_dapm_context *d, const char *p) { return *pin(d, p); }
static int snd_soc_dapm_disable_pin(struct snd_soc_dapm_context *d, const char *p) { *pin(d, p) = 0; return 0; }
static int snd_soc_dapm_put_pin_switch(struct snd_kcontrol *k, struct snd_ctl_elem_value *u) {
 *pin(&k->card->dapm, (const char *)k->private_value) = u->value.integer.value[0]; k->card->dapm.syncs++; return 1; }
''' + function(CARD, 'y2_headphones_in') + function(CARD, 'y2_streaming') + function(CARD, 'y2_output_put') + r'''
static struct y2_audio y2;
static int set(const char *p, int on) {
 struct snd_kcontrol k = { &y2.card, (unsigned long)p }; struct snd_ctl_elem_value v = { { { { on } } } };
 return y2_output_put(&k, &v); }
int main(void) {
 struct snd_soc_dapm_context *d = &y2.card.dapm;
 map.powered = 1; map.status = 0;                   /* empty jack, codec held (screen on) */
 assert(set("Speaker", 1) == 1 && d->spk && !d->hp);
 assert(set("Headphone", 1) == 1 && d->hp && !d->spk); /* one on turns the other off */
 assert(set("Speaker", 1) == 1 && d->spk && !d->hp);
 codec.active = 1;                                   /* a stream runs on the speaker */
 assert(set("Headphone", 1) == -EBUSY && d->spk && !d->hp);
 assert(set("Speaker", 0) == 1 && !d->spk);          /* turning off is always allowed */
 assert(set("Headphone", 1) == 1 && d->hp);          /* nothing else on: allowed */
 assert(set("Speaker", 1) == -EBUSY && d->hp && !d->spk);
 codec.active = 0; map.status = BIT(6);              /* headphones in: never the speaker */
 assert(set("Speaker", 1) == -EBUSY && d->hp && !d->spk);
 map.powered = 0;                                    /* unreadable: allowed, the poll guards */
 assert(set("Speaker", 1) == 1 && d->spk && !d->hp);
 return 0;
}
''')

    def test_card_wires_the_amplifier_behind_both_hp_outputs(self):
        self.assertIn('SND_SOC_DAPM_SPK("Speaker", NULL)', CARD)
        for route in ('{ "PA IN", NULL, "HPOUTA" }', '{ "PA IN", NULL, "HPOUTB" }', '{ "Speaker", NULL, "PA OUT" }'):
            self.assertIn(route, CARD)
        self.assertIn('Y2_OUTPUT_SWITCH("Headphone"), Y2_OUTPUT_SWITCH("Speaker")', CARD)
        self.assertIn('of_parse_phandle(dev->of_node, "audio-amp", 0)', CARD)
        self.assertIn('.aux_dev = &y2->amp, .num_aux_devs = 1', CARD)
        late = function(CARD, 'y2_late_probe')
        self.assertIn('snd_soc_dapm_disable_pin(&card->dapm, "Speaker")', late)
        self.assertIn('aw87559.o', (ROOT / 'kernel/audio/Makefile').read_text())

    def test_amplifier_tables_are_the_stock_ones(self):
        amp = (ROOT / 'kernel/audio/aw87559.c').read_text()
        # aw87559_on (FM kernel 0xc054acc4): first and last writes, the OCA72559 table's last.
        self.assertIn('{ 0x62, 0xb5 }, { 0x78, 0x39 }', amp)
        self.assertIn('{ 0x01, 0x30 }, { 0x01, 0x78 },', amp)
        self.assertIn('{ 0x01, 0x38 }, { 0x01, 0x7c },', amp)
        self.assertIn('regmap_write(aw->regmap, AW87559_REG_SYSCTRL, 0x30);', amp)
        pulse = function(amp, 'aw87559_power_pulse')
        self.assertIn('msleep(10);', pulse)
        self.assertIn('msleep(100);', pulse)
        self.assertIn('msleep(80);', function(amp, 'aw87559_enable'))
        # A silent amplifier must not keep the card (and the headphones) from binding.
        probe = function(amp, 'aw87559_i2c_probe')
        self.assertNotIn('return dev_err_probe(dev, ret, "chip id', probe)
        self.assertIn('devm_snd_soc_register_component', probe)


class JackPoll(unittest.TestCase):
    def setUp(self):
        self.codec = overlay('sound/soc/codecs/cs43130.c')

    def test_poll_never_keeps_the_codec_powered(self):
        poll = function(self.codec, 'y2_jack_poll')
        self.assertIn('pm_runtime_get_if_active(cs43130->dev) <= 0', poll)
        self.assertIn('__pm_runtime_put_autosuspend(cs43130->dev);', poll)
        self.assertNotIn('pm_runtime_get_sync', poll)
        self.assertNotIn('pm_runtime_resume', poll)
        self.assertNotRegex(poll, r'(?<!_)pm_runtime_put_autosuspend\(cs43130')  # marks busy in 6.18
        # Stops while powered down; runtime resume and probe start it.
        self.assertIn('mod_delayed_work(system_wq, &y2_jack_work', function(self.codec, 'cs43130_runtime_resume'))
        self.assertIn('schedule_delayed_work(&y2_jack_work', function(self.codec, 'cs43130_probe'))
        self.assertIn('cancel_delayed_work_sync(&y2_jack_work);', function(self.codec, 'cs43130_i2c_remove'))

    def test_plug_turns_the_speaker_off_before_reporting(self):
        poll = function(self.codec, 'y2_jack_poll')
        off = poll.index('snd_soc_dapm_disable_pin(&card->dapm, "Speaker");')
        self.assertLess(poll.index('if (plugged && card && y2_speaker_pin(cs43130))'), off)
        self.assertLess(off, poll.index('snd_soc_jack_report('))
        self.assertIn('y2_jack_empty_reads >= 2', poll)   # unplug debounced, plug at once
        self.assertIn('regmap_write(cs43130->regmap, CS43130_HP_DETECT, Y2_HP_DETECT_STOCK);',
                      function(self.codec, 'cs43130_probe'))

    def test_no_codec_irq_so_the_crystal_start_keeps_its_polling(self):
        dts = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()
        codec = dts[dts.index('dac: codec@30 {'):]
        self.assertNotIn('interrupts', codec[:codec.index('};')])


if __name__ == '__main__':
    unittest.main()
