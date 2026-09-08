#!/bin/sh
set -eu
# Run only through tools/build/run.py. Reject reuse of an existing product tree.
test ! -e /build/BOOTIMG.img
test ! -e /build/kernel/vmlinux
sh /project/tools/build/configure.sh
sh /project/tools/build/initramfs.sh
make -C /src O=/build/kernel -j12 Image zImage > /build/kernel-build.log 2>&1
sh /project/tools/build/dtb.sh
cd /project
python3 -m tools.validation.artifacts /build --append > /build/artifact-validation.log
python3 -m tools.build.package /build > /build/package-validation.log
Y2_ARTIFACT_TEST_ROOT=/build python3 -m unittest discover -s tests -v > /build/tests.log 2>&1
cp /build/kernel/arch/arm/boot/Image /build/Image
cp /build/kernel/arch/arm/boot/zImage /build/zImage
cp /build/kernel/.config /build/kernel.config
sha256sum /build/Image /build/zImage /build/y2.dtb /build/zImage-dtb /build/initramfs.cpio.gz /build/BOOTIMG.img
