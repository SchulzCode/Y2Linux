#!/bin/sh
# Bounded, inhibited acquisition through the production IIO owner. No raw PMIC
# access, charge control, temperature/current conversion or persistent writes.
# Run over SSH using sh -s; stdout belongs to the host evidence capture.
set -eu

samples=${1:-30}
case "$samples" in ''|*[!0-9]*) echo 'Expected 1..60 samples' >&2; exit 2;; esac
[ "$samples" -ge 1 ] && [ "$samples" -le 60 ] || exit 2
[ "$#" -le 1 ] || exit 2

adc=
for candidate in /sys/bus/iio/devices/iio:device*; do
    [ -r "$candidate/name" ] || continue
    [ "$(cat "$candidate/name")" = mt6323-auxadc ] || continue
    [ -z "$adc" ] || { echo 'Ambiguous ADC owner' >&2; exit 1; }
    adc=$candidate
done
[ -n "$adc" ] || { echo 'MT6323 ADC unavailable' >&2; exit 1; }
for channel in 5 6 7; do
    [ -r "$adc/in_voltage${channel}_raw" ] || {
        echo "Channel $channel unavailable; production ADC update required" >&2
        exit 1
    }
    printf '# channel_%s_scale_mV_per_count=%s\n' "$channel" "$(cat "$adc/in_voltage${channel}_scale")"
done

bat=/sys/class/power_supply/BAT0
usb=/sys/class/power_supply/y2-usb-presence
printf '# kernel=%s\n# adc=%s\n' "$(uname -r)" "$adc"
printf 'sample\tuptime_start_s\tuptime_end_s\tbatsns_before_raw\tbaton_raw\tisense_raw\tbatsns_after_raw\tbattery_uV\tusb_online\tstatus\n'
i=0
while [ "$i" -lt "$samples" ]; do
    status=$(cat "$bat/status")
    case "$status" in
        'Not charging'|Discharging) ;;
        *) echo "Inhibited sampling required; status=$status" >&2; exit 1;;
    esac
    [ "$(cat "$bat/present")" = 1 ] || { echo 'Battery absent' >&2; exit 1; }
    battery_uv=$(cat "$bat/voltage_now")
    # Existing M4 observation cutoff, not a charging/pack protection threshold.
    [ "$battery_uv" -ge 3400000 ] || { echo 'Stop: battery below observation cutoff (3.4 V)' >&2; exit 1; }
    online=$(cat "$usb/online")
    read -r start_uptime unused < /proc/uptime
    before=$(cat "$adc/in_voltage7_raw")
    baton=$(cat "$adc/in_voltage5_raw")
    isense=$(cat "$adc/in_voltage6_raw")
    after=$(cat "$adc/in_voltage7_raw")
    read -r end_uptime unused < /proc/uptime
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$i" "$start_uptime" "$end_uptime" "$before" "$baton" "$isense" "$after" "$battery_uv" "$online" "$status"
    # Preserve the endpoint in the record, then stop. Never turn it into C/A.
    for raw in "$before" "$baton" "$isense" "$after"; do
        [ "$raw" -gt 0 ] && [ "$raw" -lt 32767 ] || {
            echo 'Stop: zero/saturated ADC result; channel not yet usable' >&2
            exit 1
        }
    done
    i=$((i + 1))
    [ "$i" -ge "$samples" ] || sleep 1
done
