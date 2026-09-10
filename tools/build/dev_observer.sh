#!/bin/sh
set -eu
clang --target=arm-linux-gnueabi -mcpu=cortex-a7 -marm -mfloat-abi=soft \
    -Os -fomit-frame-pointer -ffreestanding -fno-builtin -fno-stack-protector \
    -fno-unwind-tables -fno-asynchronous-unwind-tables -fno-pie -nostdlib -static \
    -fuse-ld=lld -Wl,--build-id=none,-e,_start,--gc-sections,-z,max-page-size=4096 \
    -Wall -Wextra -Werror /project/initramfs/start.S /project/initramfs/init.c -o /build/y2-observer
qemu-arm -cpu cortex-a7 /build/y2-observer --selftest
