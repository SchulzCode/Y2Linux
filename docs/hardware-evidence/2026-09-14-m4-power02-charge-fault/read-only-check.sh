#!/bin/sh
# Read-only inspection through existing production interfaces.
set -eu
for item in /proc/sys/kernel/random/boot_id /proc/uptime /sys/firmware/y2_boot/metadata /sys/firmware/y2_boot/normal_boot; do
    printf 'FILE %s\n' "$item"
    if [ -r "$item" ]; then cat "$item"; else printf 'UNAVAILABLE\n'; fi
done
printf 'CHARGER_REGISTER_PREFIX\n'
dd if=/sys/kernel/debug/regmap/1000d000.pwrap/registers bs=11 count=36 2>/dev/null
printf 'LOG_DIRECTORY_METADATA\n'
ls -l /data/logs /var/log /sys/fs/pstore /tmp || :
for n in 1 2 3 4 5 6; do
    printf 'SAMPLE %s UPTIME ' "$n"
    cat /proc/uptime
    cat /sys/class/power_supply/BAT0/uevent /sys/class/power_supply/BAT0/charging_state
    [ "$n" -eq 6 ] || sleep 5
done
printf 'END_READ_ONLY_INSPECTION\n'
