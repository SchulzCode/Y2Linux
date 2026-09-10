# M3-AUDIO-01 first physical result

The real Y2 now probes the standard ALSA card/PCM and CS43131 correctly, with
VGP2 at1.8V and the speaker disabled. **PCM notification is defective; M3 remains
open.** [Full evidence and reproduction](../hardware-evidence/2026-09-10-m3-audio-01-live/).

| Subsystem | Physical result | Next action |
| --- | --- | --- |
| Kernel/core | M3-AUDIO-01 boots; SSH/SD, four CPUs, MemTotal954384KiB | Retain baseline; no repeated M2 pass |
| ALSA/codec probe | Y2Audio card, PCM0; CS43131 revisionA1 | Class A probe gate passes |
| Power | VGP2=1.8V, enable20/18/15 high; speaker8 low; no observed DAC reset during first run | Retain sequencing; longer playback still unqualified |
| Second I2S / DMA | CONN0=00400020, CON3=90b while playing, CUR advances;327IRQ events | Register/DMA evidence is not pad/audio-quality proof |
| PCM accounting | Three-second WAV still running after4seconds; lost ALSA pointer wraps | Correct positive-success handling in startup; new kernel needed |
| Rootfs utilities | SD remains DEV-01; ALSA tools missing | Validated tools staged in /tmp through a reproducible bundler; persistent SD update remains outstanding |
| Audible headphones | Owner heard sound with clicking during repeat | Audible path confirmed; clean playback remains unqualified |

The new regression invokes actual production mt6582_open with constraint return
values0,+1 and negative errors. It fails before the correction and passes after.
The driver must attach the substream after either nonnegative success, allowing
its existing IRQ handler to notify ALSA. No route/voltage/clock changes accompany
this fix. Source-only tests do not qualify the corrected kernel on hardware.
