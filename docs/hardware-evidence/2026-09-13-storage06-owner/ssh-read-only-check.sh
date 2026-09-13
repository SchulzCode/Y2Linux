#!/bin/sh
# Read-only acceptance after the owner's manual USRDATA initialization.
# Host-key files are inspected as metadata only; no private key content is read.
set -eu
printf 'KERNEL\n'
uname -r
printf 'UPTIME\n'
cat /proc/uptime
printf 'MOUNTS\n'
cat /proc/mounts
printf 'PARTITIONS\n'
cat /proc/partitions
printf 'MMC_GEOMETRY\n'
for name in mmcblk0 mmcblk0p5 mmcblk0p7; do
    printf '%s ' "$name"
    [ ! -r "/sys/class/block/$name/start" ] || cat "/sys/class/block/$name/start"
    cat "/sys/class/block/$name/size"
    readlink -f "/sys/class/block/$name/device" || true
done
printf 'MMC_TYPE\n'
cat /sys/class/block/mmcblk0/device/type
printf 'TABLE_HASHES_512_BYTES\n'
for lba in 0 1024 145408; do
    printf 'logical_sector=%s\n' "$lba"
    dd if=/dev/mmcblk0 bs=512 skip="$lba" count=1 2>/dev/null | sha256sum
done
printf 'BOOTIMG_HASH_5193728_BYTES\n'
dd if=/dev/mmcblk0 bs=512 skip=60416 count=10144 2>/dev/null | sha256sum
printf 'FILESYSTEM_IDENTITIES\n'
blkid -p -o export /dev/mmcblk0p5 /dev/mmcblk0p7
printf 'OWNER_AUTHORIZATION_HASH\n'
sha256sum /data/ssh/authorized_keys.d/authorized_keys
printf 'SSH_PATHS_AND_HOST_KEY_METADATA_ONLY\n'
ls -ld /root/.ssh /etc/dropbear /data/ssh /data/ssh/authorized_keys.d /data/ssh/authorized_keys.d/authorized_keys /data/ssh/host-keys
ls -l /data/ssh/host-keys
printf 'DATA_SCHEMA\n'
cat /data/.y2data-schema
printf 'CPUS_RAM_NETWORK\n'
cat /sys/devices/system/cpu/online
head -5 /proc/meminfo
ip -4 addr show dev usb0
printf 'KERNEL_LOG\n'
dmesg
printf 'READ_ONLY_ACCEPTANCE_COMPLETE\n'
