# Read-only wired audio observer

Build `y2_audio_observer.ko` as an external module against the installed
kernel's exact release, configuration, upstream headers and Module.symvers.
The completed physical run used Linux 6.18.0-y2linux-platform-v1-baseline-01
and configuration SHA256
`368105827a8c70ddfcec9a95ffc425bad27e21b4f4bda6ae8dedb11f3890fe0a`.

Hold `/tmp/y2-platform-v1-device.lock` for the complete device session. Check
`CONFIG_MODULE_UNLOAD` **before loading**. This installed kernel disables
unloading; a normal reboot is necessary to remove any loaded diagnostic module.
Plan that cleanup within the authorized device session. Never force removal.

The root-only file `/sys/kernel/debug/y2_audio_observer/highres` reports the
ALSA runtime, a bounded instantaneous DMA snapshot, and fixed audio register
allowlists. It uses normal PCM locks, regmap physical reads and existing active
runtime PM references. Closed/suspended paths are skipped; it neither programs
registers nor wakes them. DMA writes can run concurrently with observation:
compare repeating deterministic fixtures, not arbitrary music. The snapshot
does not capture external I2S or measure analog precision.

The allowlists and the DL1 source contract are explained in
[the completion report](../../../docs/validation/Y2-HIGHRES-AUDIO-COMPLETION.md).
`AFE_MEMIF_MON0` exposes documented left/right 16-bit monitor fields. Their
width does not establish the full internal bus width in every silicon mode;
`AFE_MEMIF_PBUF_SIZE[19:16]` is the prefetch threshold, not an HD fetch mode.
The initial live module's `dl1_output_bits_per_channel=16` label was an
overinterpretation, corrected to `dl1_monitor_bits_per_channel=16` in source.
Raw physical register/DMA observations remain valid independently of that label.

Generate the conservative fixtures without playing anything:

```sh
python3 tools/host/audio_precision.py out/audio-fixtures --live-safe
```

They peak below -60 dBFS. Keep the mixer conservative, mute before configuring
formats, and use direct `hw:Y2Audio,0` for capability tests. `plughw` conversion
must be identified explicitly. Stop the test process, restore the saved mixer,
remove temporary files and unload the observer, or reboot when unloading is
disabled. Loading an external GPL module adds the standard out-of-tree taint.
