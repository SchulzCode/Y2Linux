#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# Run on the Y2 only AFTER the owner deploys Y2LINUX-M4-01. Each phase is explicit.
set -eu
[ "$(uname -r)" = 6.18.0-y2linux-m4-01 ] || { echo 'M4-01 kernel required' >&2; exit 1; }
awk '$2=="/" && $3=="ext4" && $1=="/dev/mmcblk0p5" {ok=1} END {exit !ok}' /proc/mounts
awk '$2=="/data" && $3=="ext4" && $1=="/dev/mmcblk0p7" {ok=1} END {exit !ok}' /proc/mounts
record=/data/y2linux-m4-01
policy=/sys/devices/system/cpu/cpufreq/policy0
mkdir -p "$record"
show() { for f in "$@"; do [ -f "$f" ] || continue; printf '\n%s\n' "$f"; cat "$f" || true; done; }
thermal_guard() {
    found=0
    for z in /sys/class/thermal/thermal_zone*; do
        [ -f "$z/type" ] || continue
        case "$(cat "$z/type")" in cpu-thermal|pmic-thermal)
            t=$(cat "$z/temp") || return 1
            [ "$t" -ge -10000 ] && [ "$t" -lt 60000 ] || { echo "Test temperature stop: $z $t" >&2; return 1; }
            found=$((found+1));;
        esac
    done
    [ "$found" -eq 2 ] || { echo 'Both calibrated thermal zones required for load' >&2; return 1; }
    v=$(cat /sys/class/power_supply/BAT0/voltage_now) || return 1
    [ "$v" -ge 3400000 ] || { echo 'Low-voltage test stop; recharge through established stock procedure' >&2; return 1; }
}
idle() {
    show /sys/devices/system/cpu/cpu*/cpuidle/state*/name /sys/devices/system/cpu/cpu*/cpuidle/state*/usage /sys/devices/system/cpu/cpu*/cpuidle/state*/time
}
inventory() {
    uname -a
    show /proc/sys/kernel/random/boot_id /proc/uptime /sys/devices/system/cpu/online
    awk '/MemTotal/ {print}' /proc/meminfo
    cat /proc/mounts /proc/partitions /proc/asound/cards /proc/bus/input/devices
    show /sys/class/block/mmcblk0p*/start /sys/class/block/mmcblk0p*/size
    show /sys/class/power_supply/*/uevent /sys/class/thermal/thermal_zone*/type /sys/class/thermal/thermal_zone*/temp
    show "$policy"/affected_cpus "$policy"/scaling_available_frequencies "$policy"/scaling_cur_freq "$policy"/scaling_governor
    idle
    show /sys/power/state /sys/power/mem_sleep /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/wakealarm
    show /sys/class/drm/card*-*/status /sys/class/backlight/*/brightness /sys/class/backlight/*/actual_brightness
    show /sys/kernel/debug/regulator/regulator_summary /sys/kernel/debug/clk/clk_summary /sys/kernel/debug/wakeup_sources
    cat /proc/interrupts
    dmesg
}
case "${1:-inventory}" in
inventory) inventory;;
begin)
    [ ! -f "$record/boot-before" ] || { echo 'Existing run retained; use inventory to resume it' >&2; exit 1; }
    cat /proc/sys/kernel/random/boot_id > "$record/boot-before"
    dd if=/dev/urandom of="$record/sentinel" bs=4096 count=64
    (cd "$record"; sha256sum sentinel > sentinel.sha256)
    sync
    inventory > "$record/begin.log" 2>&1
    cat "$record/begin.log";;
audio)
    amixer -c Y2Audio sset Headphone off
    trap 'amixer -c Y2Audio sset Headphone off' EXIT
    trap 'exit 130' HUP INT TERM
    amixer -c Y2Audio cset name='PCM Ch Select' 0
    amixer -c Y2Audio sset Master 207,207
    for round in 1 2; do
        amixer -c Y2Audio sset Headphone on
        timeout 6 aplay -D hw:CARD=Y2Audio,DEV=0 --period-size=1024 --buffer-size=8192 /usr/share/y2linux/audio/headphone-44100.wav
        amixer -c Y2Audio sset Headphone off
        sleep 3
    done;;
dvfs)
    thermal_guard
    [ "$(cat "$policy/affected_cpus")" = '0 1 2 3' ] || exit 1
    pids=''
    cleanup() { for pid in $pids; do kill "$pid" 2>/dev/null || true; done; echo powersave > "$policy/scaling_governor"; }
    trap cleanup EXIT
    trap 'exit 130' HUP INT TERM
    echo userspace > "$policy/scaling_governor"
    for f in 598000 747500 1040000 598000; do
        thermal_guard
        echo "$f" > "$policy/scaling_setspeed"
        [ "$(cat "$policy/scaling_cur_freq")" -eq "$f" ] || exit 1
        show "$policy/scaling_cur_freq" /sys/kernel/debug/regulator/regulator_summary
        pids=''
        for cpu in 0 1 2 3; do
            timeout 5 sh -c 'while :; do :; done' &
            pids="$pids $!"
        done
        for second in 1 2 3 4 5; do
            thermal_guard
            show /sys/class/thermal/thermal_zone*/temp
            sleep 1
        done
        for pid in $pids; do
            status=0; wait "$pid" || status=$?
            case "$status" in 0|124|143) :;; *) exit "$status";; esac
        done
        pids=''
    done
    dmesg | tail -100;;
idle) idle; sleep 5; idle;;
display)
    set -- /sys/class/backlight/*/brightness
    [ "$#" -eq 1 ] && [ -f "$1" ] || exit 1
    backlight=$1; brightness=$(cat "$backlight")
    restore_display() { echo 0 > /sys/class/graphics/fb0/blank; echo "$brightness" > "$backlight"; }
    trap restore_display EXIT
    trap 'exit 130' HUP INT TERM
    echo 0 > "$backlight"; sleep 2
    echo "$brightness" > "$backlight"; sleep 2
    echo 4 > /sys/class/graphics/fb0/blank; sleep 3
    restore_display;;
rtc)
    show /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/wakealarm
    hwclock -r -u;;
suspend|rtc-alarm)
    [ -f "$record/boot-before" ] || exit 1
    cmp "$record/boot-before" /proc/sys/kernel/random/boot_id
    if [ "$1" = rtc-alarm ]; then
        hwclock -r -u
        [ -z "$(cat /sys/class/rtc/rtc0/wakealarm)" ] || { echo 'Existing RTC alarm retained' >&2; exit 1; }
        rtcwake -m no -s 15
    fi
    echo s2idle > /sys/power/mem_sleep
    sync
    count=$(cat /sys/power/wakeup_count)
    echo "$count" > /sys/power/wakeup_count
    echo mem > /sys/power/state
    cmp "$record/boot-before" /proc/sys/kernel/random/boot_id
    (cd "$record"; sha256sum -c sentinel.sha256)
    show /sys/power/pm_wakeup_irq /sys/kernel/debug/wakeup_sources /proc/uptime
    dmesg | tail -120;;
post-resume)
    cmp "$record/boot-before" /proc/sys/kernel/random/boot_id
    (cd "$record"; sha256sum -c sentinel.sha256)
    inventory > "$record/resume.log" 2>&1
    cat "$record/resume.log";;
cable-reboot)
    # Launch this phase with nohup, then unplug/replug once. It reboots in 40 s.
    for second in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
        show /proc/uptime /sys/class/power_supply/*/uevent
        sleep 2
    done
    sync; reboot;;
reboot) sync; reboot;;
post-reboot)
    if cmp -s "$record/boot-before" /proc/sys/kernel/random/boot_id; then echo 'No reboot observed' >&2; exit 1; fi
    (cd "$record"; sha256sum -c sentinel.sha256)
    inventory > "$record/reboot.log" 2>&1
    cat "$record/reboot.log";;
poweroff)
    # Launch with nohup, unplug USB during the delay, observe actual hardware off.
    echo 'Orderly poweroff in 15 seconds; disconnect USB now.'
    sleep 15; sync; poweroff;;
*) echo 'inventory | begin | audio | dvfs | idle | display | rtc | suspend | rtc-alarm | post-resume | cable-reboot | reboot | post-reboot | poweroff' >&2; exit 2;;
esac
