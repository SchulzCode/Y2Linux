#!/bin/sh
set -eu
target=$1
project=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
# Re-running Buildroot may retain these exact production-owned symlinks.
for pair in 'etc/dropbear:/data/ssh/host-keys' 'root/.ssh:/data/ssh/authorized_keys.d'; do
    name=${pair%%:*}; expected=${pair#*:}
    if [ -L "$target/$name" ] && [ "$(readlink "$target/$name")" = "$expected" ]; then
        unlink "$target/$name"
        mkdir -p "$target/$name"
    fi
done
# Use canonical base package/service/module installation, with no developer key.
unset Y2_PUBLIC_KEY_FILE
sh "$project/buildroot/board/y2/post-build.sh" "$target"
# Fresh output required: never sanitize or reuse a live system tree.
[ ! -e "$target/root/.ssh/authorized_keys" ]
rmdir "$target/etc/dropbear" "$target/root/.ssh"
ln -s /data/ssh/host-keys "$target/etc/dropbear"
ln -s /data/ssh/authorized_keys.d "$target/root/.ssh"
mkdir -p "$target/data" "$target/media/sd" "$target/etc/y2linux"
printf '%s\n' 1 > "$target/etc/y2linux/layout-version"
if [ -x "$target/usr/libexec/reborn-splash" ]; then
    printf '%s\n' Y2LINUX-REBORN-SPLASH-01 > "$target/etc/y2linux/build-id"
    sed -i 's@::sysinit:/etc/init.d/rcS@::sysinit:/usr/libexec/reborn-boot-services@' "$target/etc/inittab"
elif [ -x "$target/usr/bin/reborn" ]; then
    printf '%s\n' Y2LINUX-REBORN-BASELINE-01 > "$target/etc/y2linux/build-id"
else
    printf '%s\n' Y2LINUX-GPU-02 > "$target/etc/y2linux/build-id"
fi
cp "$Y2_ARTIFACT_DIR/versions.json" "$target/etc/y2linux/versions.json"
install -m 644 "$project/initramfs/production/storage.sh" "$target/usr/lib/y2-storage.sh"
# Never package host private keys, device calibration, or a universal login key.
install -m 755 "$Y2_ARTIFACT_DIR/y2-platform-start" "$target/usr/sbin/y2-platform-start"
install -m 755 "$project/tools/production/y2-status" "$target/usr/sbin/y2-status"
install -m 755 "$Y2_ARTIFACT_DIR/y2-usb-status" "$target/usr/sbin/y2-usb-status"
printf '%s\n' y2-platform-v1 > "$target/etc/y2linux/platform-contract"
# Explicit owner provisioning is a build input, never a boot-time download.
[ -n "${Y2_OWNER_FIRMWARE:-}" ]
PYTHONPATH="$project" python3 - "$Y2_OWNER_FIRMWARE" "$target" <<'PY'
import sys
from pathlib import Path
from tools.connectivity.provision import install_provision
install_provision(Path(sys.argv[1]), Path(sys.argv[2]))
PY
# D-Bus identity and all radio state are generated/persisted on Y2DATA.
[ ! -f "$target/var/lib/dbus/machine-id" ]
[ ! -f "$target/etc/machine-id" ]
