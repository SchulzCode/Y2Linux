#!/bin/sh
# Production cross build with the Buildroot SDK (as Y2Reborn's tools/build/cross.sh). No network.
set -eu
repo=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
build=${Y2_BUILDROOT_OUTPUT:?Set Y2_BUILDROOT_OUTPUT to the production Buildroot output}
build=$(CDPATH= cd -- "$build" && pwd)
sdk=${Y2_BUILDROOT_HOST:-$build/host}
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=$sdk/bin/arm-linux-gcc
export CC_armv7_unknown_linux_gnueabihf=$sdk/bin/arm-linux-gcc
export AR_armv7_unknown_linux_gnueabihf=$sdk/bin/arm-linux-ar
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS='-C target-cpu=cortex-a7 -C link-arg=-Wl,-z,relro,-z,now'
cd "$repo"
exec cargo build --workspace --release --locked --offline --target armv7-unknown-linux-gnueabihf
