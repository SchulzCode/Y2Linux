#!/bin/sh
set -eu
cd /project
# New AFE/rail paths and preserved memory/packaging; no M2 hardware repetition.
Y2_ARTIFACT_TEST_ROOT=/build python3 -m unittest discover -s tests -p 'test_audio*.py' -v
python3 -m unittest discover -s tests -p test_baseline_pwrap.py -v
python3 -m unittest tests.test_baseline.SharedPolicy.test_clock_decode_and_write_firewalls -v
for pattern in test_dev_foundation.py test_dev_i2c.py test_dev_abi.py test_overlays.py; do
    Y2_ARTIFACT_TEST_ROOT=/build python3 -m unittest discover -s tests -p "$pattern" -v
done
for script in /project/initramfs/rescue/init /project/buildroot/board/y2/overlay/etc/init.d/S*y2* /project/buildroot/board/y2/overlay/etc/init.d/S50dropbear /project/buildroot/board/y2/overlay/usr/sbin/y2-*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/y2-abi-check
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/aplay --version
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/amixer --version
