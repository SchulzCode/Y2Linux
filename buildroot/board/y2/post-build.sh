#!/bin/sh
set -eu
target=$1
# Runtime state belongs in tmpfs; host keys persist on removable Y2ROOT.
if [ -L "$target/etc/dropbear" ]; then
    [ "$(readlink "$target/etc/dropbear")" = /var/run/dropbear ] || exit 1
    unlink "$target/etc/dropbear"
fi
mkdir -p "$target/root/.ssh" "$target/etc/dropbear" "$target/etc/y2linux"
chmod 700 "$target/root/.ssh" "$target/etc/dropbear"
if [ -n "${Y2_PUBLIC_KEY_FILE:-}" ]; then
    case "$Y2_PUBLIC_KEY_FILE" in *.pub|*/authorized_keys) ;; *) echo 'Public-key path required' >&2; exit 1;; esac
    awk 'NF && $1 !~ /^#/ && $1 !~ /^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(256|384|521))$/ {exit 1}' "$Y2_PUBLIC_KEY_FILE"
    install -m 600 "$Y2_PUBLIC_KEY_FILE" "$target/root/.ssh/authorized_keys"
fi
printf '%s\n' 'Y2LINUX-DEV-01' > "$target/etc/y2linux/build-id"

# An impossible SHA512-crypt value keeps password login unusable without locking
# the account (Dropbear must still accept the authorized public key).
awk -F: 'BEGIN {OFS=":"} $1=="root" {$2="$6$y2linux$disabled"} {print}' "$target/etc/shadow" > "$target/etc/shadow.y2"
mv "$target/etc/shadow.y2" "$target/etc/shadow"
chmod 600 "$target/etc/shadow"

project=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
artifacts=${Y2_ARTIFACT_DIR:-$project/out/y2linux-dev-01}
install -m 755 "$artifacts/y2-observer" "$target/usr/sbin/y2-observer"
install -m 755 "$artifacts/y2-fbtest" "$target/usr/sbin/y2-fbtest"
install -m 400 "$artifacts/kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko" "$target/display.ko"
release=$(cat "$artifacts/kernel/include/config/kernel.release")
mkdir -p "$target/lib/modules/$release/kernel/drivers/gpu/drm/mediatek"
install -m 644 "$target/display.ko" "$target/lib/modules/$release/kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko"

# Match the one packaged module to its exact kernel and generate modprobe indexes.
cp "$artifacts/kernel/modules.builtin" "$target/lib/modules/$release/modules.builtin"
cp "$artifacts/kernel/modules.builtin.modinfo" "$target/lib/modules/$release/modules.builtin.modinfo"
"${HOST_DIR}/sbin/depmod" -b "$target" "$release"
