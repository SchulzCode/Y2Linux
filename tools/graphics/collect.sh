#!/bin/sh
# SPDX-License-Identifier: MIT
# Read-only bounded telemetry. This script never changes power/radio policy.
set -eu
seconds=${1:-1}
case "$seconds" in ''|*[!0-9]*) exit 2;; esac
[ "$seconds" -ge 1 ] && [ "$seconds" -le 300 ] || exit 2
uname -r
cat /proc/sys/kernel/random/boot_id
index=0
while [ "$index" -lt "$seconds" ]; do
    printf 'SAMPLE %s ' "$index"
    cat /proc/uptime
    for file in /sys/devices/platform/13010000.gpu/power/runtime_status \
        /sys/devices/platform/13010000.gpu/power/runtime_active_time \
        /sys/devices/platform/13010000.gpu/power/runtime_suspended_time \
        /sys/devices/platform/10006000.power-controller/state \
        /sys/class/power_supply/BAT0/charging_state \
        /sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq \
        /sys/class/thermal/thermal_zone*/type /sys/class/thermal/thermal_zone*/temp \
        /sys/class/power_supply/*/status /sys/class/power_supply/*/health \
        /sys/class/power_supply/*/voltage_now /sys/class/power_supply/*/current_max \
        /sys/class/backlight/*/actual_brightness; do
        [ -r "$file" ] || continue
        printf '%s=' "$file"
        cat "$file"
    done
    sed -n '/^MemTotal:/p; /^MemAvailable:/p; /^Slab:/p' /proc/meminfo
    if [ -r /sys/kernel/debug/clk/clk_summary ]; then
        sed -n '/y2-mfg/p; /y2-mmpll/p' /sys/kernel/debug/clk/clk_summary
    fi
    index=$((index+1))
    [ "$index" -ge "$seconds" ] || sleep 1
done
for file in /sys/kernel/debug/pm_genpd/pm_genpd_summary /sys/kernel/debug/dri/*/state; do
    [ -r "$file" ] || continue
    printf '%s\n' "$file"
    cat "$file"
done
if [ -r /sys/kernel/debug/clk/clk_summary ]; then
    sed -n '/y2-mfg/p; /y2-mmpll/p; /mm_smi_common/p' /sys/kernel/debug/clk/clk_summary
fi
sed -n '/lima/p; /gpmmu/p; /ppmmu/p; / mt6582/p' /proc/interrupts
dmesg | tail -100
