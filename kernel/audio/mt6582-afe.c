// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 playback AFE, adapted from Chris Hendrickson's GPL-2.0 donor
 * mt6582-afe-pcm.c. MediaTek FE/BE topology with Linux 6.18 managed DMA,
 * runtime PM and synchronized IRQ lifetime. The one capture is the MT6627 FM
 * tuner (stock HAL indirect path, see mt6582-afe.h); no board policy.
 */
#include <linux/clk.h>
#include <linux/dma-mapping.h>
#include <linux/interrupt.h>
#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/pm_runtime.h>
#include <linux/regmap.h>
#include <sound/pcm_params.h>
#include <sound/soc.h>
#include "mt6582-afe.h"
#include "/project/kernel/platform/boot.h"

struct mt6582_afe {
	struct device *dev;
	struct regmap *regmap;
	struct clk_bulk_data clocks[3];
	struct snd_pcm_substream *substream, *capture;
	spinlock_t lock;
	int irq;
	bool running, capturing, irq_ready;
	unsigned long periods, capture_periods;
	unsigned configured_rate;
};

static const struct snd_pcm_hardware mt6582_hw = {
	.info = SNDRV_PCM_INFO_MMAP | SNDRV_PCM_INFO_MMAP_VALID |
		SNDRV_PCM_INFO_INTERLEAVED | SNDRV_PCM_INFO_BLOCK_TRANSFER,
	.formats = SNDRV_PCM_FMTBIT_S16_LE,
	.rates = SNDRV_PCM_RATE_44100 | SNDRV_PCM_RATE_48000,
	.rate_min = 44100, .rate_max = 48000,
	.channels_min = 2, .channels_max = 2,
	.buffer_bytes_max = PCM_MAX_BYTES,
	.period_bytes_min = 512, .period_bytes_max = 128 * 1024,
	.periods_min = 2, .periods_max = 256,
};

static const struct snd_pcm_hardware mt6582_fm_hw = {
	.info = SNDRV_PCM_INFO_MMAP | SNDRV_PCM_INFO_MMAP_VALID |
		SNDRV_PCM_INFO_INTERLEAVED | SNDRV_PCM_INFO_BLOCK_TRANSFER,
	.formats = SNDRV_PCM_FMTBIT_S16_LE,
	.rates = SNDRV_PCM_RATE_48000, .rate_min = FM_RATE, .rate_max = FM_RATE,
	.channels_min = 2, .channels_max = 2,
	.buffer_bytes_max = PCM_MAX_BYTES,
	.period_bytes_min = 512, .period_bytes_max = 128 * 1024,
	.periods_min = 2, .periods_max = 256,
};

static void mt6582_capture_stop(struct mt6582_afe *afe)
{
	/* Called with the private spinlock held, or before IRQ registration. */
	regmap_update_bits(afe->regmap, AFE_IRQ_CON, IRQ2, 0);
	regmap_update_bits(afe->regmap, AFE_DAC_CON0, AWB_ON, 0);
	regmap_write(afe->regmap, AFE_IRQ_CLR, IRQ2);
	afe->capturing = false;
}

/* The FM source module, in AudioFMResourceManager::SetFmSourceModuleEnable
 * order: second I2S in as slave, ASRC 32k -> 48k (SetI2SASRCConfig), ASRC
 * on, I2S in on, then the I00/I01 -> O05/O06 connection of the stream. */
static void mt6582_fm_input_off(struct mt6582_afe *afe)
{
	regmap_update_bits(afe->regmap, AFE_CONN2, FM_ROUTE, 0);
	regmap_update_bits(afe->regmap, AFE_I2S_CON, I2S_ON, 0);
	regmap_update_bits(afe->regmap, AFE_ASRC_CON0, ASRC_ON, 0);
	regmap_update_bits(afe->regmap, AFE_CONN4, ASRC_BYPASS, ASRC_BYPASS);
}
static int mt6582_fm_input_on(struct mt6582_afe *afe)
{
	static const unsigned asrc[][2] = {
		{AFE_ASRC_CON14, 0x00600000}, {AFE_ASRC_CON15, 0x00400000},
		{AFE_ASRC_CON17, 0x00000cb2}, {AFE_ASRC_CON16, 0x00075987},
		{AFE_ASRC_CON20, 0x00001b00},
	};
	int ret = regmap_update_bits(afe->regmap, AFE_I2S_CON, ~I2S_ON, I2S_IN_WORD);

	if (!ret)
		ret = regmap_update_bits(afe->regmap, FPGA_CFG1, I2S_IN_SLAVE_CLOCK, 0);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_CONN4, ASRC_BYPASS, 0);
	/* Memif I2S rate = the ASRC output rate. */
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_DAC_CON1, 0xf00,
					 mt6582_rate_code(FM_RATE) << 8);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_ASRC_CON13, BIT(16), 0);
	for (unsigned i = 0; !ret && i < ARRAY_SIZE(asrc); i++)
		ret = regmap_write(afe->regmap, asrc[i][0], asrc[i][1]);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_ASRC_CON0, ASRC_ON, ASRC_ON);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_I2S_CON, I2S_ON, I2S_ON);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_CONN2, FM_ROUTE, FM_ROUTE);
	if (ret)
		mt6582_fm_input_off(afe);
	return ret;
}

static void mt6582_stop(struct mt6582_afe *afe)
{
	/* Called with the private spinlock held, or before IRQ registration. */
	regmap_update_bits(afe->regmap, AFE_IRQ_CON, IRQ1, 0);
	regmap_update_bits(afe->regmap, AFE_DAC_CON0, DL1_ON, 0);
	regmap_write(afe->regmap, AFE_IRQ_CLR, IRQ1);
	afe->running = false;
}

static irqreturn_t mt6582_irq(int irq, void *data)
{
	struct mt6582_afe *afe = data;
	struct snd_pcm_substream *substream = NULL, *capture = NULL;
	unsigned status;
	unsigned long flags;

	spin_lock_irqsave(&afe->lock, flags);
	if (regmap_read(afe->regmap, AFE_IRQ_STATUS, &status) || !(status & 0xff)) {
		spin_unlock_irqrestore(&afe->lock, flags);
		return IRQ_NONE;
	}
	/* Ack the snapshot before ALSA callbacks; never lose a newly latched IRQ. */
	regmap_write(afe->regmap, AFE_IRQ_CLR, status & 0xff);
	if ((status & IRQ1) && afe->running) {
		substream = afe->substream;
		afe->periods++;
	}
	if ((status & IRQ2) && afe->capturing) {
		capture = afe->capture;
		afe->capture_periods++;
	}
	spin_unlock_irqrestore(&afe->lock, flags);
	/* No private lock across period_elapsed: ALSA can call trigger(STOP).
	 * sync_stop/hw_free/close drain this handler before buffer/runtime release. */
	if (substream)
		snd_pcm_period_elapsed(substream);
	if (capture)
		snd_pcm_period_elapsed(capture);
	return IRQ_HANDLED;
}

static int mt6582_open(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	int ret;

	snd_soc_set_runtime_hwparams(substream, &mt6582_hw);
	ret = snd_pcm_hw_constraint_step(substream->runtime, 0,
					SNDRV_PCM_HW_PARAM_BUFFER_BYTES, 16);
	if (ret < 0)
		return ret;
	ret = snd_pcm_hw_constraint_step(substream->runtime, 0,
					SNDRV_PCM_HW_PARAM_PERIOD_BYTES, 16);
	if (ret < 0)
		return ret;
	ret = snd_pcm_hw_constraint_integer(substream->runtime, SNDRV_PCM_HW_PARAM_PERIODS);
	if (ret < 0)
		return ret;
	/* Constraint refinement may return +1: only negatives are failures. */
	WRITE_ONCE(afe->substream, substream);
	return 0;
}

static int mt6582_sync(struct snd_soc_component *component,
		      struct snd_pcm_substream *substream)
{
	struct mt6582_afe *afe = snd_soc_component_get_drvdata(component);
	synchronize_irq(afe->irq);
	return 0;
}

static int mt6582_free(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	unsigned long flags;

	spin_lock_irqsave(&afe->lock, flags);
	mt6582_stop(afe);
	afe->configured_rate = 0;
	spin_unlock_irqrestore(&afe->lock, flags);
	synchronize_irq(afe->irq);
	return 0;
}

static void mt6582_close(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);

	mt6582_free(substream, dai);
	WRITE_ONCE(afe->substream, NULL);
}

static int mt6582_params(struct snd_pcm_substream *substream,
			struct snd_pcm_hw_params *params, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	struct snd_pcm_runtime *runtime = substream->runtime;
	int fs = mt6582_rate_code(params_rate(params)), ret;
	unsigned bytes = params_buffer_bytes(params);
	unsigned long flags;

	spin_lock_irqsave(&afe->lock, flags);
	afe->configured_rate = 0;
	spin_unlock_irqrestore(&afe->lock, flags);
	if (fs < 0 || params_channels(params) != 2 ||
	    params_format(params) != SNDRV_PCM_FORMAT_S16_LE ||
	    !mt6582_dma_valid(runtime->dma_addr, bytes))
		return -EINVAL;
	ret = regmap_write(afe->regmap, AFE_DL1_BASE, lower_32_bits(runtime->dma_addr));
	if (ret)
		return ret;
	ret = regmap_write(afe->regmap, AFE_DL1_END, lower_32_bits(runtime->dma_addr) + bytes - 1);
	if (ret)
		return ret;
	/* S16 stereo: DL1 rate field, mono disabled; no unimplemented HD format. */
	ret = regmap_update_bits(afe->regmap, AFE_DAC_CON1, 0xf | BIT(21), fs);
	if (!ret) {
		spin_lock_irqsave(&afe->lock, flags);
		afe->configured_rate = params_rate(params);
		spin_unlock_irqrestore(&afe->lock, flags);
	}
	if (!ret)
		dev_info(afe->dev, "DL1 %u Hz S16_LE stereo DMA=%pad bytes=%u period=%u frames\n",
			 params_rate(params), &runtime->dma_addr, bytes, params_period_size(params));
	return ret;
}

static int mt6582_trigger(struct snd_pcm_substream *substream, int cmd,
			 struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	unsigned long flags;
	int ret = 0, fs = mt6582_rate_code(substream->runtime->rate);

	spin_lock_irqsave(&afe->lock, flags);
	switch (cmd) {
	case SNDRV_PCM_TRIGGER_START:
	case SNDRV_PCM_TRIGGER_RESUME:
		if (fs < 0 || !substream->runtime->period_size ||
		    substream->runtime->period_size > 0x3ffff) {
			ret = -EINVAL;
			break;
		}
		ret = regmap_update_bits(afe->regmap, AFE_IRQ_CNT1, 0x3ffff,
					 substream->runtime->period_size);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_IRQ_CON, 0xf0 | IRQ1, fs << 4);
		if (!ret)
			ret = regmap_write(afe->regmap, AFE_IRQ_CLR, IRQ1);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_DAC_CON0, DL1_ON, DL1_ON);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_IRQ_CON, IRQ1, IRQ1);
		if (ret)
			mt6582_stop(afe);
		else
			afe->running = true;
		break;
	case SNDRV_PCM_TRIGGER_STOP:
	case SNDRV_PCM_TRIGGER_SUSPEND:
		mt6582_stop(afe);
		break;
	default:
		ret = -EINVAL;
	}
	spin_unlock_irqrestore(&afe->lock, flags);
	return ret;
}

static const struct snd_soc_dai_ops mt6582_fe_ops = {
	.startup = mt6582_open, .shutdown = mt6582_close,
	.hw_params = mt6582_params, .hw_free = mt6582_free, .trigger = mt6582_trigger,
};

static int mt6582_fm_open(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	int ret;

	snd_soc_set_runtime_hwparams(substream, &mt6582_fm_hw);
	ret = snd_pcm_hw_constraint_step(substream->runtime, 0,
					SNDRV_PCM_HW_PARAM_BUFFER_BYTES, 16);
	if (ret >= 0)
		ret = snd_pcm_hw_constraint_step(substream->runtime, 0,
						SNDRV_PCM_HW_PARAM_PERIOD_BYTES, 16);
	if (ret >= 0)
		ret = snd_pcm_hw_constraint_integer(substream->runtime, SNDRV_PCM_HW_PARAM_PERIODS);
	if (ret < 0)
		return ret;
	WRITE_ONCE(afe->capture, substream);
	return 0;
}

static int mt6582_fm_free(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	unsigned long flags;

	spin_lock_irqsave(&afe->lock, flags);
	mt6582_capture_stop(afe);
	spin_unlock_irqrestore(&afe->lock, flags);
	synchronize_irq(afe->irq);
	mt6582_fm_input_off(afe);
	return 0;
}

static void mt6582_fm_close(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);

	mt6582_fm_free(substream, dai);
	WRITE_ONCE(afe->capture, NULL);
}

static int mt6582_fm_params(struct snd_pcm_substream *substream,
			   struct snd_pcm_hw_params *params, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	struct snd_pcm_runtime *runtime = substream->runtime;
	unsigned bytes = params_buffer_bytes(params);
	int ret;

	if (params_rate(params) != FM_RATE || params_channels(params) != 2 ||
	    params_format(params) != SNDRV_PCM_FORMAT_S16_LE ||
	    !mt6582_dma_valid(runtime->dma_addr, bytes))
		return -EINVAL;
	ret = regmap_write(afe->regmap, AFE_AWB_BASE, lower_32_bits(runtime->dma_addr));
	if (!ret)
		ret = regmap_write(afe->regmap, AFE_AWB_END, lower_32_bits(runtime->dma_addr) + bytes - 1);
	/* AWB rate field, stereo (mono bit clear). */
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_DAC_CON1, (0xfU << AWB_RATE_SHIFT) | AWB_MONO,
					 mt6582_rate_code(FM_RATE) << AWB_RATE_SHIFT);
	if (!ret)
		ret = mt6582_fm_input_on(afe);
	if (!ret)
		dev_info(afe->dev, "FM I2S in -> ASRC -> AWB %u Hz DMA=%pad bytes=%u period=%u frames\n",
			 FM_RATE, &runtime->dma_addr, bytes, params_period_size(params));
	return ret;
}

static int mt6582_fm_trigger(struct snd_pcm_substream *substream, int cmd,
			    struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	unsigned long flags;
	int ret = 0;

	spin_lock_irqsave(&afe->lock, flags);
	switch (cmd) {
	case SNDRV_PCM_TRIGGER_START:
	case SNDRV_PCM_TRIGGER_RESUME:
		if (!substream->runtime->period_size || substream->runtime->period_size > 0x3ffff) {
			ret = -EINVAL;
			break;
		}
		ret = regmap_update_bits(afe->regmap, AFE_IRQ_CNT2, 0x3ffff,
					 substream->runtime->period_size);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_IRQ_CON, 0xf00 | IRQ2,
						 mt6582_rate_code(FM_RATE) << 8);
		if (!ret)
			ret = regmap_write(afe->regmap, AFE_IRQ_CLR, IRQ2);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_DAC_CON0, AWB_ON, AWB_ON);
		if (!ret)
			ret = regmap_update_bits(afe->regmap, AFE_IRQ_CON, IRQ2, IRQ2);
		if (ret)
			mt6582_capture_stop(afe);
		else
			afe->capturing = true;
		break;
	case SNDRV_PCM_TRIGGER_STOP:
	case SNDRV_PCM_TRIGGER_SUSPEND:
		mt6582_capture_stop(afe);
		break;
	default:
		ret = -EINVAL;
	}
	spin_unlock_irqrestore(&afe->lock, flags);
	return ret;
}

static const struct snd_soc_dai_ops mt6582_fm_ops = {
	.startup = mt6582_fm_open, .shutdown = mt6582_fm_close,
	.hw_params = mt6582_fm_params, .hw_free = mt6582_fm_free, .trigger = mt6582_fm_trigger,
};

static int mt6582_i2s_prepare(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);
	int fs = mt6582_rate_code(substream->runtime->rate), ret;

	if (fs < 0)
		return -EINVAL;
	ret = regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, 0);
	if (!ret)
		ret = regmap_write(afe->regmap, AFE_CONN0, DL1_ROUTE);
	/* CPU clock provider, normal polarity, I2S, 32-bit slots = 64 BCLK/frame.
	 * CON1 drives internal ADDA, not the Y2 external DAC. */
	if (!ret)
		ret = regmap_write(afe->regmap, AFE_I2S_CON3, (fs << 8) | BIT(3) | BIT(1));
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, I2S_ON);
	if (ret) {
		regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, 0);
		regmap_update_bits(afe->regmap, AFE_CONN0, DL1_ROUTE, 0);
	}
	return ret;
}

static void mt6582_i2s_shutdown(struct snd_pcm_substream *substream, struct snd_soc_dai *dai)
{
	struct mt6582_afe *afe = snd_soc_dai_get_drvdata(dai);

	regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, 0);
	regmap_update_bits(afe->regmap, AFE_CONN0, DL1_ROUTE, 0);
}

static int mt6582_i2s_fmt(struct snd_soc_dai *dai, unsigned fmt)
{
	return fmt == (SND_SOC_DAIFMT_I2S | SND_SOC_DAIFMT_NB_NF |
		       SND_SOC_DAIFMT_CBP_CFP) ? 0 : -EINVAL;
}

static const struct snd_soc_dai_ops mt6582_i2s_ops = {
	.prepare = mt6582_i2s_prepare, .shutdown = mt6582_i2s_shutdown,
	.set_fmt = mt6582_i2s_fmt,
};

#define MT6582_PLAYBACK(_name) { .stream_name = _name, .channels_min = 2, \
	.channels_max = 2, .rates = SNDRV_PCM_RATE_44100 | SNDRV_PCM_RATE_48000, \
	.formats = SNDRV_PCM_FMTBIT_S16_LE }
static struct snd_soc_dai_driver mt6582_dais[] = {
	{ .name = "DL1", .id = 0, .playback = MT6582_PLAYBACK("DL1"), .ops = &mt6582_fe_ops },
	{ .name = "I2S", .id = 1, .playback = MT6582_PLAYBACK("I2S Playback"), .ops = &mt6582_i2s_ops },
	{ .name = "FM", .id = 2, .ops = &mt6582_fm_ops,
	  .capture = { .stream_name = "FM Capture", .channels_min = 2, .channels_max = 2,
		       .rates = SNDRV_PCM_RATE_48000, .formats = SNDRV_PCM_FMTBIT_S16_LE } },
};
static const struct snd_soc_dapm_route mt6582_routes[] = {
	{ "I2S Playback", NULL, "DL1" },
};

static snd_pcm_uframes_t mt6582_pointer(struct snd_soc_component *component,
				      struct snd_pcm_substream *substream)
{
	struct mt6582_afe *afe = snd_soc_component_get_drvdata(component);
	struct snd_pcm_runtime *runtime = substream->runtime;
	unsigned cur;

	if (regmap_read(afe->regmap, substream->stream == SNDRV_PCM_STREAM_CAPTURE ?
			AFE_AWB_CUR : AFE_DL1_CUR, &cur))
		return 0;
	return bytes_to_frames(runtime, mt6582_pointer_bytes(lower_32_bits(runtime->dma_addr),
							    cur, runtime->dma_bytes));
}

static int mt6582_pcm_new(struct snd_soc_component *component, struct snd_soc_pcm_runtime *rtd)
{
	/* Managed coherent DMA: no physical carveout, no virt_to_phys, no DMAengine
	 * fiction. DL1 is the controller's native ring and 32-bit bus master. */
	return snd_pcm_set_managed_buffer_all(rtd->pcm, SNDRV_DMA_TYPE_DEV, component->dev,
					      0, PCM_MAX_BYTES);
}
static const struct snd_soc_component_driver mt6582_component = {
	.name = "mt6582-afe", .pointer = mt6582_pointer, .pcm_construct = mt6582_pcm_new,
	.sync_stop = mt6582_sync,
	.dapm_routes = mt6582_routes, .num_dapm_routes = ARRAY_SIZE(mt6582_routes),
};

static int mt6582_runtime_resume(struct device *dev)
{
	struct mt6582_afe *afe = dev_get_drvdata(dev);
	int ret = clk_bulk_prepare_enable(ARRAY_SIZE(afe->clocks), afe->clocks);

	if (ret)
		return ret;
	/* Vendor APB source selection + AFE/I2S gates. Preserve unrelated bits
	 * instead of the donor's whole-register 0x60004000 write. */
	ret = regmap_update_bits(afe->regmap, AUDIO_TOP_CON0,
				 BIT(30) | BIT(29) | BIT(14) | BIT(6) | BIT(2),
				 BIT(30) | BIT(29) | BIT(14));
	if (!ret)
		ret = regmap_write(afe->regmap, AFE_IRQ_CON, 0);
	if (!ret)
		ret = regmap_write(afe->regmap, AFE_IRQ_CLR, 0xff);
	if (!ret)
		ret = regmap_update_bits(afe->regmap, AFE_DAC_CON0, AFE_ON | DL1_ON | AWB_ON, AFE_ON);
	if (!ret) {
		if (afe->irq_ready)
			enable_irq(afe->irq);
		return 0;
	}
	clk_bulk_disable_unprepare(ARRAY_SIZE(afe->clocks), afe->clocks);
	return ret;
}
static int mt6582_runtime_suspend(struct device *dev)
{
	struct mt6582_afe *afe = dev_get_drvdata(dev);
	unsigned long flags;

	disable_irq(afe->irq); /* No handler may read the clock-gated domain. */

	spin_lock_irqsave(&afe->lock, flags);
	mt6582_stop(afe);
	mt6582_capture_stop(afe);
	regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, 0);
	regmap_update_bits(afe->regmap, AFE_DAC_CON0, AFE_ON, 0);
	spin_unlock_irqrestore(&afe->lock, flags);
	synchronize_irq(afe->irq);
	mt6582_fm_input_off(afe);
	regmap_update_bits(afe->regmap, AUDIO_TOP_CON0, BIT(6) | BIT(2), BIT(6) | BIT(2));
	clk_bulk_disable_unprepare(ARRAY_SIZE(afe->clocks), afe->clocks);
	return 0;
}
static void mt6582_pm_disable(void *data)
{
	struct device *dev = data;

	pm_runtime_disable(dev);
	if (!pm_runtime_status_suspended(dev))
		mt6582_runtime_suspend(dev);
	pm_runtime_set_suspended(dev);
}
static const struct regmap_config mt6582_regmap = {
	.reg_bits = 32, .val_bits = 32, .reg_stride = 4,
	.max_register = 0x570, .cache_type = REGCACHE_NONE,
	.fast_io = true,
};
static int mt6582_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct mt6582_afe *afe;
	void __iomem *base;
	int ret;

	if (IS_ENABLED(CONFIG_Y2_POWER) && !y2_normal_boot_enabled())
		return -EPROBE_DEFER;

	ret = dma_set_mask_and_coherent(dev, DMA_BIT_MASK(32));
	if (ret)
		return ret;
	afe = devm_kzalloc(dev, sizeof(*afe), GFP_KERNEL);
	if (!afe)
		return -ENOMEM;
	afe->dev = dev;
	spin_lock_init(&afe->lock);
	platform_set_drvdata(pdev, afe);
	base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(base))
		return PTR_ERR(base);
	afe->regmap = devm_regmap_init_mmio(dev, base, &mt6582_regmap);
	if (IS_ERR(afe->regmap))
		return PTR_ERR(afe->regmap);
	afe->clocks[0].id = "audintbus";
	afe->clocks[1].id = "audio";
	afe->clocks[2].id = "infra_audio";
	ret = devm_clk_bulk_get(dev, ARRAY_SIZE(afe->clocks), afe->clocks);
	if (ret)
		return dev_err_probe(dev, ret, "audio clocks\n");
	afe->irq = platform_get_irq(pdev, 0);
	if (afe->irq < 0)
		return afe->irq;
	/* Quiet inherited state before exposing the IRQ, with clocks accessible. */
	ret = mt6582_runtime_resume(dev);
	if (ret)
		return ret;
	regmap_update_bits(afe->regmap, AFE_I2S_CON3, I2S_ON, 0);
	ret = devm_request_irq(dev, afe->irq, mt6582_irq, 0, "mt6582-afe", afe);
	if (ret) {
		regmap_update_bits(afe->regmap, AFE_DAC_CON0, AFE_ON | DL1_ON, 0);
		regmap_update_bits(afe->regmap, AUDIO_TOP_CON0, BIT(6) | BIT(2), BIT(6) | BIT(2));
		clk_bulk_disable_unprepare(ARRAY_SIZE(afe->clocks), afe->clocks);
		return ret;
	}
	afe->irq_ready = true;
	pm_runtime_set_active(dev);
	pm_runtime_enable(dev);
	/* Registered after IRQ: component unregisters first, then PM/IRQ cleanup. */
	ret = devm_add_action_or_reset(dev, mt6582_pm_disable, dev);
	if (ret)
		return ret;
	ret = devm_snd_soc_register_component(dev, &mt6582_component,
					    mt6582_dais, ARRAY_SIZE(mt6582_dais));
	if (ret)
		return ret;
	pm_runtime_idle(dev);
	dev_info(dev, "DL1 -> I05/I06 -> O00/O01 -> second I2S CON3; DMA32, stereo S16 44.1/48k\n");
	return 0;
}
/* Bounded read-only diagnostic, with runtime PM guarding MMIO access.
 * No writable register interface. Use this instead of dumping regmap while
 * the clock domain is suspended. Counter is cumulative, not a clock meter. */
static ssize_t state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct mt6582_afe *afe = dev_get_drvdata(dev);
	static const unsigned regs[] = { AUDIO_TOP_CON0, AFE_DAC_CON0, AFE_DAC_CON1,
		AFE_CONN0, AFE_DL1_BASE, AFE_DL1_CUR, AFE_DL1_END, AFE_I2S_CON3,
		AFE_IRQ_CON, AFE_IRQ_CNT1, AFE_I2S_CON, AFE_CONN2, AFE_CONN4, AFE_AWB_BASE,
		AFE_AWB_CUR, AFE_AWB_END, AFE_ASRC_CON0, AFE_IRQ_CNT2 };
	unsigned long flags;
	unsigned i, value;
	int ret, len = 0;

	ret = pm_runtime_resume_and_get(dev);
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&afe->lock, flags);
	len += sysfs_emit_at(buf, len, "running=%u periods=%lu capturing=%u capture_periods=%lu\n",
			     afe->running, afe->periods, afe->capturing, afe->capture_periods);
	for (i = 0; i < ARRAY_SIZE(regs); i++) {
		ret = regmap_read(afe->regmap, regs[i], &value);
		if (ret)
			break;
		len += sysfs_emit_at(buf, len, "%04x=%08x\n", regs[i], value);
	}
	spin_unlock_irqrestore(&afe->lock, flags);
	pm_runtime_put(dev);
	return ret < 0 ? ret : len;
}
static DEVICE_ATTR_RO(state);

/* Software contract, not a silicon maximum or an electrical I2S capture.
 * No MMIO and no runtime-PM wake: polling diagnostics must not start clocks.
 * Only hw_params success publishes a configured stream; hw_free clears it.
 * ALSA's format width, the 32-bit DMA address mask and serial slots are
 * deliberately separate from the admitted meaningful payload precision.
 */
static ssize_t pcm_contract_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct mt6582_afe *afe = dev_get_drvdata(dev);
	unsigned long flags;
	unsigned rate;
	bool running;

	spin_lock_irqsave(&afe->lock, flags);
	rate = afe->configured_rate;
	running = afe->running;
	spin_unlock_irqrestore(&afe->lock, flags);
	return sysfs_emit(buf,
		"{\"schema\":\"org.y2linux.pcm-contract/v1\","
		"\"evidence\":\"driver_contract\",\"physical_bit_capture\":false,"
		"\"hardware_max_meaningful_bits\":null,"
		"\"supported_modes\":[{\"alsa_format\":\"S16_LE\","
		"\"container_bits\":16,\"meaningful_bits\":16,\"channels\":2,"
		"\"dma_format\":\"interleaved_s16_le\",\"dl1_format\":\"s16_stereo\","
		"\"afe_format\":\"I05_I06_to_O00_O01_s16\","
		"\"afe_interconnect_physical_bits\":null,\"i2s_slot_bits\":32,"
		"\"i2s_payload_bits\":16,\"dac_input_word_bits\":16,"
		"\"rates_hz\":[44100,48000]}],"
		"\"current\":{\"configured\":%s,\"running\":%s,\"rate_hz\":%u,"
		"\"alsa_format\":\"%s\",\"container_bits\":%u,\"meaningful_bits\":%u,"
		"\"dma_format\":\"%s\",\"dl1_format\":\"%s\",\"afe_format\":\"%s\","
		"\"i2s_slot_bits\":%u,\"i2s_payload_bits\":%u,\"dac_input_word_bits\":%u}}\n",
		rate ? "true" : "false", running ? "true" : "false", rate,
		rate ? "S16_LE" : "NONE", rate ? 16 : 0, rate ? 16 : 0,
		rate ? "interleaved_s16_le" : "NONE", rate ? "s16_stereo" : "NONE",
		rate ? "I05_I06_to_O00_O01_s16" : "NONE",
		rate ? 32 : 0, rate ? 16 : 0, rate ? 16 : 0);
}
static DEVICE_ATTR_RO(pcm_contract);
static struct attribute *mt6582_attrs[] = {
	&dev_attr_state.attr, &dev_attr_pcm_contract.attr, NULL
};
ATTRIBUTE_GROUPS(mt6582);
static const struct of_device_id mt6582_match[] = { { .compatible = "mediatek,mt6582-afe" }, {} };
MODULE_DEVICE_TABLE(of, mt6582_match);
static const struct dev_pm_ops mt6582_pm = {
	RUNTIME_PM_OPS(mt6582_runtime_suspend, mt6582_runtime_resume, NULL)
	SYSTEM_SLEEP_PM_OPS(pm_runtime_force_suspend, pm_runtime_force_resume)
};
static struct platform_driver mt6582_driver = {
	.probe = mt6582_probe,
	.driver = { .name = "mt6582-afe", .of_match_table = mt6582_match, .pm = &mt6582_pm,
		    .dev_groups = mt6582_groups },
};
module_platform_driver(mt6582_driver);
MODULE_DESCRIPTION("MT6582 DL1 and second-I2S ASoC playback, FM capture");
MODULE_LICENSE("GPL");
