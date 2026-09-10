#!/bin/sh
set -eu
cd /project
for pattern in 'test_dev_*.py' test_relay.py test_d08.py test_bootimg.py 'test_usb_*.py' test_overlays.py; do
    Y2_ARTIFACT_TEST_ROOT=/build python3 -m unittest discover -s tests -p "$pattern" -v
done
# The old DT mutation fixture is specific to D08; DEV DT/memory has its own checks.
# Shared clock, PMIC, display-loader, PHY and evdev production callbacks still apply.
python3 -m unittest discover -s tests -p 'test_baseline*.py' -v
python3 -m unittest discover -s tests/observation -p test_usb_capture.py -v
for script in /project/initramfs/rescue/init /project/buildroot/board/y2/overlay/etc/init.d/S*y2* /project/buildroot/board/y2/overlay/etc/init.d/S50dropbear /project/buildroot/board/y2/overlay/usr/sbin/y2-*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox --list > /build/busybox-applets.txt
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/sbin/dropbear -h > /build/dropbear-help.txt 2>&1
