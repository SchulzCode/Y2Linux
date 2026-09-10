// SPDX-License-Identifier: GPL-2.0-only
/* Innioasis Y2 headphone card. Board topology adapted from Chris Hendrickson's
 * GPL-2.0 mt6582-cs43131.c; codec implementation is unmodified upstream 6.18.
 */
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/regulator/consumer.h>
#include <sound/pcm_params.h>
#include <sound/soc.h>
#include "../../codecs/cs43130.h"

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
static const struct snd_soc_dapm_widget y2_widgets[] = { SND_SOC_DAPM_HP("Headphone", NULL) };
static const struct snd_soc_dapm_route y2_routes[] = {
	{ "Headphone", NULL, "HPOUTA" }, { "Headphone", NULL, "HPOUTB" },
};
static const struct snd_kcontrol_new y2_controls[] = { SOC_DAPM_PIN_SWITCH("Headphone") };
static int y2_late_probe(struct snd_soc_card *card)
{
	/* No automatic output: owner enables the pin after setting a safe level.
	 * Upstream codec reset volume is -60 dB; no startup state restoration. */
	return snd_soc_dapm_disable_pin(&card->dapm, "Headphone");
}
static void y2_node_put(void *node) { of_node_put(node); }
static int y2_audio_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct device_node *afe, *codec;
	struct y2_audio *y2;
	int ret, i;

	y2 = devm_kzalloc(dev, sizeof(*y2), GFP_KERNEL);
	if (!y2)
		return -ENOMEM;
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
	.driver = { .name = "y2-audio", .of_match_table = y2_audio_match },
};
module_platform_driver(y2_audio_driver);
MODULE_DESCRIPTION("Innioasis Y2 CS43131 headphone ASoC card");
MODULE_LICENSE("GPL");
