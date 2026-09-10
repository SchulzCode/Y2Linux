# Persistent audio tools — physical SD installation

Owner authorized permanent installation, then confirmed AUDIO-02 was flashed
and connected. SSH verified kernel6.18.0-y2linux-m3audio02, Y2Audio card/PCM,
writable ext4 root on removable host11240000 and exact matching libc/libm/loader.
The canonical audio-sd-update.py installed the already-built Buildroot ALSA files
into standard system paths. install.txt retains prerequisite/archive/installed
hash checks and enumeration; verify.txt is a new SSH session running the
installed diagnostic collector and verifying paths belong to the SD filesystem.

No reboot, flash, internal eMMC write, raw register access, playback or mixer
mutation. Headphone off after installation. Persistence is established by the
files residing on synced SD ext4; a reboot was not performed for this check.
Clean AUDIO-02 playback remains to be qualified. USB reconnect #27 stays deferred.
See ../../knowledge/audio-tools-sd.md for reproduction and source ownership.
