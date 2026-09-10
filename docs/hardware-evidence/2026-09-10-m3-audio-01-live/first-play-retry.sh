set -eu
cd /tmp/y2-m3-audio-aa3c949
trap './amixer -c Y2Audio sset Headphone off >/dev/null 2>&1' EXIT
./amixer -c Y2Audio sset Headphone off
./amixer -c Y2Audio cset name='PCM Ch Select' 0
./amixer -c Y2Audio sset Master 207,207
./amixer -c Y2Audio sget Master
./amixer -c Y2Audio sset Headphone on
cat /proc/uptime
cat /proc/interrupts
./aplay -v -D hw:CARD=Y2Audio,DEV=0 --period-size=1024 --buffer-size=8192 audio/headphone-44100.wav &
play_pid=$!
(sleep 8; printf "PLAYBACK_DEADLINE\n"; kill -TERM "$play_pid" 2>/dev/null || :) &
guard_pid=$!
for n in $(seq 1 16); do
    printf '\nSAMPLE %s\n' "$n"
    cat /proc/uptime
    cat /sys/bus/platform/devices/11220000.audio-controller/state
    cat /proc/asound/card0/pcm0p/sub0/status
    cat /proc/asound/card0/pcm0p/sub0/hw_params
    sleep 0.25
done
status=0
wait "$play_pid" || status=$?
kill "$guard_pid" 2>/dev/null || :
wait "$guard_pid" 2>/dev/null || :
printf '\nAPLAY_EXIT=%s\n' "$status"
./amixer -c Y2Audio sset Headphone off
cat /proc/interrupts
cat /sys/kernel/debug/regulator/regulator_summary
cat /sys/bus/platform/devices/11220000.audio-controller/state
dmesg
exit "$status"
