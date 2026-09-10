# AUDIO-02 first controlled headphone playback

Owner requested another headphone test after flashing AUDIO-02 and installing
permanent ALSA tools. Used standard aplay, stereo S16_LE44100, period1024,
buffer8192, three-second faded -30dBFS fixture and Master207=-24dB.

Result: PLAY_START250.05, PLAY_END253.18 (3.13seconds including setup/drain),
aplay exit0, no deadline or reported underrun. AFE interrupts/period count0->130;
ALSA hw_ptr advances across repeated ring wraps (8864->127924 in samples).
Second-I2S/DL1 active during playback, stopped afterward. Headphone off on exit.
VGP2 remains1.8V. No new DAC reset/I2C/PLL-ready error or kernel crash observed.
Regulator summary logs unsupported get_mode on VCAMA/VRTC as before playback;
these are unrelated read-only diagnostic warnings.

Owner confirms: "perfect, no clicking!" This establishes clean audible headphone
output for this short 44.1kHz/S16 stereo run and supports the notification fix.
Repeated/stop-start runs and48kHz remain required before M3 wired-audio acceptance.
Native24/32-bit PCM and higher rates are not implemented or qualified. No new kernel
build, reboot, flash, speaker activation or USB reconnect experiment performed.

before.txt and play-44100.txt retain command outputs; play-44100.sh is the exact
bounded test with cleanup/watchdog. SHA256SUMS covers this evidence.
