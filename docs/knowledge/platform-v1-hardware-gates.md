# Platform v1 source and hardware boundaries

2026-09-23 software review; no physical access. Exact primary-source URLs and
hashes are in [the research inventory](platform-v1-feasibility-sources.json).
The retained [GPU-01 receipt](../hardware-evidence/2026-09-18-gpu01/README.md)
contains narrow S16/44.1 playback and an adverse same-boot suspend result.
Later source repairs do not supersede that physical failure without a new test.

## CPU, idle, memory and suspend

Keep 598/747.5/1040 MHz OPPs, the existing fixed voltage, WFI-only cpuidle,
NO_HZ_IDLE/HZ=100, reserved regions and current thermal trip points. Collect
frequency/idle residency, per-core load, interrupts/context switches, RSS/PSS,
LOWMEM/HIGHMEM, page cache/slab, die temperatures and XRUNs for idle UI, screen
off, FLAC16/44, 24/96 downsampling, EQ/crossfade, scan/artwork, Wi-Fi and SBC.
No available measurement answers whether 598 MHz or fewer cores meet deadlines.
The owner may compare supported governor/frequency policies after a baseline;
do not infer a voltage change or copy one from a donor.

The pinned MediaTek [idle policy](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_idle.c)
requires a single online CPU, clock exclusions, timer budget and early-suspend
conditions. Its [SPM implementation](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_spm_sleep.c)
has a distinct deep-idle PCM program, PMIC variants, UART handoff, interrupt/CIRQ
handling and CPU context/timer restoration. Those are not established for the
Y2 board by the existing full-suspend program. Deeper cpuidle is
BLOCKED_BY_EVIDENCE / PHYSICAL_GATE; no speculative SPM code is added.

`y2-suspend` now refuses by default. The explicit `--owner-qualify` escape is
only for the separately controlled resume experiment, never automatic UI idle.
It retains the existing exclusive activity lock, radio-off checks and restoration.
Reborn ALSA handles already hold a shared `/run/y2/activity.lock` for their whole
lifetime; `y2-radio-hold COMMAND...` provides the same scoped lease for transfer
work. Applications may use a shared nonblocking flock directly, close on exit,
and treat an unavailable/exclusive lock as refusal. A lease is not proof of safe
suspend; ordinary Platform v1 suspend stays disabled. Do not run the experiment
with unsaved work, active update/maintenance or an unproven wake/recovery path.

## Internal audio and CS43131

Current AFE DMA consumes interleaved stereo S16, with direct hardware rates
44.1/48 kHz; the product allowlist remains S16/44.1. The I2S serializer uses
32-bit slots, which proves neither wider DMA fetch nor preserved source bits.
The reviewed pinned vendor [AFE header](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/sound/mt6582/AudDrv_Afe.h)
and associated source expose rate/channel/buffer configuration and independent
I2S word length. They do not establish a Y2 DL1 high-definition fetch/alignment
and interconnect path. Absence here is not proof the silicon cannot support it.
Registers from newer MT8167/MT8173 hardware are not interchangeable evidence.

The pinned Linux 6.18 `sound/soc/codecs/cs43130.c` supports CS43131 clock families
and wider formats, but that alone cannot qualify MT6582 fetch/clocking or board
analog topology. Existing hardware volume/filter controls remain available.
Load-measurement DT properties are absent and automatic impedance/gain policy
remains disabled. S24/S32, preserved 24-bit and 88.2/96 kHz are unavailable,
BLOCKED_BY_EVIDENCE / PHYSICAL_GATE, until the complete path is established.

`y2-audio-contract` queries combinations without committing hw_params or writing
PCM. An occupied direct PCM may return Unavailable. Its `--device null` option
is host/QEMU validation only. `tools/platform/audio_precision.py OUTPUT` creates
24-bit WAV plus exact S24-in-32/S32 references and independent channel patterns;
nonzero low bits reveal S16 truncation. It never plays them. After a future
source-backed driver change, owner acceptance must compare DMA/sample words and
logic captures, MCLK/BCLK/LRCLK at each supported rate, both channels, switching,
XRUNs and analog pop/click behavior. Do not activate unsupported masks or rates
to try these fixtures. Packed 24-bit file decoding already works; that is a
different capability from high-resolution internal output.

## Reset evidence and watchdog

Previous boot ID/stage/orderly preparation and private bounded syslog tails are
captured before this boot starts logging, once per boot. The status API exposes
only metadata/hashes; `y2-platform boot-evidence` retrieves the private record.
There is no claim that a panic reached persistent storage. Syslog rotation is
bounded; no extra polling logger or reserved RAM was added. An unclean previous
boot remains an observation, not a diagnosed watchdog/panic or electrical reset.

The existing Linux 6.18 MediaTek AP watchdog driver supports normal watchdog
operations but does not populate a proven MT6582 reset-cause register. Its
bootstatus zero is not an affirmative reset diagnosis. AP watchdog arming stays
disabled; the charger watchdog is unrelated. Ramoops/pstore needs proven retained
RAM placement across stock loaders; no reserved region is reclaimed or guessed.

Before an AP watchdog policy can ship, owner qualification must establish timeout
clock, disable/magic-close behavior, actual reset and storage/loader recovery,
then a durable bounded boot-attempt budget and rescue escalation. First test
normal keepalive/disarm on a recoverable qualification image; only a separately
approved session may stop keepalives. Do not make an untested watchdog the
production recovery path or repeatedly reset after an unknown boot failure.

USB host/OTG remains unavailable until connector ID/role wiring, VBUS switch,
source current and PHY behavior are established. No VBUS path is enabled.
