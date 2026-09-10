#!/usr/bin/env python3
"""Package the built ALSA files for a persistent, additive Y2ROOT update.

Reuses canonical Buildroot output. Does not rebuild, deploy, play audio, replace
libc/loader, alter services/mixer state or change the whole-rootfs build identity.
Run the generated install.sh on the Y2 after transferring the output directory.
"""
import argparse
import hashlib
import io
import tarfile
from pathlib import Path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def build(root, output):
    target = root / 'buildroot/target'
    files = {}

    def include(name):
        src = target / name
        # Flatten runtime symlinks to their actual built content. All destinations
        # come from this fixed allowlist, never from a remote archive.
        files[name] = (src.read_bytes(), src.stat().st_mode & 0o777)

    for name in ('aplay', 'amixer', 'speaker-test'):
        include('usr/bin/' + name)
    include('usr/lib/libasound.so.2')
    include('usr/sbin/y2-audio-collect')
    for directory in ('usr/share/alsa', 'usr/share/y2linux/audio'):
        for path in sorted((target / directory).rglob('*')):
            if path.is_file():
                include(str(path.relative_to(target)))
    sums = ''.join(digest(data) + '  ' + name + '\n'
                   for name, (data, _) in sorted(files.items()))
    files['etc/y2linux/audio-tools.sha256'] = (sums.encode(), 0o644)
    runtime = ''.join(digest((target / name).read_bytes()) + '  /' + name + '\n'
                      for name in ('lib/libc.so.6', 'lib/libm.so.6',
                                   'lib/ld-linux-armhf.so.3'))
    output.mkdir(parents=True, exist_ok=True)
    archive = output / 'audio-tools.tar'
    with tarfile.open(archive, 'w') as tar:
        for name, (data, mode) in sorted(files.items()):
            entry = tarfile.TarInfo(name)
            entry.mode, entry.size = mode, len(data)
            tar.addfile(entry, io.BytesIO(data))
    (output / 'runtime.sha256').write_text(runtime)
    (output / 'SHA256SUMS').write_text(
        digest(archive.read_bytes()) + '  audio-tools.tar\n' +
        digest(runtime.encode()) + '  runtime.sha256\n')
    (output / 'install.sh').write_text('''#!/bin/sh
set -eu
cd "$(dirname "$0")"
[ "$(id -u)" = 0 ]
awk '$2 == "/" && $3 == "ext4" && $4 ~ /(^|,)rw(,|$)/ { ok=1 } END { exit !ok }' /proc/mounts
rootdev=$(awk '$2 == "/" && $3 == "ext4" { print $1 }' /proc/mounts)
case "$(readlink -f "/sys/class/block/${rootdev##*/}")" in
    */11240000.mmc/*) ;; *) echo 'Root is not the removable Y2 SD' >&2; exit 1;;
esac
sha256sum -c SHA256SUMS
sha256sum -c runtime.sha256
# These audio-only paths must be absent, regular files, or directories. Refuse
# destination symlinks so extraction cannot redirect writes outside the paths.
for file in $(tar -tf audio-tools.tar); do
    path=/$file
    while [ "$path" != / ]; do
        [ ! -L "$path" ] || { echo "Refusing destination symlink: $path" >&2; exit 1; }
        path=${path%/*}
        [ -n "$path" ] || path=/
    done
done
tar -xf audio-tools.tar -C /
cd /
sha256sum -c /etc/y2linux/audio-tools.sha256
sync
aplay --version
amixer --version
aplay -l
aplay -L
amixer -c Y2Audio sget Headphone
echo 'Persistent ALSA tools installed on Y2ROOT; no playback or mixer changes.'
''')
    print(f'{archive}: {archive.stat().st_size} bytes; SHA256 {digest(archive.read_bytes())}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    build(args.root, args.output)
