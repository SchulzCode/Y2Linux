#!/bin/sh
# Attended qualification only. Standard power_supply policy, never raw PMIC IO.
# Run over owner SSH after manually installing M4-CHARGE-01. Always inhibits at exit.
set -eu
bat=/sys/class/power_supply/BAT0
input=/sys/class/power_supply/y2-usb-input
[ "$(uname -r)" = '6.18.0-y2linux-m4-charge-01' ] || exit 1
[ -w "$bat/charge_behaviour" ] || exit 1
inhibit() { printf '%s\n' inhibit-charge > "$bat/charge_behaviour"; }
trap inhibit EXIT
trap 'exit 1' HUP INT TERM PIPE
inhibit
[ "$(cat "$bat/present")" = 1 ]
[ "$(cat /sys/class/power_supply/y2-usb-presence/online)" = 1 ]
[ "$(cat "$input/current_max")" = 500000 ]
[ "$(cat "$bat/constant_charge_current")" = 70000 ]
[ "$(cat "$bat/constant_charge_voltage")" = 4175000 ]
voltage=$(cat "$bat/voltage_now")
[ "$voltage" -ge 3400000 ] && [ "$voltage" -lt 4175000 ]
# Existing kernel safety policy checks sensors/protections again before enabling.
# A latched fault is never cleared by this script.
printf '%s\n' 'BEGIN attended 120-second charge; automatically inhibits at exit'
cat "$bat/charging_state"
printf '%s\n' auto > "$bat/charge_behaviour"
sample=0
while [ "$sample" -lt 24 ]; do
    sleep 5
    printf 'SAMPLE %s uptime=' "$sample"
    cat /proc/uptime
    cat "$bat/uevent"
    state=$(cat "$bat/charging_state")
    printf '%s\n' "$state"
    for zone in /sys/class/thermal/thermal_zone*; do
        printf 'THERMAL '
        cat "$zone/type" "$zone/temp"
    done
    [ "$(cat "$bat/present")" = 1 ]
    [ "$(cat "$input/current_max")" = 500000 ]
    [ "$(cat "$bat/constant_charge_current")" = 70000 ]
    [ "$(cat "$bat/constant_charge_voltage")" = 4175000 ]
    voltage=$(cat "$bat/voltage_now")
    [ "$voltage" -ge 3400000 ] && [ "$voltage" -lt 4200000 ]
    case "$state" in *'fault=0x0 sample_error=0 stop_error=0 '*) :;; *) exit 1;; esac
    case "$state" in *'active=1 online=1 present=1 '*) :;; *) exit 1;; esac
    [ "$(cat "$bat/status")" = Charging ]
    sample=$((sample + 1))
done
printf '%s\n' 'END observation; charging inhibited for review'
