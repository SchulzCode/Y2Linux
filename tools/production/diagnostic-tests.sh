#!/bin/sh
set -eu
cd /project
python3 -m unittest tests.test_rescue_diagnostic tests.test_bootimg tests.test_d08 -v
python3 -c 'from pathlib import Path; from tests.test_rescue_diagnostic import fixture; Path("/build/usb-valid.bin").write_bytes(fixture()); Path("/build/usb-short.bin").write_bytes(fixture()[:40])'
base=/project/out/y2linux-production-build-v1-r2/buildroot/target
for script in initramfs/production/diagnostic-init initramfs/production/diagnostic-pages.sh; do
    qemu-arm -cpu cortex-a7 -L "$base" "$base/bin/busybox" sh -n "/project/$script"
done
qemu-arm -cpu cortex-a7 -L "$base" /build/y2-usb-status /build/usb-valid.bin
if qemu-arm -cpu cortex-a7 -L "$base" /build/y2-usb-status /build/usb-short.bin; then
    echo 'ERROR: ARM helper accepted short diagnostic record'; exit 1
else
    test "$?" -eq 2
fi
# Verify that the exact ARM filesystem probe still accepts the two image identities.
test "$(qemu-arm -cpu cortex-a7 -L "$base" "$base/sbin/blkid" -p -s UUID -o value /project/out/y2linux-production-v1-r2/Y2ROOT.img)" = 79324c69-6e75-4801-8000-000000000101
test "$(qemu-arm -cpu cortex-a7 -L "$base" "$base/sbin/blkid" -p -s UUID -o value /project/out/y2linux-production-v1-r2/Y2DATA.img)" = 79324c69-6e75-4801-8000-000000000102
echo 'PASS ARM cached-status decoder, rejection, shell syntax and real blkid identity'
