#!/bin/sh
# Explicit, bounded observation on production Y2DATA; no hardware-policy writes.
# Launch with nohup so USB removal does not terminate the recorder.
set -eu
seconds=${1:-300}
case "$seconds" in ''|*[!0-9]*) echo 'Seconds must be 1..600' >&2; exit 1;; esac
[ "$seconds" -ge 1 ] && [ "$seconds" -le 600 ]
[ "$(cat /data/.y2data-schema)" = 1 ]
awk '$2 == "/data" && $3 == "ext4" && $4 ~ /(^|,)rw(,|$)/ { ok=1 }
     END { exit !ok }' /proc/mounts
umask 077
mkdir -p /data/logs
dir=$(mktemp -d /data/logs/usb-charge.XXXXXX)
echo "$dir"
uname -a > "$dir/identity.txt"
cat /proc/uptime >> "$dir/identity.txt"
dmesg > "$dir/dmesg-start.txt"
read -r uptime rest < /proc/uptime
deadline=$((${uptime%.*} + seconds))
last_flush=0
last_usb=
finish() {
    status=$?
    dmesg > "$dir/dmesg-final.txt"
    echo "END exit=$status" >> "$dir/samples.txt"
    sync
}
trap finish EXIT
trap 'exit 1' HUP INT TERM
while :; do
    read -r uptime rest < /proc/uptime
    now=${uptime%.*}
    [ "$now" -lt "$deadline" ] || break
    usb=$(head -n 1 /sys/devices/platform/11200000.usb/status 2>&1 || true)
    usb_state=${usb%% polls=*}
    {
        echo "UPTIME $uptime"
        echo "$usb"
        /usr/sbin/y2-usb-status 2>&1 || true
        for udc in /sys/class/udc/*; do
            [ ! -r "$udc/state" ] || cat "$udc/state"
            for field in control runtime_status; do
                path=$udc/device/power/$field
                [ ! -r "$path" ] || { echo "$path"; cat "$path"; }
            done
        done
        cat /sys/class/power_supply/BAT0/charging_state 2>&1 || true
        cat /sys/class/power_supply/y2-usb-presence/uevent 2>&1 || true
    } >> "$dir/samples.txt"
    # Keep the previous complete dump if observation is interrupted by reboot.
    if [ "$now" -ge "$((last_flush + 5))" ] || [ "$usb_state" != "$last_usb" ]; then
        dmesg > "$dir/dmesg-next.txt"
        mv "$dir/dmesg-next.txt" "$dir/dmesg-latest.txt"
        sync
        last_flush=$now
    fi
    last_usb=$usb_state
    sleep 1
done
