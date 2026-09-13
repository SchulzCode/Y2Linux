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
printf '%s\n' Y2LINUX-STORAGE-03 > "$target/etc/y2linux/build-id"
cp "$Y2_ARTIFACT_DIR/versions.json" "$target/etc/y2linux/versions.json"
install -m 644 "$project/initramfs/production/storage.sh" "$target/usr/lib/y2-storage.sh"
# Never package host private keys, device calibration, or a universal login key.
