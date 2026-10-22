// SPDX-License-Identifier: GPL-2.0-only
/* Innioasis Y2 headphone card. Board topology adapted from Chris Hendrickson's
 * GPL-2.0 mt6582-cs43131.c; upstream 6.18 codec with system PM/error fixes.
 */
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/regulator/consumer.h>
#include <sound/pcm_params.h>
#include <sound/soc.h>
#include "../../codecs/cs43130.h"
#include "/project/kernel/platform/boot.h"

struct y2_audio {
	struct snd_soc_card card;
	struct snd_soc_dai_link links[2];
	struct snd_soc_dai_link_component cpus[2], codecs[2], platforms[2];
};
static int y2_codec_params(struct snd_pcm_substream *s, struct snd_pcm_hw_params *params)
{
	struct snd_soc_pcm_runtime *rtd = snd_soc_substream_to_rtd(s);

	return snd_soc_dai_set_sysclk(snd_soc_rtd_to_codec(rtd, 0), 0,
				      params_rate(params) * 64, SND_SOC_CLOCK_IN);
}
static int y2_codec_init(struct snd_soc_pcm_runtime *rtd)
{
	return snd_soc_component_set_sysclk(snd_soc_rtd_to_codec(rtd, 0)->component,
					    0, CS43130_MCLK_SRC_EXT, 22579200,
					    SND_SOC_CLOCK_IN);
}
static const struct snd_soc_ops y2_codec_ops = { .hw_params = y2_codec_params };
/* POWER-FINAL-27 (ledger F20): stock powers VGP2 (the CS43131 VA/VP/VCP rail)
 * down whenever the DAC is idle; it has no other user. vgp2_idle_off=N, the
 * kernel default, keeps 26's always-on rail with one card reference; the
 * platform sets it after the UI is up, and the codec's runtime PM then switches
 * the rail with its GPIO-enabled VD/VL rails (GPIOs first, VGP2 last, as stock). */
static DEFINE_MUTEX(y2_vgp2_lock);
static struct regulator *y2_vgp2;
static bool y2_vgp2_held, vgp2_idle_off;
static int y2_vgp2_sync(void)
{
	int ret = 0;

	lockdep_assert_held(&y2_vgp2_lock);
	if (!y2_vgp2)
		return 0;
	if (!vgp2_idle_off && !y2_vgp2_held) {
		ret = regulator_enable(y2_vgp2);
		y2_vgp2_held = !ret;
	} else if (vgp2_idle_off && y2_vgp2_held) {
		ret = regulator_disable(y2_vgp2);
		y2_vgp2_held = !!ret;
	}
	return ret;
}
static int y2_vgp2_idle_off_set(const char *val, const struct kernel_param *kp)
{
	int ret;

	mutex_lock(&y2_vgp2_lock);
	ret = param_set_bool(val, kp);
	if (!ret)
		ret = y2_vgp2_sync();
	mutex_unlock(&y2_vgp2_lock);
	return ret;
}
static const struct kernel_param_ops y2_vgp2_idle_off_ops = {
	.set = y2_vgp2_idle_off_set, .get = param_get_bool,
};
module_param_cb(vgp2_idle_off, &y2_vgp2_idle_off_ops, &vgp2_idle_off, 0644);
MODULE_PARM_DESC(vgp2_idle_off, "let the CS43131 VGP2 rail turn off while the DAC is idle (stock)");
static void y2_vgp2_release(void *unused)
{
	mutex_lock(&y2_vgp2_lock);
	if (y2_vgp2_held && !regulator_disable(y2_vgp2))
		y2_vgp2_held = false;
	y2_vgp2 = NULL;
	mutex_unlock(&y2_vgp2_lock);
}
static const struct snd_soc_dapm_widget y2_widgets[] = { SND_SOC_DAPM_HP("Headphone", NULL) };
static const struct snd_soc_dapm_route y2_routes[] = {
	{ "Headphone", NULL, "HPOUTA" }, { "Headphone", NULL, "HPOUTB" },
};
static const struct snd_kcontrol_new y2_controls[] = { SOC_DAPM_PIN_SWITCH("Headphone") };
static int y2_late_probe(struct snd_soc_card *card)
{
	struct y2_audio *y2 = container_of(card, struct y2_audio, card);
	struct snd_soc_pcm_runtime *rtd = snd_soc_get_pcm_runtime(card, &y2->links[1]);
	/* Quiesce ALSA/DAPM before the AFE and I2C codec power down. These
	 * managed links order system PM without adding runtime power references. */
	if (!rtd || !device_link_add(card->dev, snd_soc_rtd_to_cpu(rtd, 0)->dev,
				   DL_FLAG_AUTOREMOVE_CONSUMER) ||
	    !device_link_add(card->dev, snd_soc_rtd_to_codec(rtd, 0)->dev,
			     DL_FLAG_AUTOREMOVE_CONSUMER))
		return -ENOMEM;
	/* This headphone-only board needs no codec jack-detect standby bias.
	 * Let the core's initial DAPM sync and later stream shutdown balance the
	 * codec runtime-PM reference. A live stream still raises bias normally;
	 * the checked codec callbacks own reset, cache and supply sequencing. */
	snd_soc_dapm_set_idle_bias(snd_soc_component_get_dapm(
				 snd_soc_rtd_to_codec(rtd, 0)->component), false);
	/* No automatic output: owner enables the pin after setting a safe level.
	 * Upstream codec reset volume is -60 dB; no startup state restoration. */
	return snd_soc_dapm_disable_pin(&card->dapm, "Headphone");
}
static void y2_node_put(void *node) { of_node_put(node); }
static int y2_audio_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct device_node *afe, *codec;
	struct regulator *vgp2;
	struct y2_audio *y2;
	int ret, i;

	if (IS_ENABLED(CONFIG_Y2_POWER) && !y2_normal_boot_enabled())
		return -EPROBE_DEFER;

	y2 = devm_kzalloc(dev, sizeof(*y2), GFP_KERNEL);
	if (!y2)
		return -ENOMEM;
	vgp2 = devm_regulator_get(dev, "vgp2");
	if (IS_ERR(vgp2))
		return dev_err_probe(dev, PTR_ERR(vgp2), "VGP2\n");
	mutex_lock(&y2_vgp2_lock);
	y2_vgp2 = vgp2;
	ret = y2_vgp2_sync();
	mutex_unlock(&y2_vgp2_lock);
	if (ret)
		return dev_err_probe(dev, ret, "VGP2 hold\n");
	ret = devm_add_action_or_reset(dev, y2_vgp2_release, NULL);
	if (ret)
		return ret;
	afe = of_parse_phandle(dev->of_node, "mediatek,platform", 0);
	if (!afe)
		return -EINVAL;
	ret = devm_add_action_or_reset(dev, y2_node_put, afe);
	if (ret)
		return ret;
	codec = of_parse_phandle(dev->of_node, "audio-codec", 0);
	if (!codec)
		return -EINVAL;
	ret = devm_add_action_or_reset(dev, y2_node_put, codec);
	if (ret)
		return ret;
	for (i = 0; i < 2; i++) {
		y2->cpus[i].of_node = afe;
		y2->cpus[i].dai_name = i ? "I2S" : "DL1";
		y2->platforms[i].of_node = afe;
		y2->links[i].cpus = &y2->cpus[i];
		y2->links[i].num_cpus = 1;
		y2->links[i].codecs = &y2->codecs[i];
		y2->links[i].num_codecs = 1;
		y2->links[i].platforms = &y2->platforms[i];
		y2->links[i].num_platforms = 1;
		y2->links[i].playback_only = 1;
		/* With DPCM the front end's close stops the back end at once
		 * (dpcm_fe_dai_shutdown), so a power-down delay would not apply. */
		y2->links[i].ignore_pmdown_time = 1;
	}
	y2->codecs[0].name = "snd-soc-dummy";
	y2->codecs[0].dai_name = "snd-soc-dummy-dai";
	y2->links[0].name = "Y2 Headphone Playback";
	y2->links[0].stream_name = "Y2 Headphone Playback";
	y2->links[0].dynamic = 1;
	y2->links[0].trigger[0] = SND_SOC_DPCM_TRIGGER_POST;
	y2->links[0].trigger[1] = SND_SOC_DPCM_TRIGGER_POST;
	y2->codecs[1].of_node = codec;
	y2->codecs[1].dai_name = "cs43130-asp-pcm";
	y2->links[1].name = "Y2 Second I2S";
	y2->links[1].no_pcm = 1;
	y2->links[1].init = y2_codec_init;
	y2->links[1].ops = &y2_codec_ops;
	y2->links[1].dai_fmt = SND_SOC_DAIFMT_I2S | SND_SOC_DAIFMT_NB_NF | SND_SOC_DAIFMT_CBC_CFC;
	y2->card = (struct snd_soc_card) {
		.name = "Y2Audio", .owner = THIS_MODULE, .dev = dev,
		.dai_link = y2->links, .num_links = ARRAY_SIZE(y2->links),
		.controls = y2_controls, .num_controls = ARRAY_SIZE(y2_controls),
		.dapm_widgets = y2_widgets, .num_dapm_widgets = ARRAY_SIZE(y2_widgets),
		.dapm_routes = y2_routes, .num_dapm_routes = ARRAY_SIZE(y2_routes),
		.late_probe = y2_late_probe,
	};
	return devm_snd_soc_register_card(dev, &y2->card);
}
static const struct of_device_id y2_audio_match[] = { { .compatible = "innioasis,y2-audio" }, {} };
MODULE_DEVICE_TABLE(of, y2_audio_match);
static struct platform_driver y2_audio_driver = {
	.probe = y2_audio_probe,
	.driver = { .name = "y2-audio", .of_match_table = y2_audio_match, .pm = &snd_soc_pm_ops },
};
module_platform_driver(y2_audio_driver);
MODULE_DESCRIPTION("Innioasis Y2 CS43131 headphone ASoC card");
MODULE_LICENSE("GPL");
