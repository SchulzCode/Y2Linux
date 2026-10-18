# Headphone output: level, equalizer profiles, DAC filter and measurement

Status 2026-10-18: released in Beta 8.5 (Baseline 93); device results in the
[Beta 8.5 report](validation/Y2-LINUX-1.0-BETA-8.5.md). No output has been measured yet. Audit row HP-LEVEL-EQ in
[roadmap-gap-audit](planning/roadmap-gap-audit.md).

## What the hardware does

The headphone jack is driven by a Cirrus Logic **CS43131** (I2C1 0x30) with a ground-centred class-H amplifier.
Datasheet DS1155F2, HP Output Control 1 (0x80000) OUT_FS:

| Setting | OUT_FS | Full scale | CLASS_H (0xB0000) | Y2Linux level |
| --- | --- | --- | --- | --- |
| 00 | 0x00 | 0.5 Vrms | — | not offered |
| 01 | 0x10 | 1.0 Vrms | 0x1C (HV_EN off) | **Low**: stock headphone level, the default |
| 10 | 0x20 | 1.41 Vrms | 0x1E (HV_EN on) | Medium |
| 11 | 0x30 | 1.73 Vrms | 0x1E (HV_EN on) | High: what Y2Linux used before |

The HV_EN pairing follows the upstream driver's load table (`hv_seq`). The datasheet allows OUT_FS changes only while
the amplifier is powered down (PDN_HP set). It specifies HV_EN = 1 for loads of 600 Ω and more and VP ≥ 3.3 V. Stock never
writes CLASS_H, so stock runs 0x10 with the reset value 0x1E.

Stock (REA of the stock kernel's vendor driver `cs43131_apply_out_fs`): headphones get 0x10, the speaker route 0x30.
It has no impedance measurement. The route comes from jack detection (HP_STATUS / EINT16). Stock's 16-step volume
table tops out at −3 dB, so the stock maximum is about 0.71 Vrms.

## What Y2Linux does now

- **Kernel** (`kernel/patches/0030-cs43130-system-pm.patch`): ALSA control `HP Output Level` (Low/Medium/High) on
  card Y2Audio, and the read-only `HP Output Level Live` (what the output runs at, or `Other`). A requested level is
  written at once while the amplifier is down. Otherwise it is written right before the next power-up, in the
  "HiFi DAC" PRE_PMU event, while PDN_HP is still set. The card ignores pmdown_time, so the amplifier powers down when
  a stream closes. The request lives in the driver, not the register cache, so it survives runtime and system
  suspend. The PDN_HP check and the write run under the card's DAPM lock, so a stream starting at that moment
  cannot power the amplifier in between (audio review, 2026-10-18). The old sysfs switch `y2_hp/hp_gain` uses the
  same path. `y2_hp/hpload_measure` still measures the load but no longer changes the level; it refuses while a
  stream is open (`EBUSY`) and holds the DAPM lock while it measures. Host test: `tests/test_hp_level.py` (C harness on the real driver functions).
- **Reborn**: Settings → Sound → Output → *Headphone Level* sets the control before each wired stream starts. The DAC Master
  is now −19.5 dB instead of −24 dB, so Low plays 0.26 dB below the old level (1.73 Vrms at −24 dB) and never louder.
  Medium is +2.7 dB and High +4.5 dB over the old level. Changing the level during wired playback restarts the
  stream so the new level takes effect, and the first stream after a rise fades in over 2 s. A held pause gives its
  stream back, so the new level starts with Play. A fade carries over when the stream restarts during it (Next, a
  seek), and volume up during it keeps fading. Sound settings that play louder (a profile turned off, a boosting
  preset, Normalize changed) fade in the same way. On a kernel without the control, Master stays at −24 dB.
  At full volume, the output is about 0.106 / 0.149 / 0.183 Vrms (Low / Medium / High, 0 dBFS).
- **DAC Filter** (Settings → Sound → Output): Fast (default, the chip's and stock's), Fast Phase Compensated, Slow, Slow Phase
  Compensated, Nonoversampling. These are the upstream controls `PCM Filter Speed`, `PCM Phase Compensation` and `PCM
  Nonoversample Emulate`, applied at once.
- **Headphone Profile** (Settings → Sound → Equalizer → Headphone Profile): 47 AutoEQ profiles of common headphones
  and IEMs come with the image (`/usr/share/reborn/eq`, Reborn `assets/eq/SOURCES.md`, MIT). More: put AutoEQ
  `ParametricEQ.txt` files in a folder named `EQ` on Internal Storage or the SD card; a file there replaces a
  shipped profile of the same name. Up to 10 filters: PK (peaking), LSC (low shelf) and HSC (high
  shelf), with Q. These are the RBJ cookbook biquads that Equalizer APO and AutoEQ use, played by FFmpeg's
  `equalizer`, `lowshelf` and `highshelf`. The preamp is the file's own, or lower when the filters' combined response
  still rises above 0 dB between 10 Hz and Nyquist at any source rate from 44.1 to 384 kHz (the filters run before
  resampling; AutoEQ's HD 650 file says −6.1 dB; Reborn uses −6.4 dB). For files below 44.1 kHz FFmpeg skips filters
  above Nyquist and the limiter catches the rest. The
  limiter stays after the filters. Reborn keeps the imported profile when the file is removed. Choosing a preset
  turns the profile off.

## Measurement plan

Goal: measure the real output of each level into realistic loads, and find out where stock's bass roll-off comes from.

### Equipment

- An audio interface or analyser whose input takes ≥ 2 Vrms without clipping (or a known attenuator); REW, ARTA or
  an APx/QA403-class analyser.
- Load resistors **16 Ω, 32 Ω and 300 Ω** (and **open circuit**), with a 3.5 mm breakout. Optionally one real IEM
  and one real headphone, through a coupler or as an electrical load.
- The player on battery and **USB unplugged** (no ground loop, no charger noise). One extra pass with the charger
  connected shows charger noise.

### Player settings

Y2Linux build with the level control (the next test build). Then:
Equalizer **Flat**, Normalize Volume **off**, Crossfade **off**, DAC Filter **Fast**, Output **Headphone Jack**,
**volume 100** (Reborn's volume is digital; 100 = 0 dB). Copy `Y2 Headphone Test Signal.flac`
(`tools/audio/test_signals.py`, timeline in its `sections.txt`) to Internal Storage and play it once per level and load.

| Time (s) | Section | Use |
| --- | --- | --- |
| 2–4 | 1 kHz −20 dBFS marker | sync |
| 4–34 | digital silence | idle noise (unweighted and A-weighted, 20 Hz–20 kHz) |
| 34–44 | 1 kHz −20 dBFS | level, THD+N, output impedance (loaded vs open) |
| 44–49 | 1 kHz −1 dBFS | highest output at this level, THD+N at full scale |
| 49–57 | 1 kHz −20 dBFS left only, then right only | crosstalk |
| 57–62 | 50 Hz −20 dBFS | bass level and bass output impedance (capacitor check) |
| 64–74, 76–86 | log sweep 20 Hz–20 kHz −12 dBFS | frequency response |

REW users can also export REW's own sweep file and play it from the player (file playback mode).

### Matrix and results

For **Low, Medium and High** × **open, 300 Ω, 32 Ω and 16 Ω**, record:

1. Vrms at 1 kHz −20 dBFS and −1 dBFS. Output impedance from open vs loaded: Zout = R_load × (V_open / V_load − 1),
   at 1 kHz and at 50 Hz.
2. Frequency response, 20 Hz–20 kHz, left and right, normalised at 1 kHz (export as CSV or REW .mdat).
3. Idle noise: µV unweighted and A-weighted, plus the spectrum (hum, charge-pump or class-H tones).
4. THD+N at −20 and −1 dBFS, and crosstalk.
5. The same matrix on **stock firmware** (top volume step), at least FR and 1 kHz level into 16 Ω and 300 Ω.

### Reading the results

- **Bass roll-off that depends on the load** (worse into 16 Ω than 300 Ω, and the 50 Hz Zout higher than at 1 kHz)
  means an analog series capacitor or resistor on the board. It is then the same on stock and Y2Linux. The kernel's
  load measurement already reads about 102 Ω high on this board (commit `c852008`), so check series resistance.
- **Roll-off on stock only** means Android audio processing, which Y2Linux bypasses.
- **Roll-off the same into every load** comes from the digital path or the DAC filter. Compare DAC Filter settings.
- **Level changes with the load beyond what Zout explains** would be real "gain switching". The stock kernel has no
  load measurement, so stock should not do this.

### Building a player correction

1. Average L/R per level and load and normalise at 1 kHz; the deviation from flat is the player's own response.
2. A **load-independent** deviation becomes one fixed correction for the wired output, a low shelf or one or two
   peaking filters, fitted the same way AutoEQ fits headphones.
3. A **load-dependent** deviation (series C/R) cannot be fixed by one EQ. Fit R and C from the curves:
   H(f) = Z_L / (Z_L + R + 1/(j2πfC)). Then compute the inverse for a load class (IEM ≈ 16–32 Ω, headphone ≈ 32–300 Ω)
   and approximate it with one or two biquads. The load class comes from a user choice or, once jack detection
   works (#11/#12), from the kernel's `hpload_measure`.
4. Reborn would apply this "player correction" before the headphone profile (≤ 2 filters plus AutoEQ's 10; the
   engine limit would rise to 12), with the clip-safe preamp covering both. Re-measure with the correction on.

Safety: measure into loads, never with headphones on someone's head while testing levels or Master changes. A
developer-only full-scale test can raise the DAC Master over SSH once playback has started
(`amixer -c Y2Audio sset Master 0dB`; Reborn sets Master again at every stream start). That plays up to 1.73 Vrms,
about 19.5 dB above Reborn's maximum.
