#!/bin/sh
# Read-only live inventory. Missing optional tools are recorded, not fatal.
set +e
run() { printf '\n===== COMMAND:'; printf ' %s' "$@"; printf ' =====\n'; "$@"; rc=$?; printf '\n===== EXIT %s =====\n' "$rc"; }
for cmd in 'uname -a' 'cat /proc/cpuinfo' 'cat /proc/meminfo' 'cat /proc/zoneinfo' 'cat /proc/interrupts' 'cat /proc/iomem' 'cat /proc/cmdline' 'cat /proc/sys/kernel/random/boot_id' 'uptime' 'free -m' 'ps' 'mount' 'df -h' 'lsblk' 'blkid' 'ip addr' 'ip link' 'route' 'ip route' 'dmesg'; do run sh -c "$cmd"; done
for dir in /sys/class/drm /sys/class/graphics /sys/class/input /sys/class/backlight /sys/class/power_supply /sys/class/rtc /sys/class/mmc_host /sys/bus/mmc/devices /sys/bus/i2c/devices /sys/devices/system/cpu /sys/class/udc /sys/class/tty/ttyGS0 /sys/class/block /dev/dri /dev/input /lib/modules /sys/kernel/debug/dri /sys/kernel/debug/regulator /sys/class/vtconsole; do run ls -la "$dir"; done
for f in /etc/os-release /etc/inittab /etc/fstab /etc/network/interfaces /etc/resolv.conf /etc/default/dropbear /proc/1/mountinfo /proc/bus/input/devices /proc/modules /proc/buddyinfo /proc/sys/kernel/tainted /proc/sys/kernel/random/entropy_avail /sys/devices/system/cpu/online /sys/devices/system/cpu/possible /sys/kernel/debug/devices_deferred /sys/kernel/debug/clk/clk_summary /sys/kernel/debug/regulator/regulator_summary /sys/kernel/debug/dri/*/state /sys/kernel/debug/dri/*/framebuffer /sys/kernel/debug/dri/*/clients /sys/kernel/debug/dri/*/gem_names /sys/kernel/debug/dri/*/vblank /sys/class/drm/card*-*/status /sys/class/drm/card*-*/modes /sys/class/drm/card*-*/enabled /sys/class/graphics/fb*/name /sys/class/graphics/fb*/bits_per_pixel /sys/class/graphics/fb*/stride /sys/class/graphics/fb*/virtual_size /sys/class/graphics/fb*/modes /sys/class/graphics/fb*/blank /sys/class/vtconsole/vtcon*/name /sys/class/vtconsole/vtcon*/bind /sys/class/backlight/*/brightness /sys/class/backlight/*/actual_brightness /sys/class/backlight/*/max_brightness /sys/class/backlight/*/bl_power /sys/bus/mmc/devices/*/type /sys/bus/mmc/devices/*/name /sys/bus/mmc/devices/*/cid /sys/bus/mmc/devices/*/csd /sys/bus/mmc/devices/*/uevent /sys/class/block/mmcblk*/ro /sys/class/block/mmcblk*/removable /sys/bus/i2c/devices/*/name /sys/class/udc/*/state; do [ -f "$f" ] && run cat "$f"; done
run readlink /proc/1/exe
run sh -c 'tr "\000" " " < /proc/1/cmdline; echo'
run sh -c 'for f in /etc/init.d/*; do echo "===== $f ====="; cat "$f"; done'
run sh -c 'ls -la /dev /run /var/log /etc/dropbear /root/.ssh; ls -l /dev/dri/* /dev/input/* /dev/ttyGS* /dev/fb* /dev/mmc*; for f in /run/*.pid; do echo "$f"; cat "$f"; done'
run sh -c 'for cmd in memtester timeout evtest modetest fbset i2cdetect i2cget i2ctransfer depmod modinfo logread dmesg sha256sum findmnt e2fsck; do command -v "$cmd"; done'
run logread
run sh -c 'for f in /var/log/messages*; do [ ! -f "$f" ] || cat "$f"; done'
run sh -c 'find /sys/kernel/debug/dri /sys/kernel/debug/regulator /sys/kernel/debug/clk -maxdepth 2 -type f'
run sh -c 'for d in /sys/firmware/devicetree/base/*mmc*; do echo "$d"; for f in status compatible; do [ ! -f "$d/$f" ] || tr "\000" "\n" < "$d/$f"; done; done'
