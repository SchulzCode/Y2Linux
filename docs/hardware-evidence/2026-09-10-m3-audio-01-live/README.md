# M3-AUDIO-01 live entry and first playback

Owner reports manual update/connection and confirms headphones plugged in.
Running kernel: 6.18.0-y2linux-m3audio01; source aa3c949. Full BOOTIMG was not
read back. Kernel release/probe behavior agrees with the candidate.

Observed: Y2Audio card and PCM0; CS43131 ID43131 revisionA1; VGP2 constrained from
2.5V to1.8V, GPIO20/18/15 high, speakerGPIO8 low; four CPUs, MemTotal954384KiB,
removable SD root. I2S inherited muxes GPIO43/44 mode4,46 mode3.

SD still identifies DEV-01; aplay/amixer are absent. No full rootfs replacement
was performed. tools/development/audio-live-bundle.py reproducibly packages the
built M3 utilities/libraries/fixtures under /tmp/y2-m3-audio-aa3c949. Target file
hashes pass. An initial minimal ALSA config lacked ctl.sysdefault; it was fixed
in the canonical bundler and restaged. The staged tools now list all mixer and
PCM controls. No change to /etc/y2linux/build-id or existing system libraries.

First playback attempt did not start because installed rootfs lacks timeout.
The retry used a shell watchdog, stereo S16_LE44100, period1024/buffer8192,
Master207=-24dB, three-second -30dBFS fixture. Headphone endpoint is disabled on
exit. No raw codec/PMIC writes or speaker enable, kernel rebuild/flash/reboot or
USB reconnect experiment occurred in this live pass.

The retry exits0 and records327AFE interrupts, CONN0=00400020, CON3=0000090b,
DMA ring845c8000..845cffff and advancing CUR, then CON3=0000090a at shutdown.
No new I2C/PLL-ready/brownout/kernel errors appear. However playback is still
RUNNING more than4seconds after trigger for a3second fixture, and ALSA hw_ptr
loses ring wraps. Do not call this stable PCM or clock-accuracy acceptance.
Owner reports audible sound with clicking during the requested repeat. The
repeat log records PLAYBACK_DEADLINE: its exit0 does not prove completion.
Missing period notifications are a plausible cause of stale-buffer playback;
clean output must be retested with the corrected kernel.

Source investigation identified mt6582_open treating positive ALSA constraint
success as an early exit. Linux6.18 snd_interval_setinteger returns1 when it
sets the integer constraint. That skips afe->substream assignment while ASoC
accepts the positive return; IRQs count but period_elapsed has no stream.
The production-startup regression fails on aa3c949 and passes after changing
checks to ret<0. Existing trigger/IRQ/DMA/clock tests also pass. This is a
kernel PCM notification/lifetime defect (class B in the audio A–G taxonomy),
not evidence to retune clocks or change CON3/VGP2. Source fix is not running
on the device and requires a subsequent owner-deployed kernel.

Raw outputs and exact scripts are retained here; SHA256SUMS covers them.
M3 remains open. Speaker/jack/rate accuracy are unqualified; #27 stays deferred.
