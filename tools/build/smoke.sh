#!/bin/sh
set -eu
clang --version
ld.lld --version
llvm-nm --version
make --version
python3 --version
printf 'int entry(void) { return 618; }\n' > smoke.c
clang --target=arm-linux-gnueabi -mcpu=cortex-a7 -marm -mfloat-abi=soft -ffreestanding -c smoke.c -o smoke.o
llvm-readelf -h -A smoke.o
