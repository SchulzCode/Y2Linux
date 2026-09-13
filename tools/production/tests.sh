#!/bin/sh
set -eu
cd /project
export Y2_ARTIFACT_TEST_ROOT=/build Y2_PRODUCTION_TEST=1
python3 -m unittest tests.test_production_storage tests.test_audio tests.test_dev_abi tests.test_dev_i2c tests.test_baseline_pwrap tests.test_overlays tests.test_d08 tests.test_bootimg -v
for script in /project/initramfs/production/init /project/initramfs/production/storage.sh /project/buildroot/board/y2/production-overlay/etc/init.d/* /project/buildroot/board/y2/production-overlay/usr/sbin/*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/y2-abi-check
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/aplay --version
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/amixer --version
