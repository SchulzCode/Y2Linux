#!/bin/sh
set -eu
play_pid=
guard_pid=
monitor_pid=
cleanup() {
    [ -z "$play_pid" ] || kill -TERM "$play_pid" 2>/dev/null || :
    [ -z "$guard_pid" ] || kill -TERM "$guard_pid" 2>/dev/null || :
    [ -z "$monitor_pid" ] || kill -TERM "$monitor_pid" 2>/dev/null || :
    amixer -c Y2Audio sset Headphone off >/dev/null 2>&1
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
amixer -c Y2Audio sset Headphone off
amixer -c Y2Audio cset name='PCM Ch Select' 0
amixer -c Y2Audio sset Master 207,207
amixer -c Y2Audio sget Master
cat /proc/interrupts
amixer -c Y2Audio sset Headphone on
printf '\nPLAY_START '
cat /proc/uptime
aplay -v -D hw:CARD=Y2Audio,DEV=0 --period-size=1024 --buffer-size=8192 /usr/share/y2linux/audio/headphone-44100.wav &
play_pid=$!
(sleep 6; echo PLAYBACK_DEADLINE; kill -TERM "$play_pid" 2>/dev/null || :) &
guard_pid=$!
(
 for n in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16; do
    printf '\nSAMPLE %s ' "$n"
    cat /proc/uptime
    cat /sys/bus/platform/devices/11220000.audio-controller/state
    cat /proc/asound/card0/pcm0p/sub0/status
    sleep 0.25
 done
) &
monitor_pid=$!
status=0
wait "$play_pid" || status=$?
play_pid=
printf '\nPLAY_END '
cat /proc/uptime
kill "$guard_pid" "$monitor_pid" 2>/dev/null || :
wait "$guard_pid" 2>/dev/null || :
wait "$monitor_pid" 2>/dev/null || :
guard_pid=
monitor_pid=
printf '\nAPLAY_EXIT=%s\n' "$status"
amixer -c Y2Audio sset Headphone off
cat /proc/interrupts
cat /sys/bus/platform/devices/11220000.audio-controller/state
cat /sys/kernel/debug/regulator/regulator_summary
dmesg
exit "$status"
