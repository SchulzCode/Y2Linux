# Reborn local-video dependencies

This is a source candidate. No video resolution, frame rate, synchronization or battery result has
been qualified on a physical Y2 for this change.

The paired Y2Reborn video controller reuses the media membrane, ALSA/Bluetooth transport, storage
guards, skin interpreter and GLES renderer. The upstream plugin host is not implemented yet; no
plugin ABI or system service is introduced. The controller is an application feature boundary that
can be moved behind a future official plugin interface.

Buildroot must rebuild FFmpeg with `h264` and `vp8` decoders and parsers and the `matroska` demuxer.
MOV already supplies MP4, and AAC/Opus are existing audio decoders. VP8 was previously an implicit
dependency of WebP artwork; it is now explicit. Network protocols, device capture, encoders, muxers
and FFmpeg command-line tools stay disabled. HEVC, VP9, AV1 and hardware video decoding are outside
this candidate. Rebuild Reborn and its `libreborn_media.so` together; do not copy the new application
onto an older media membrane.

Reborn limits decoded source pictures to 414,720 pixels (720×576 in area), uses two codec threads, converts to at most
480×360 RGBA, and uses a bounded picture queue. These are admission/resource bounds, not playable
performance promises. The audio sink's reported audible position drives video presentation. A stalled
or paused audio clock cannot advance pictures.

Use the normal production Buildroot integration and run `tools/production/verify-ffmpeg.py
--buildroot-output OUT` and Reborn's `tools/build/qemu-check.py` against the resulting ARM rootfs.
The verifier now requires H.264/VP8 and Matroska in generated component headers and the runtime
manifest. Host development libraries do not qualify the target package.

Before release, test the paired image on the Y2 with synthetic motion/audio patterns: baseline and
high-profile H.264, variable frame rate, long seeks, anamorphic and portrait files, sustained memory,
late frames, Bluetooth latency/output changes, unplug/eject, repeated sleep/wake and battery use.
Record exact image/build identities, limits and measured results. The standing roadmap audit has
not promoted any hardware or release milestone for this source change.
