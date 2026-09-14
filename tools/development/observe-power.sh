#!/bin/sh
# Read-only, bounded observation of production power interfaces.
# Run through authenticated SSH; this process never controls the charger.
set -eu
count=${1:-60}
interval=${2:-5}
case "$count:$interval" in *[!0-9:]*|:*|*:) exit 2;; esac
[ "$count" -ge 1 ] && [ "$count" -le 120 ]
[ "$interval" -ge 1 ] && [ "$interval" -le 10 ]
bat=/sys/class/power_supply/BAT0
printf 'KERNEL '
uname -r
printf 'BOOT_COMMAND_LINE '
cat /proc/cmdline
printf 'ONLINE_CPUS '
cat /sys/devices/system/cpu/online
cat /proc/mounts
n=0
while [ "$n" -lt "$count" ]; do
    printf 'SAMPLE %s UPTIME ' "$n"
    cat /proc/uptime
    cat "$bat/uevent" "$bat/charging_state"
    for supply in /sys/class/power_supply/*; do
        [ "$supply" = "$bat" ] && continue
        printf 'SUPPLY %s\n' "${supply##*/}"
        cat "$supply/uevent"
    done
    for zone in /sys/class/thermal/thermal_zone*; do
        [ -f "$zone/type" ] || continue
        printf 'THERMAL '
        tr '\n' ' ' < "$zone/type"
        cat "$zone/temp"
    done
    for policy in /sys/devices/system/cpu/cpufreq/policy*; do
        [ -f "$policy/scaling_cur_freq" ] || continue
        printf 'CPU_KHZ '
        cat "$policy/scaling_cur_freq"
    done
    for light in /sys/class/backlight/*; do
        [ -f "$light/brightness" ] || continue
        printf 'BACKLIGHT '
        cat "$light/brightness"
    done
    n=$((n+1))
    [ "$n" -ge "$count" ] || sleep "$interval"
done
printf 'END_SAMPLES %s\n' "$n"
dmesg
