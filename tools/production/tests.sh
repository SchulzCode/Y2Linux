#!/bin/sh
set -eu
cd /project
export Y2_ARTIFACT_TEST_ROOT=/build Y2_PRODUCTION_TEST=1 PYTHONPATH=/project/tests
# Current production profile and retained source-boundary regression modules.
# The test profile inventory and host prerequisites are documented in tests/README.md.
# The locked minimal host has libc headers but no distribution Linux UAPI.
# Export sanitised host UAPI from our already pinned kernel for native UI tests.
make -C /src O=/build/host-headers ARCH=x86 headers > /build/test-headers.log 2>&1
export C_INCLUDE_PATH=/build/host-headers/usr/include
python3 -m unittest tests.test_production_storage tests.test_production_handover tests.test_readback_profiles tests.test_owner_data tests.test_reborn_receipts tests.test_mmc_requests tests.test_mmc_context tests.test_emmc_layout tests.test_mmc_transport tests.test_stock_msdos tests.test_usb_recover tests.test_usb_wake tests.test_usb_session tests.test_usb_state tests.test_usb_clock tests.test_usb_reconnect tests.test_usb_failure tests.test_audio tests.test_dev_abi tests.test_dev_foundation tests.test_dev_i2c tests.test_baseline_pwrap tests.test_overlays tests.test_d08 tests.test_bootimg -v
# Power, radio and GPU subsystem/profile contracts remain separately identifiable.
PYTHONPATH=/project/tests python3 -m unittest tests.test_power tests.test_charger tests.test_power_policy tests.test_offline_charging tests.test_power03 tests.test_connectivity tests.test_suspend_policy tests.test_gpu -v
for script in /project/initramfs/production/init /project/initramfs/production/storage.sh /project/buildroot/board/y2/production-overlay/etc/init.d/* /project/buildroot/board/y2/production-overlay/usr/sbin/*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
for script in /project/buildroot/board/y2/production-overlay/usr/libexec/y2/*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n /project/tools/development/qualify-m4-power.sh
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/y2-abi-check
# A BOOTIMG-only build stages rescue binaries; ALSA utilities remain in the
# unchanged verified ext4 image rather than this reduced staging directory.
if [ ! -f /build/userspace-source.json ]; then
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/aplay --version
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/amixer --version
fi
