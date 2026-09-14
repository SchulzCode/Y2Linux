#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# Explicit qualification phases through normal production interfaces.
# This helper neither controls the watchdog nor writes raw hardware/storage.
set -eu
[ "$(uname -r)" = 6.18.0-y2linux-m4-power-02 ] || { echo 'M4-POWER-02 required' >&2; exit 1; }
awk '$2=="/" && $3=="ext4" && $1=="/dev/mmcblk0p5" {ok=1} END {exit !ok}' /proc/mounts
awk '$2=="/data" && $3=="ext4" && $1=="/dev/mmcblk0p7" {ok=1} END {exit !ok}' /proc/mounts
record=/data/y2linux-m4-power-02
policy=/sys/devices/system/cpu/cpufreq/policy0
mkdir -p "$record"
show() { for f in "$@"; do [ -f "$f" ] || continue; printf '\n%s\n' "$f"; cat "$f" || true; done; }
check_files() { sha256sum -c "$record/sentinels.sha256"; }
check_session() { cmp "$record/boot-before" /proc/sys/kernel/random/boot_id; check_files; }
inventory() {
    uname -a; date -u
    show /proc/sys/kernel/random/boot_id /proc/uptime /proc/cmdline /sys/firmware/y2_boot/metadata /sys/firmware/y2_boot/normal_boot
    show /sys/devices/system/cpu/online /proc/meminfo /proc/mounts /proc/partitions /proc/asound/cards /proc/bus/input/devices
    show /sys/class/power_supply/*/uevent /sys/class/power_supply/BAT0/charging_state
    show /sys/class/thermal/thermal_zone*/type /sys/class/thermal/thermal_zone*/temp /sys/bus/platform/drivers/mt6582-thermal/*/acquisition
    show "$policy"/scaling_available_frequencies "$policy"/scaling_cur_freq "$policy"/scaling_governor "$policy"/stats/time_in_state
    show /sys/devices/system/cpu/cpu*/cpuidle/state*/name /sys/devices/system/cpu/cpu*/cpuidle/state*/usage /sys/devices/system/cpu/cpu*/cpuidle/state*/time
    show /sys/power/state /sys/power/mem_sleep /sys/power/pm_wakeup_irq /sys/bus/platform/drivers/y2-spm/*/state
    show /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/wakealarm
    show /sys/class/drm/card*-*/status /sys/class/backlight/*/brightness /sys/class/backlight/*/actual_brightness
    show /sys/kernel/debug/regulator/regulator_summary /sys/kernel/debug/clk/clk_summary /sys/kernel/debug/wakeup_sources /proc/interrupts
    dmesg
}
thermal_guard() {
    found=0
    for z in /sys/class/thermal/thermal_zone*; do
        [ -f "$z/type" ] || continue
        case "$(cat "$z/type")" in cpu-thermal|pmic-thermal)
            t=$(cat "$z/temp") || return 1
            # Conservative test stops, not invented hardware protection limits.
            # Implausible cold readings must be resolved before load testing.
            [ "$t" -ge 10000 ] && [ "$t" -lt 60000 ] || { echo "Temperature test stop: $z $t" >&2; return 1; }
            found=$((found+1));;
        esac
    done
    [ "$found" -eq 2 ] || return 1
    [ "$(cat /sys/class/power_supply/BAT0/voltage_now)" -ge 3500000 ] || { echo 'Charge further before load/suspend tests' >&2; return 1; }
}
case "${1:-inventory}" in
inventory) inventory;;
begin)
    [ ! -e "$record/boot-before" ] && [ ! -e /root/y2-m4-power-sentinel ] || { echo 'Prior qualification retained; use inventory' >&2; exit 1; }
    cat /proc/sys/kernel/random/boot_id > "$record/boot-before"
    dd if=/dev/urandom of="$record/data-sentinel" bs=4096 count=64
    dd if=/dev/urandom of=/root/y2-m4-power-sentinel bs=4096 count=64
    sha256sum "$record/data-sentinel" /root/y2-m4-power-sentinel > "$record/sentinels.sha256"
    sync; inventory > "$record/begin.log" 2>&1; cat "$record/begin.log";;
audio)
    amixer -c Y2Audio sset Headphone off
    trap 'amixer -c Y2Audio sset Headphone off' EXIT
    trap 'exit 130' HUP INT TERM
    amixer -c Y2Audio cset name='PCM Ch Select' 0
    amixer -c Y2Audio sset Master 207,207
    amixer -c Y2Audio sset Headphone on
    timeout 6 aplay -D hw:CARD=Y2Audio,DEV=0 --period-size=1024 --buffer-size=8192 /usr/share/y2linux/audio/headphone-44100.wav
    ;;
frequency)
    thermal_guard
    [ "$(cat /sys/devices/system/cpu/online)" = 0-3 ] || exit 1
    previous_governor=$(cat "$policy/scaling_governor");pids=''
    cleanup() { for pid in $pids; do kill "$pid" 2>/dev/null || true; done; echo "$previous_governor" > "$policy/scaling_governor"; }
    trap cleanup EXIT
    trap 'exit 130' HUP INT TERM
    echo userspace > "$policy/scaling_governor"
    for round in 1 2 3; do
        for frequency in 598000 747500 1040000; do
            thermal_guard
            echo "$frequency" > "$policy/scaling_setspeed"
            [ "$(cat "$policy/scaling_cur_freq")" -eq "$frequency" ] || exit 1
            show "$policy/scaling_cur_freq" /sys/kernel/debug/regulator/regulator_summary
            pids=''
            for cpu in 0 1 2 3; do timeout 3 sh -c 'while :; do :; done' & pids="$pids $!"; done
            for second in 1 2 3; do thermal_guard; show /sys/class/thermal/thermal_zone*/temp; sleep 1; done
            for pid in $pids; do status=0; wait "$pid" || status=$?; case "$status" in 0|124|143) :;; *) exit "$status";; esac; done
            pids=''
        done
    done
    dmesg | tail -100;;
s2idle|deep-power|deep-rtc)
    thermal_guard; check_session
    [ "$(cat /sys/class/power_supply/BAT0/status)" != Charging ] || { echo 'Active charger must continue watchdog service; no suspend test' >&2; exit 1; }
    mode=deep
    [ "$1" != s2idle ] || mode=s2idle
    if [ "$1" = deep-rtc ]; then
        [ -z "$(cat /sys/class/rtc/rtc0/wakealarm)" ] || { echo 'Existing alarm retained' >&2; exit 1; }
        rtcwake -m no -s 20
    fi
    echo "$mode" > /sys/power/mem_sleep
    show /proc/uptime /sys/bus/platform/drivers/y2-spm/*/state
    sync
    count=$(cat /sys/power/wakeup_count); echo "$count" > /sys/power/wakeup_count
    echo mem > /sys/power/state
    check_session
    inventory > "$record/resume-$1.log" 2>&1; cat "$record/resume-$1.log";;
post-resume) check_session; inventory;;
reboot)
    check_files
    cat /proc/sys/kernel/random/boot_id > "$record/transition-before"
    hwclock -r -u > "$record/rtc-before-reboot"
    sync; reboot;;
post-reboot)
    ! cmp -s "$record/transition-before" /proc/sys/kernel/random/boot_id || exit 1
    check_files; hwclock -r -u; inventory;;
poweroff-connected)
    check_files; cat /proc/sys/kernel/random/boot_id > "$record/transition-before"
    hwclock -r -u > "$record/rtc-before-poweroff"; sync; poweroff;;
poweroff-unplugged)
    check_files; cat /proc/sys/kernel/random/boot_id > "$record/transition-before"
    echo 'Poweroff in 15 seconds; unplug USB during this delay.'
    sleep 15
    [ "$(cat /sys/class/power_supply/y2-usb-presence/online)" -eq 0 ] || { echo 'Cable still present; retaining current session' >&2; exit 1; }
    hwclock -r -u > "$record/rtc-before-poweroff"; sync; poweroff;;
*) echo 'inventory | begin | audio | frequency | s2idle | deep-power | deep-rtc | post-resume | reboot | post-reboot | poweroff-connected | poweroff-unplugged' >&2; exit 2;;
esac
