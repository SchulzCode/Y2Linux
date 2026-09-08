#!/bin/sh
set -eu
make -C /src O=/build/kernel KCONFIG_ALLCONFIG=/project/kernel/config/first-boot.config allnoconfig
python3 /project/tools/build/check_config.py /build/kernel/.config /project/kernel/config/first-boot.config
