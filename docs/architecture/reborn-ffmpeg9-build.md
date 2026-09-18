# Reborn FFmpeg 9.0.1 production build

Y2Linux pins the Reborn media build to FFmpeg **9.0.1**. Buildroot supplies
`libavformat`, `libavcodec`, `libavutil`, `libavfilter`, `libswresample` and
`libswscale`; the FFmpeg command line tools, `libavdevice`, network protocols,
capture devices, hardware autodetection, post-processing and unrelated video
families are disabled. Encoders and muxers are empty because Reborn only
reads and processes local media. `libswscale` remains for JPEG, PNG and WebP
artwork conversion.

The component lists live in
[`y2_production_defconfig`](../../buildroot/configs/y2_production_defconfig).
`tools/production/verify-ffmpeg.py` checks the generated FFmpeg
`config_components.h`, generated `config.h`, target library SONAMEs, and CLI
and `libavdevice` absence after Buildroot finishes. The ARM runtime manifest is
checked by `Y2Reborn/tools/build/qemu-check.py`; it records the actual FFmpeg
and library versions rather than trusting the Buildroot defconfig.

The ALSA qualification profile intentionally prefers `S32_LE` but currently
allows only the owner-confirmed `S16_LE`, stereo, 44.1 kHz wired combination.
The MT6582 driver and CS43131 path have not been physically qualified for
S32_LE or higher rates in the retained evidence. Reborn therefore logs and
performs an explicit fallback to the qualified combination instead of
advertising an untested format.
