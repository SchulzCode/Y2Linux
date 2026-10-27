"""Exercise the real headphone output level code of the codec overlay against a register model."""
import unittest
from test_audio import function
from test_cpu_fix02 import overlay
from test_power import run_c


class HeadphoneLevel(unittest.TestCase):
    def test_level_is_written_only_while_the_amplifier_is_down(self):
        source = overlay('sound/soc/codecs/cs43130.c')
        tables = source[source.index('#define Y2_HP_OUT_FS_MASK'):source.index('/* Caller holds y2_hp_level_lock')]
        enums = source[source.index('static const char *const y2_hp_level_texts'):source.index('static int y2_hp_level_get')]
        sysfs_names = 'static const char *const y2_hp_gain_names[] = { "low", "mid", "high" };\n'
        self.assertIn(sysfs_names, source)
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/types.h>
typedef unsigned char u8;
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define CS43130_PWDN_CTL 0
#define CS43130_HP_OUT_CTL_1 1
#define CS43130_CLASS_H_CTL 2
#define CS43130_DXD1 3
#define CS43130_DXD12 4
#define CS43130_DXD13 5
#define CS43130_PDN_HP_MASK 0x10
#define CS43130_PCM_PATH_CTL_2 6
#define CS43130_ASP_CH_2_SZ_EN 7
enum { CS43130_CHIP_ID, CS4399_CHIP_ID, CS43131_CHIP_ID, CS43198_CHIP_ID };
enum { SND_SOC_DAPM_PRE_PMU = 1, SND_SOC_DAPM_POST_PMU = 2, SND_SOC_DAPM_POST_PMD = 8 };
struct mutex { int held; };
#define DEFINE_MUTEX(m) struct mutex m
static void mutex_lock(struct mutex *m) { assert(!m->held); m->held = 1; }
static void mutex_unlock(struct mutex *m) { assert(m->held); m->held = 0; }
struct regmap { unsigned reg[8]; int read_error; unsigned level_writes; };
struct reg_sequence { unsigned reg, def; };
struct snd_soc_component;
struct cs43130_private { struct regmap *regmap; int dev_id; struct snd_soc_component *component; };
/* DAPM powers the amplifier under the card's DAPM mutex (modelled as a flag). */
struct snd_soc_card { int dapm_held; int dapm; };
static struct snd_soc_card card;
/* The card's Speaker pin (#28 speaker route), read under the DAPM mutex. */
static int speaker_pin;
static int snd_soc_dapm_get_pin_status(int *d, const char *pin) {
 assert(d == &card.dapm && !strcmp(pin, "Speaker") && card.dapm_held); return speaker_pin;
}
static int bound(void);
static void snd_soc_dapm_mutex_lock(struct snd_soc_card *c) { assert(!c->dapm_held); c->dapm_held = 1; }
static void snd_soc_dapm_mutex_unlock(struct snd_soc_card *c) { assert(c->dapm_held); c->dapm_held = 0; }
static struct regmap map;
static struct cs43130_private codec = { &map, CS43131_CHIP_ID, 0 };
static int regmap_read(struct regmap *m, unsigned r, unsigned *v) {
 /* The PDN_HP check and the write after it are one step under DAPM. */
 if (r == CS43130_PWDN_CTL) assert(!bound() || card.dapm_held);
 if (m->read_error && r == CS43130_PWDN_CTL) return m->read_error;
 *v = m->reg[r]; return 0;
}
static int regmap_update_bits(struct regmap *m, unsigned r, unsigned mask, unsigned v) {
 /* DS1155F2: OUT_FS only while PDN_HP is set; never touch a live output. */
 assert(m->reg[CS43130_PWDN_CTL] & CS43130_PDN_HP_MASK);
 assert(r == CS43130_HP_OUT_CTL_1 || r == CS43130_CLASS_H_CTL ||
        r == CS43130_PCM_PATH_CTL_2 || r == CS43130_ASP_CH_2_SZ_EN);
 assert(!bound() || card.dapm_held);
 m->reg[r] = (m->reg[r] & ~mask) | v; m->level_writes++; return 0;
}
static int regmap_write(struct regmap *m, unsigned r, unsigned v) { m->reg[r] = v; return 0; }
static int regmap_multi_reg_write(struct regmap *m, const struct reg_sequence *s, int n) {
 for (int i = 0; i < n; i++) if (s[i].reg < 6) m->reg[s[i].reg] = s[i].def; return 0;
}
static void usleep_range(unsigned a, unsigned b) { (void)a; (void)b; }
static void msleep(unsigned a) { (void)a; }
#define dev_err(...) do {} while (0)
static const struct reg_sequence pop_free_seq[] = { {CS43130_DXD12, 0x0a} };
static const struct reg_sequence pop_free_seq2[] = { {CS43130_DXD13, 0x20} };
static const struct reg_sequence dac_postpmu_seq[] = { {CS43130_DXD12, 0} };
static const struct reg_sequence dac_postpmd_seq[] = { {CS43130_DXD12, 0} };
/* ASoC shims for the control callbacks and the DAPM event. */
struct snd_soc_component { struct cs43130_private *data; struct snd_soc_card *card; };
static struct snd_soc_component component = { &codec, &card };
static int bound(void) { return codec.component && codec.component->card; }
struct snd_kcontrol { int unused; };
struct snd_soc_dapm_context { struct snd_soc_component *component; };
struct snd_soc_dapm_widget { struct snd_soc_dapm_context *dapm; };
struct snd_ctl_elem_value { union { struct { unsigned item[1]; } enumerated; } value; };
static struct snd_soc_component *snd_soc_kcontrol_component(struct snd_kcontrol *k) { (void)k; return &component; }
static struct snd_soc_component *snd_soc_dapm_to_component(struct snd_soc_dapm_context *d) { return d->component; }
static void *snd_soc_component_get_drvdata(struct snd_soc_component *c) { return c->data; }
struct soc_enum { int items; const char *const *texts; };
#define SOC_ENUM_SINGLE_EXT_DECL(name, texts) const struct soc_enum name = { ARRAY_SIZE(texts), texts }
struct device { struct cs43130_private *data; };
struct device_attribute { int unused; };
static struct device dev = { &codec };
static void *dev_get_drvdata(struct device *d) { return d->data; }
static int sysfs_emit(char *b, const char *f, ...) {
 __builtin_va_list a; __builtin_va_start(a, f); int n = vsprintf(b, f, a); __builtin_va_end(a); return n;
}
static int sysfs_match_string(const char *const *names, const char *b) {
 for (int i = 0; i < 3; i++) if (!strncmp(names[i], b, strlen(names[i]))) return i; return -EINVAL;
}
''' + tables + function(source, 'y2_hp_level_write') + function(source, 'y2_hp_dapm_lock')
           + function(source, 'y2_hp_dapm_unlock') + function(source, 'y2_hp_level_set')
           + function(source, 'y2_hp_level_live') + enums + function(source, 'y2_hp_level_get')
           + function(source, 'y2_hp_level_put') + function(source, 'y2_hp_live_get')
           + function(source, 'cs43130_dac_event') + sysfs_names
           + function(source, 'hp_gain_show') + function(source, 'hp_gain_store') + r'''
static struct snd_soc_dapm_context dapm = { &component };
static struct snd_soc_dapm_widget dac = { &dapm };
static unsigned get(void) { struct snd_ctl_elem_value v = {0}; assert(!y2_hp_level_get(0, &v)); return v.value.enumerated.item[0]; }
static int put(unsigned i) { struct snd_ctl_elem_value v = {0}; v.value.enumerated.item[0] = i; return y2_hp_level_put(0, &v); }
static unsigned live(void) { struct snd_ctl_elem_value v = {0}; assert(!y2_hp_live_get(0, &v)); return v.value.enumerated.item[0]; }
static void power_up(void) {
 snd_soc_dapm_mutex_lock(&card); /* DAPM: PRE_PMU, then the PDN_HP write */
 assert(!cs43130_dac_event(&dac, 0, SND_SOC_DAPM_PRE_PMU)); map.reg[CS43130_PWDN_CTL] &= ~CS43130_PDN_HP_MASK;
 snd_soc_dapm_mutex_unlock(&card);
}
static void power_down(void) { map.reg[CS43130_PWDN_CTL] |= CS43130_PDN_HP_MASK; }
int main(void) {
 char buf[32];
 assert(y2_hp_level_enum.items == 3 && !strcmp(y2_hp_level_enum.texts[2], "High"));
 assert(y2_hp_live_enum.items == 4 && !strcmp(y2_hp_live_enum.texts[3], "Other"));
 /* Reset values: PWDN_CTL 0xfe (HP down), HP_OUT_CTL_1 0x30 with HP_IN_EN and
  * a clamp bit set to prove only OUT_FS moves, CLASS_H 0x1e (adapt, HV_EN). */
 map.reg[CS43130_PWDN_CTL] = 0xfe; map.reg[CS43130_HP_OUT_CTL_1] = 0x30 | 0x08 | 0x80;
 map.reg[CS43130_CLASS_H_CTL] = 0x1e;
 assert(get() == 0 && live() == 2);              /* before probe: reset High */
 assert(y2_hp_level_set(&codec, 2) == 1);         /* sysfs before the card binds: no DAPM yet */
 assert(y2_hp_level_set(&codec, 0) == 1 && !card.dapm_held);
 codec.component = &component;                    /* bound: every check runs under DAPM */
 assert(y2_hp_level_set(&codec, 0) == 0 && !card.dapm_held); /* probe: default Low, no change */
 assert(map.reg[CS43130_HP_OUT_CTL_1] == (0x10 | 0x08 | 0x80) && map.reg[CS43130_CLASS_H_CTL] == 0x1c);
 assert(live() == 0 && hp_gain_show(&dev, 0, buf) > 0 && !strcmp(buf, "low\n"));
 /* Down: a request is written at once and reported as changed. */
 assert(put(2) == 1 && get() == 2 && live() == 2);
 assert(map.reg[CS43130_HP_OUT_CTL_1] == (0x30 | 0x08 | 0x80) && map.reg[CS43130_CLASS_H_CTL] == 0x1e);
 assert(put(2) == 0);                             /* same level: no change event */
 assert(put(1) == 1 && map.reg[CS43130_HP_OUT_CTL_1] == (0x20 | 0x08 | 0x80) && map.reg[CS43130_CLASS_H_CTL] == 0x1e);
 /* Playing: a request waits; the live output keeps its level. */
 power_up(); assert(live() == 1);
 unsigned writes = map.level_writes;
 assert(put(0) == 1 && get() == 0 && live() == 1 && map.level_writes == writes);
 assert(hp_gain_store(&dev, 0, "high\n", 5) == 5 && get() == 2 && live() == 1);
 assert(put(3) == -EINVAL && get() == 2);         /* out of range leaves the request */
 power_down(); power_up();                        /* next stream: requested level first */
 assert(live() == 2 && map.reg[CS43130_HP_OUT_CTL_1] == (0x30 | 0x08 | 0x80));
 power_down();
 /* Unreadable power state counts as live: request kept, nothing written. */
 map.read_error = -EBUSY; writes = map.level_writes;
 assert(put(0) == 1 && map.level_writes == writes && live() == 2);
 map.read_error = 0; power_up(); assert(live() == 0 && map.reg[CS43130_CLASS_H_CTL] == 0x1c);
 power_down();
 /* Another register combination reads as Other / raw in sysfs. */
 map.reg[CS43130_HP_OUT_CTL_1] = 0x00; assert(live() == 3);
 assert(hp_gain_show(&dev, 0, buf) > 0 && !strcmp(buf, "raw:00\n"));
 map.reg[CS43130_HP_OUT_CTL_1] = 0x30; map.reg[CS43130_CLASS_H_CTL] = 0x1c; assert(live() == 3);
 assert(hp_gain_store(&dev, 0, "loud\n", 5) == -EINVAL);
 /* Speaker pin: stock speaker route at the next power-up (PCM path 0x05,
  * channel 2 on the left slot, OUT_FS 1.73 Vrms, HV_EN), whatever the
  * headphone level; headphones again restore stereo and their level. */
 map.reg[CS43130_ASP_CH_2_SZ_EN] = 0x0e; assert(put(0) == 0); /* still Low */
 speaker_pin = 1; power_up();
 assert(map.reg[CS43130_PCM_PATH_CTL_2] == 0x05 && map.reg[CS43130_ASP_CH_2_SZ_EN] == 0x06);
 assert((map.reg[CS43130_HP_OUT_CTL_1] & 0x30) == 0x30 && (map.reg[CS43130_CLASS_H_CTL] & 0x02));
 power_down(); speaker_pin = 0; power_up();
 assert(map.reg[CS43130_PCM_PATH_CTL_2] == 0 && map.reg[CS43130_ASP_CH_2_SZ_EN] == 0x0e);
 assert((map.reg[CS43130_HP_OUT_CTL_1] & 0x30) == 0x10 && !(map.reg[CS43130_CLASS_H_CTL] & 0x02));
 power_down();
 assert(!card.dapm_held);
 return 0;
}
''')

    def test_level_controls_are_registered_and_applied_at_probe_and_power_up(self):
        source = overlay('sound/soc/codecs/cs43130.c')
        controls = source[source.index('static const struct snd_kcontrol_new cs43130_snd_controls[]'):]
        controls = controls[:controls.index('};\n')]
        self.assertIn('SOC_ENUM_EXT("HP Output Level", y2_hp_level_enum', controls)
        self.assertIn('.name = "HP Output Level Live"', controls)
        self.assertIn('SNDRV_CTL_ELEM_ACCESS_READ | SNDRV_CTL_ELEM_ACCESS_VOLATILE', controls)
        probe = function(source, 'cs43130_probe')
        self.assertLess(probe.index('y2_hp_level_set(cs43130, y2_hp_level)'), probe.index('snd_soc_card_jack_new'))
        event = function(source, 'cs43130_dac_event')
        pre = event[event.index('case SND_SOC_DAPM_PRE_PMU:'):event.index('case SND_SOC_DAPM_POST_PMU:')]
        self.assertLess(pre.index('y2_hp_level_write(cs43130);'), pre.index('pop_free_seq'))
        measure = function(source, 'hpload_measure_store')
        self.assertLess(measure.index('cs43130_imp_meas(&cs43130->work);'), measure.index('y2_hp_level_write(cs43130);'))
        # No stream may start (and power the amplifier) during a measurement:
        # DAPM is held from the open-stream check to after the level write.
        lock, unlock = measure.index('card = y2_hp_dapm_lock(cs43130);'), measure.index('y2_hp_dapm_unlock(card);')
        self.assertLess(lock, measure.index('snd_soc_component_active(cs43130->component)'))
        self.assertLess(measure.index('y2_hp_level_write(cs43130);'), unlock)
        self.assertLess(unlock, measure.index('pm_runtime_put(dev);'))


class ShippedProfiles(unittest.TestCase):
    def test_image_installs_the_autoeq_profiles_with_their_license(self):
        from pathlib import Path
        mk = (Path(__file__).resolve().parents[1] / 'buildroot/package/reborn/reborn.mk').read_text()
        self.assertIn('cp -a $(@D)/assets/eq/. $(TARGET_DIR)/usr/share/reborn/eq/', mk)
        self.assertLess(mk.index('rm -rf $(TARGET_DIR)/usr/share/reborn/eq'),
                        mk.index('cp -a $(@D)/assets/eq/.'))
        self.assertIn('assets/eq/LICENSE-AutoEq.txt', mk.split('REBORN_LICENSE_FILES =', 1)[1].splitlines()[0])


if __name__ == '__main__':
    unittest.main()
