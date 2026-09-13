# SPDX-License-Identifier: GPL-2.0-only
# All storage probes are read-only and restricted to the approved internal spans.
y2_diag_page() {
    page=$1
    echo "Y2 STORAGE DIAG01  PAGE $page/3  READ ONLY"
    echo 'No fsck / no block mounts / no root handover'
    printf 'Uptime: '; cat /proc/uptime
    case "$page" in
    1)
        /sbin/y2-usb-status
        printf 'Internal driver: '
        driver=$(readlink /sys/bus/platform/devices/11230000.mmc/driver)
        echo "${driver:-NOT BOUND}"
        seen=0
        for entry in /sys/class/block/*; do
            [ -e "$entry" ] || continue
            path=$(readlink -f "$entry")
            case "$path" in */11230000.mmc/*) ;; *) continue;; esac
            [ ! -f "$entry/partition" ] || continue
            # Exclude hardware boot areas: no partition switching or probing.
            case "${entry##*/}" in *boot*|*rpmb*) continue;; esac
            seen=1
            echo "Disk: ${entry##*/}"
            printf 'type='; cat "$entry/device/type"
            printf 'sectors='; cat "$entry/size"
            printf 'removable='; cat "$entry/removable"
        done
        [ "$seen" = 1 ] || echo 'NO INTERNAL USER BLOCK DEVICE'
        echo 'USB: boot unplugged, attach once after 10s'
        ;;
    2)
        echo 'Internal partitions: start / sector count'
        for entry in /sys/class/block/*; do
            [ -f "$entry/partition" ] || continue
            path=$(readlink -f "$entry")
            case "$path" in */11230000.mmc/*) ;; *) continue;; esac
            start=$(cat "$entry/start"); size=$(cat "$entry/size")
            echo "${entry##*/}: $start / $size"
            case "$start:$size" in
                166912:1679360|2104320:1638400)
                    /bin/busybox timeout -s KILL 3 /sbin/blkid -p -o export "/dev/${entry##*/}" |
                        /bin/busybox grep -E '^(TYPE|LABEL|UUID)='
                    ;;
            esac
        done
        # Bound discovery probes independently; a hung read cannot hide USB page.
        for name in Y2ROOT Y2DATA; do
            if [ "$name" = Y2ROOT ]; then
                args='79324c69-6e75-4801-8000-000000000101 166912 1679360'
            else
                args='79324c69-6e75-4801-8000-000000000102 2104320 1638400'
            fi
            result=$(/bin/busybox timeout -s KILL 4 /bin/busybox sh -c \
                '. /sbin/y2-storage; y2_find_partition "$@"' sh "$name" $args)
            echo "$name resolver: ${result:-REJECTED OR ABSENT}"
        done
        ;;
    3)
        echo 'Internal MMC / USB log tail (also retained in RAM)'
        { cat /run/early-kmsg.txt; /bin/busybox dmesg; } |
            /bin/busybox grep -E '11230000|Y2 storage guard|Y2USB|non-removable|mmc[0-9]:.*(error|failed)' |
            /bin/busybox tail -n 15
        ;;
    esac
}
