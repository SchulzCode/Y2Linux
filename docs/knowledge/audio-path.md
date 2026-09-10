# Audio and FM evidence

**Current physical checkpoint:** AUDIO-02 now produces owner-confirmed clean
S16 stereo44.1kHz headphones. The notification fix is deployed and tested.
[Current result and exact limits](m3-audio-01-live-result.md) supersede the
historical untested/gated claims below.48kHz/LR/restart checks remain; M4/M5
and high-resolution implementation have not started.

**2026-09-10 M3 ACTIVE:** owner authorizes one integrated headphone-first ALSA
candidate, with #27 reconnect explicitly deferred. [Implementation and source
review](m3-audio-architecture.md); [deployment](../build/y2linux-m3-audio-01-deployment.md).
Earlier gated wording below is historical. Physical M3 acceptance remains open.

> 2026-09-10: [porting history and failures](reverse-engineering-audit.md)
> corroborate external CON3/DL1 and distinguish digital DAC probe success from
> VGP2-powered analog operation. Their observed amplifier is OCA72559 ID09;
> our revision/rails/jack/clock accuracy remain untested. Capture M3 tests for
> DMA/IRQ continuity, supply brownout and scheduling underruns separately.
> M3 remains gated; the wired ALSA/Rust/C plan is unchanged.

## Donor integration input — 2026-09-09

M3 remains gated on M2. [Complete reuse audit](donor-audit.md) and the immutable
`donorSource/sound/soc/mediatek/mt6582/` now replace discovery from zero as the
starting point. None of these donor claims is a new audio measurement on our Y2.

| Contract | Donor implementation to preserve/review for v6.18 |
| --- | --- |
| AFE | `11220000`, SPI104 level-low; DL1 base/current/end offsets40/44/48, IRQ control/status/clear/count at3a0/3a4/3a8/3ac. Uses shared MediaTek memif/IRQ framework and 32-bit DMA mask. |
| DAI | DL1 I05/I06→O00/O01; external second-I2S uses CON3 offset4c and FPGA_CFG1 bit4. 32-bit slots, 64 bit clocks per frame, rate table8–48kHz; this is a driver limit, not a proved hardware maximum. |
| Clock/PM | infra_audio, audintbus and audio CCF handles. Review runtime-resume failure unwinding, regmap errors and IRQ/substream lifetime against the v6.18 common AFE implementation. |
| CS43131 | Upstream cs43130 driver, I2C1:30, IRQ16, 22.5792MHz crystal; machine sets serial clock to rate×64. VGP2 plus GPIO20→18 and GPIO15 sequencing need electrical/supply validation. |
| Amp | I2C1:58, enable8; AW87559 ID5a / alternative OCA72559 ID09. Tables and enable delays retained in donor aw87559.c. Add mute/drop-enable on write failure and safe shutdown. |
| Routing | Headphones use HPOUTA/B; amplifier input takes both. Speaker DAPM event writes DAC PCM_PATH_CTL_2 mask7, mode5 versus stereo0. Independent switches do not enforce headphone/speaker exclusion; review jack integration, event return and pop-safe ordering. |
| FM | ASRC/gain2/32kHz route is donor reference only. Owner confirms the older physical Y2 lacks usable FM reception hardware. Do not enable this path or make reception an exit requirement for this board. |

The 6.12 machine driver uses card/DAI structures and shared static mutable state
that must be adapted to 6.18, with proper DT reference/device lifetime and
resource ownership. Real ALSA playback, rate/format/IRQ/DMA integrity and analog
levels remain M3 tests before player features. Original historical evidence follows.

Date: 2026-09-08. Related task: Y2E-101; archived evidence reviewed, no active audio investigation in this session.

CONFIRMED in the integrity-verified `2026-07-29_004935` snapshot: `1-0030` binds `cs43131_dac`, `1-0058` binds `aw87559_pa`, `/proc/asound/cards` reports no soundcards. These observations identify a vendor-controlled system; `/dev/cs43131_dac` is not established as a PCM sink.

The exact archived stock primary HAL hashes to `5c5162f6a68f7db57febd050ee88cc886779dcce5948937149d6cd211eb0e6de`. Later `Y2_AUDIO_PATH_PHASE4_SECOND_I2S.md` and `out/afe-runtime/2026-07-30_135519/` supersede parts of earlier primary-I2S analysis and support a second-I2S/DL1 route. Treat the complete electrical/analog route, PMIC role, actual slot width, supported rates and analog performance as INFERRED/UNKNOWN until their specific evidence is reviewed. Android's observed 44.1-kHz PCM16 output does not set a Linux hardware maximum.

The current generated boot manifest records a CS43131 idle-resume kernel patch. Its installed presence is UNKNOWN. Avoid conflating that artifact with the exact original kernel analyzed offline.

FM feasibility reports record chip ID 0x6627 and tuning/routing experiments. A successful open/tune is not evidence of useful RF reception or a populated antenna path on this unit. Release notes distinguish hardware revisions, but the current unit's revision and original manufacturer evidence have not been established here.

Next passive proof: re-identify bound audio devices on the current build and reconcile exact HAL/kernel/firmware lineage. Later pin down reset/IRQ/supply/mute/clock ordering and headphone/speaker/PMIC topology before designing ASoC work. No raw node writes, guessed ioctls, live register dumps, bus scans, playback/rate changes or kernel patches are authorized by this document.
