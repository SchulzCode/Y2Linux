#!/bin/sh
set -eu
# Caller supplies a checked actual gzip length through /build/initrd-size.h.
test -s /build/initrd-size.h
make -C /src O=/build/kernel scripts_dtc
clang -E -nostdinc -undef -D__DTS__ -x assembler-with-cpp \
    -I /src/include -I /build /project/kernel/dts/innioasis-y2-first-boot.dts \
    -o /build/y2.preprocessed.dts
/build/kernel/scripts/dtc/dtc -I dts -O dtb -o /build/y2.dtb /build/y2.preprocessed.dts
