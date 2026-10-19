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
    printf '%s\n' "${Y2_BUILD_ID:?Explicit build identity required}" > "$target/etc/y2linux/build-id"
    sed -i 's@::sysinit:/etc/init.d/rcS@::sysinit:/usr/libexec/reborn-boot-services@' "$target/etc/inittab"
elif [ -x "$target/usr/bin/reborn" ]; then
    printf '%s\n' Y2LINUX-REBORN-BASELINE-01 > "$target/etc/y2linux/build-id"
else
    printf '%s\n' Y2LINUX-GPU-02 > "$target/etc/y2linux/build-id"
fi
cp "$Y2_ARTIFACT_DIR/versions.json" "$target/etc/y2linux/versions.json"
git -C "$project" show -s --format=%ct HEAD > "$target/etc/y2linux/build-epoch"
rm -f "$target/etc/resolv.conf"
ln -s /run/y2/resolv.conf "$target/etc/resolv.conf"
# Data is already identity-checked and mounted before switch_root. Generate
# per-device seeds there; never ship a seed shared by multiple devices.
mkdir -p "$target/etc/default"
printf '%s\n' 'SEEDRNG_ARGS="--seed-dir=/data/system/entropy"' > "$target/etc/default/seedrng"
mv "$target/etc/init.d/S01seedrng" "$target/etc/init.d/S03seedrng" 2>/dev/null || :
install -m 644 "$project/initramfs/production/storage.sh" "$target/usr/lib/y2-storage.sh"
# Never package host private keys, device calibration, or a universal login key.
install -m 755 "$Y2_ARTIFACT_DIR/y2-platform-start" "$target/usr/sbin/y2-platform-start"
install -m 755 "$project/tools/production/y2-status" "$target/usr/sbin/y2-status"
mkdir -p "$target/usr/lib/y2-platform/y2_platform"
install -m 644 "$project"/tools/platform/y2_platform/*.py "$target/usr/lib/y2-platform/y2_platform/"
install -m 644 "$project/tools/platform/main.py" "$target/usr/lib/y2-platform/main.py"
install -m 644 "$project/tools/platform/suspend-record.py" "$target/usr/lib/y2-platform/suspend-record.py"
install -m 644 "$project/tools/platform/product-sleep.py" "$target/usr/lib/y2-platform/product-sleep.py"
for command in y2-platform y2-status y2-health y2-suspend-record y2-power-audit; do
    install -m 755 "$project/tools/platform/$command" "$target/usr/sbin/$command"
done
install -m 644 "$project/tools/platform/capabilities.json" "$target/etc/y2linux/capabilities.json"
codec_profile=sbc-only
if grep -qx 'BR2_PACKAGE_Y2_CODEC_EXPERIMENTS=y' "$Y2_ARTIFACT_DIR/buildroot/.config"; then
    # RELEASE-49: without aptX this is the public codec set (SBC, AAC, LDAC).
    if grep -qx 'BR2_PACKAGE_Y2_CODEC_APTX=y' "$Y2_ARTIFACT_DIR/buildroot/.config"; then
        codec_profile=owner-private-integration
    else
        codec_profile=public
    fi
fi
python3 "$project/tools/platform/codec_manifest.py" "$Y2_ARTIFACT_DIR/buildroot/build/bluez-alsa-5.0.0" \
    --profile "$codec_profile" --daemon-args "$target/etc/y2linux/bluetooth-daemon.args" \
    > "$target/etc/y2linux/bluetooth-codecs.json"
for command in poweroff reboot; do
    rm -f "$target/sbin/$command"
    install -m 755 "$project/tools/platform/y2-power-command" "$target/sbin/$command"
done
install -m 755 "$Y2_ARTIFACT_DIR/y2-usb-status" "$target/usr/sbin/y2-usb-status"
printf '%s\n' y2-platform-v1 > "$target/etc/y2linux/platform-contract"
# The one OpenSSH build directory of this configuration (version follows Buildroot's pin).
set -- "$Y2_ARTIFACT_DIR"/buildroot/build/openssh-[0-9]*/sftp-server
[ "$#" -eq 1 ] && [ -f "$1" ] || { echo "expected exactly one OpenSSH sftp-server build: $*" >&2; exit 1; }
install -m 755 "$1" "$target/usr/libexec/y2-sftp-server"
install -m 644 "$project/tools/update/trust.json" "$target/etc/y2linux/update-trust.json"
install -m 644 "$project/tools/update/channel.json" "$target/etc/y2linux/update-channel.json"
# Last.fm: the owner's app key when the build has one (tools/production/build.py).
rm -f "$target/usr/share/reborn/lastfm-api.json"
if [ -f "$Y2_ARTIFACT_DIR/lastfm-api.json" ]; then
    install -D -m 600 "$Y2_ARTIFACT_DIR/lastfm-api.json" "$target/usr/share/reborn/lastfm-api.json"
fi
# Error reports: the receiver's upload key when the build has one (tools/production/build.py).
rm -f "$target/usr/share/y2linux/report-upload.json"
if [ -f "$Y2_ARTIFACT_DIR/report-upload.json" ]; then
    install -D -m 600 "$Y2_ARTIFACT_DIR/report-upload.json" "$target/usr/share/y2linux/report-upload.json"
fi
python3 - "$target/etc/y2linux" <<'PYUPDATE'
import json, pathlib, sys
root=pathlib.Path(sys.argv[1]); versions=json.loads((root/'versions.json').read_text())
caps=json.loads((root/'capabilities.json').read_text())
caps['platform_version']=versions['release_version']
(root/'capabilities.json').write_text(json.dumps(caps, indent=2)+'\n')
# OTA-35: full-system packages; the installed release's own sequence is the
# downgrade floor until a recovery install records a higher accepted one.
(root/'update-compat.json').write_text(json.dumps(dict(schema=2, product='Y2',
    hardware_revision='innioasis-y2-mt6582', kernel=versions['kernel_version'],
    rootfs_contract='y2-platform-v1', layout_schema=1, data_schema=1, recovery_api=1,
    sequence=versions.get('update_sequence', 0),
    feature_contract_version=versions.get('feature_contract_version', 1),
    battery_profile_schema=versions.get('battery_profile_schema', 1),
    low_battery_policy_schema=versions.get('low_battery_policy_schema', 1)), separators=(',',':')))
PYUPDATE
# Keep bond writers stopped when an owner reset was interrupted. Match the
# pinned upstream service's explicit function, and reject an unknown structure.
python3 "$project/tools/production/service_guards.py" "$target/etc/init.d/S40bluetoothd"
# RELEASE-49: no MediaTek radio firmware in any image (it may not be
# redistributed). Each player keeps the reviewed files from its owner's copy on
# Y2DATA; the kernel and y2-factory find them through this link.
rm -rf "$target/lib/firmware/mediatek/mt6582"
mkdir -p "$target/lib/firmware/mediatek"
ln -s /data/firmware/mt6582 "$target/lib/firmware/mediatek/mt6582"
# D-Bus identity and all radio state are generated/persisted on Y2DATA.
[ ! -f "$target/var/lib/dbus/machine-id" ]
[ ! -f "$target/etc/machine-id" ]
