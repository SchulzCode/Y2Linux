# SPDX-License-Identifier: GPL-2.0-only
# Read-only resolver shared with the installed system. Output a device only when
# host, media type, capacity, geometry and filesystem identity all agree.
y2_find_partition() {
    wanted_label=$1 wanted_uuid=$2 wanted_start=$3 wanted_size=$4
    found=
    for entry in /sys/class/block/*; do
        [ -f "$entry/partition" ] || continue
        path=$(readlink -f "$entry") || continue
        case "$path" in */11230000.mmc/*/block/*/*) ;; *) continue;; esac
        parent=${path%/*}
        [ "$(cat "$parent/device/type" 2>/dev/null)" = MMC ] || continue
        [ "$(cat "$parent/removable" 2>/dev/null)" = 0 ] || continue
        [ "$(cat "$parent/size" 2>/dev/null)" = 15203328 ] || continue
        [ "$(cat "$entry/start" 2>/dev/null)" = "$wanted_start" ] || continue
        [ "$(cat "$entry/size" 2>/dev/null)" = "$wanted_size" ] || continue
        dev=/dev/${entry##*/}
        [ "$(blkid -p -s TYPE -o value "$dev" 2>/dev/null)" = ext4 ] || continue
        [ "$(blkid -p -s LABEL -o value "$dev" 2>/dev/null)" = "$wanted_label" ] || continue
        [ "$(blkid -p -s UUID -o value "$dev" 2>/dev/null)" = "$wanted_uuid" ] || continue
        [ -z "$found" ] || return 1
        found=$dev
    done
    [ -n "$found" ] || return 1
    printf '%s\n' "$found"
}
