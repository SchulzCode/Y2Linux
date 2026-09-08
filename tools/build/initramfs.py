"""Create the deliberately fixed deterministic diagnostic newc/gzip image."""
import gzip
from pathlib import Path
import stat


def archive(init):
    if not 0 < len(init) <= 0x200000:
        raise ValueError('D08 unpacked regular-file cap')
    entries = [('.', stat.S_IFDIR | 0o755, b'', 0, 0),
               ('dev', stat.S_IFDIR | 0o755, b'', 0, 0),
               ('dev/console', stat.S_IFCHR | 0o600, b'', 5, 1),
               ('dev/null', stat.S_IFCHR | 0o666, b'', 1, 3),
               ('proc', stat.S_IFDIR | 0o555, b'', 0, 0),
               ('sys', stat.S_IFDIR | 0o555, b'', 0, 0),
               ('init', stat.S_IFREG | 0o755, init, 0, 0),
               ('TRAILER!!!', 0, b'', 0, 0)]
    result = bytearray()
    for ino, (name, mode, data, major, minor) in enumerate(entries, 1):
        name = name.encode() + b'\0'
        fields = [ino, mode, 0, 0, 2 if stat.S_ISDIR(mode) else 1, 0, len(data),
                  0, 0, major, minor, len(name), 0]
        result += b'070701' + ''.join(f'{x:08x}' for x in fields).encode() + name
        result += bytes(-len(result) % 4)
        result += data
        result += bytes(-len(result) % 4)
    result += bytes(-len(result) % 512)
    return bytes(result)


def main():
    root = Path('/build')
    raw = archive((root / 'init').read_bytes())
    packed = gzip.compress(raw, compresslevel=9, mtime=0)
    if not 0 < len(packed) <= 0x80000:
        raise ValueError('D08 compressed initramfs cap')
    (root / 'initramfs.cpio').write_bytes(raw)
    (root / 'initramfs.cpio.gz').write_bytes(packed)
    (root / 'initrd-size.h').write_text(f'#define Y2_INITRD_END 0x{0x84000000 + len(packed):08x}\n')
    print(f'init={len((root / "init").read_bytes())}, cpio={len(raw)}, gzip={len(packed)} bytes')

if __name__ == '__main__':
    main()
