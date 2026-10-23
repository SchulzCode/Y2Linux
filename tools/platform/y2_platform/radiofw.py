"""Radio firmware from the owner's own copy (RELEASE-49).

Y2Linux images carry no MediaTek radio firmware: it may not be redistributed.
Each player keeps the reviewed files on Y2DATA (`/data/firmware/mt6582`, which
`/lib/firmware/mediatek/mt6582` links to). They come from the owner's copy:

* on a player that ran an older image, the recovery copies them from the old
  root before it writes the new one (recovery.sh);
* otherwise the owner copies `system.img` from the Innioasis Y2 firmware
  package into the `Firmware` folder over USB (on Internal Storage, or the
  Firmware drive on Y2DATA while there is no Internal Storage); this module reads the Android
  sparse ext4 image itself, checks the five files against the reviewed
  SHA-256 values, derives the two board defaults like
  tools/connectivity/provision.py, installs them, deletes the copy and starts
  the radios.

Only names, sizes and hashes are in this file; never firmware bytes.
"""
# SPDX-License-Identifier: GPL-2.0-only
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import time

from .common import atomic_json

# docs/knowledge/evidence/m5-firmware-inventory.json (stock v3.2.0 FM, /etc/firmware).
FILES = {
    'WIFI_RAM_CODE_MT6582': (160368, '4def37522e2bfd2d4b78e01bf9f567a37e3f67ab3d7b6646ffb317f5b2b49b82'),
    'WMT_SOC.cfg': (80, 'cea0a94b8a88e9c24e6835446ebc3106d7b100c3709ea98e8bb7368689c8a2b5'),
    'mt6572_82_patch_e1_0_hdr.bin': (33120, 'fa3be4a92540296de512d64f56272746be10bdfa2db571c8ceada2d40cd616f0'),
    'mt6572_82_patch_e1_1_hdr.bin': (17476, '96eb25489305bd472805db1da33bcc0acd37b5bb21cceda98d7e88ba1179ca37'),
    'modem_1_2g_n.img': (2424376, '5059775975cbf6ab74c43978ca8f65d9a274b83585f456134e39b09f6dc7a4f1'),
}
# provision.py: generic board defaults derived from the stock reader library.
DEFAULTS = {
    'WIFI.defaults': (512, 'eb5e87224e3a493e825179f547fc13924f564a42730ea821b4359b5b36f4defe'),
    'BT.defaults': (64, 'f224ff4abfb45a612ef03249e87e4ecfa44b7c1f9f19ee3cc78ae354764d9472'),
}
CUSTOM = ('/lib/libcustom_nvram.so', 51224, 'c6460e9649f539b56449dc19f8d20771402f539be9e73340a74a9ccf45287397')
ALL = {**FILES, **DEFAULTS}

TARGET = '/data/firmware/mt6582'
# While the firmware is missing the computer sees this folder as the MTP
# storage "Firmware" (usb.media_views); nothing else of Y2DATA is exported.
INBOX = '/data/firmware/inbox'
STATE = '/run/y2/radio-firmware.json'
FOLDER = 'Firmware'
SOURCE = 'system.img'
# With Internal Storage the copy goes there (#39: Y2DATA has room for the
# zip's 518 MB image, never for an unpacked 860 MB one).
INTERNAL_INBOX = '/media/internal/music/' + FOLDER
SD_INBOX = '/media/sd/' + FOLDER
ENOSPC = 28
README = ('Wi-Fi and Bluetooth need the radio firmware from the Innioasis Y2 firmware package.\n'
          'Copy (do not move) the file system.img from that package into this folder (about\n'
          '520 MB). The player checks it, keeps the five small radio files and deletes the copy.\n'
          'A larger system.img (about 860 MB) is an unpacked copy: if there is not enough space\n'
          'here, put it into a folder named Firmware on the SD card instead.\n')


def digest(data):
    return hashlib.sha256(data).hexdigest()


# ------------------------------------------------------------- sparse images

class Image:
    """Random access to an Android sparse image (or a raw ext4 image)."""
    RAW, FILL, DONT_CARE, CRC = 0xCAC1, 0xCAC2, 0xCAC3, 0xCAC4

    def __init__(self, path):
        self.file = open(path, 'rb')
        header = self.file.read(28)
        self.chunks = []
        if len(header) == 28 and struct.unpack_from('<I', header)[0] == 0xED26FF3A:
            (_, major, _minor, file_hdr, chunk_hdr, self.block,
             total, count, _crc) = struct.unpack('<IHHHHIIII', header)
            if major != 1 or file_hdr < 28 or chunk_hdr < 12 or self.block % 4:
                raise ValueError('unsupported_sparse_image')
            offset, at = file_hdr, 0
            for _ in range(count):
                self.file.seek(offset)
                kind, _, blocks, size = struct.unpack('<HHII', self.file.read(12))
                data = offset + chunk_hdr
                if kind == self.RAW:
                    if size != chunk_hdr + blocks * self.block:
                        raise ValueError('sparse_chunk_size')
                    self.chunks.append((at, blocks, kind, data))
                elif kind == self.FILL:
                    self.file.seek(data)
                    self.chunks.append((at, blocks, kind, self.file.read(4)))
                elif kind in (self.DONT_CARE, self.CRC):
                    if kind == self.DONT_CARE:
                        self.chunks.append((at, blocks, kind, None))
                else:
                    raise ValueError('sparse_chunk_type')
                at += blocks
                offset += size
            if at != total:
                raise ValueError('sparse_block_count')
            self.size = total * self.block
        else:
            self.block = 4096
            self.size = os.fstat(self.file.fileno()).st_size
            self.chunks = [(0, -(-self.size // 4096), self.RAW, 0)]

    def close(self):
        self.file.close()

    def read(self, offset, length):
        out = bytearray()
        while length > 0:
            block, within = divmod(offset, self.block)
            for start, blocks, kind, data in self.chunks:
                if start <= block < start + blocks:
                    take = min(length, (start + blocks - block) * self.block - within)
                    if kind == self.RAW:
                        self.file.seek(data + (block - start) * self.block + within)
                        part = self.file.read(take)
                        if len(part) != take and self.chunks[0][3] != 0:
                            raise ValueError('sparse_short_read')
                        part = part.ljust(take, b'\0')
                    elif kind == self.FILL:
                        pattern = data * (take // 4 + 2)
                        part = pattern[within % 4:within % 4 + take]
                    else:
                        part = bytes(take)
                    break
            else:
                raise ValueError('image_offset_outside')
            out += part
            offset += take
            length -= take
        return bytes(out)


# ---------------------------------------------------------------------- ext4

class Ext4:
    """Just enough ext2/3/4 to read a few named regular files."""
    def __init__(self, image):
        self.img = image
        sb = image.read(1024, 1024)
        if struct.unpack_from('<H', sb, 56)[0] != 0xEF53:
            raise ValueError('not_ext4')
        self.bs = 1024 << struct.unpack_from('<I', sb, 24)[0]
        self.first = struct.unpack_from('<I', sb, 20)[0]
        self.ipg = struct.unpack_from('<I', sb, 40)[0]
        rev = struct.unpack_from('<I', sb, 76)[0]
        self.isize = struct.unpack_from('<H', sb, 88)[0] if rev else 128
        self.incompat = struct.unpack_from('<I', sb, 96)[0]
        self.dsize = struct.unpack_from('<H', sb, 254)[0] if self.incompat & 0x80 else 32
        self.dsize = self.dsize or 32
        self.gdt = (self.first + 1) * self.bs

    def block(self, n, count=1):
        return self.img.read(n * self.bs, count * self.bs)

    def inode(self, number):
        group, index = divmod(number - 1, self.ipg)
        desc = self.img.read(self.gdt + group * self.dsize, self.dsize)
        table = struct.unpack_from('<I', desc, 8)[0]
        if self.dsize >= 64:
            table |= struct.unpack_from('<I', desc, 0x28)[0] << 32
        return self.img.read(table * self.bs + index * self.isize, self.isize)

    def data(self, number, limit):
        raw = self.inode(number)
        mode, = struct.unpack_from('<H', raw, 0)
        size = struct.unpack_from('<I', raw, 4)[0] | struct.unpack_from('<I', raw, 0x6C)[0] << 32
        if size > limit:
            raise ValueError('ext4_file_too_large')
        flags, = struct.unpack_from('<I', raw, 0x20)
        blocks = raw[0x28:0x28 + 60]
        if flags & 0x10000000:
            raise ValueError('ext4_inline_data')
        out = bytearray()
        for logical, physical, count in (self.extents(blocks) if flags & 0x80000 else self.indirect(blocks, size)):
            need = logical * self.bs
            if len(out) < need:
                out += bytes(need - len(out))
            out += self.block(physical, count) if physical else bytes(count * self.bs)
        if len(out) < size:
            out += bytes(size - len(out))
        return mode, bytes(out[:size])

    def extents(self, node, depth_guard=8):
        magic, entries, _max, depth = struct.unpack_from('<HHHH', node)
        if magic != 0xF30A or depth_guard <= 0:
            raise ValueError('ext4_extent_header')
        runs = []
        for i in range(entries):
            e = node[12 + 12 * i:24 + 12 * i]
            if depth:
                leaf = struct.unpack_from('<I', e, 4)[0] | struct.unpack_from('<H', e, 8)[0] << 32
                runs += self.extents(self.block(leaf), depth_guard - 1)
            else:
                logical, length, hi, lo = struct.unpack('<IHHI', e)
                uninit = length > 32768
                length -= 32768 if uninit else 0
                runs.append((logical, 0 if uninit else (hi << 32 | lo), length))
        return runs

    def indirect(self, blocks, size):
        ptr = struct.unpack('<15I', blocks)
        per = self.bs // 4
        want = -(-size // self.bs)
        found = []

        def walk(block, level):
            if block == 0:
                return [0] * (per ** level)
            if level == 0:
                return [block]
            out = []
            for b in struct.unpack('<%dI' % per, self.block(block)):
                if len(found) + len(out) >= want:
                    break
                out += walk(b, level - 1)
            return out
        found += list(ptr[:12])
        for level, b in ((1, ptr[12]), (2, ptr[13]), (3, ptr[14])):
            if len(found) >= want:
                break
            found += walk(b, level)
        return [(i, b, 1) for i, b in enumerate(found[:want]) if b]

    def lookup(self, path):
        number = 2
        for name in [p for p in path.split('/') if p]:
            mode, entries = self.data(number, 1 << 24)
            if not stat.S_ISDIR(mode):
                raise ValueError('ext4_not_a_directory')
            filetype = self.incompat & 0x2
            at, found = 0, None
            while at + 8 <= len(entries):
                ino, rec = struct.unpack_from('<IH', entries, at)
                length = entries[at + 6] if filetype else struct.unpack_from('<H', entries, at + 6)[0]
                if rec < 8:
                    break
                if ino and entries[at + 8:at + 8 + length] == name.encode():
                    found = ino
                    break
                at += rec
            if not found:
                raise FileNotFoundError(path)
            number = found
        return number

    def read(self, path, limit):
        mode, data = self.data(self.lookup(path), limit)
        if not stat.S_ISREG(mode):
            raise ValueError('ext4_not_a_regular_file')
        return data


# ----------------------------------------------------------- the five files

def defaults(custom):
    """provision.defaults: the generic Wi-Fi/BT board records of the stock
    reader library, with the default addresses cleared."""
    wifi = bytearray(custom[0x5a8e:0x5c8e])
    bt = bytearray(custom[0x3a18:0x3a58])
    if custom[:7] != b'\x7fELF\x01\x01\x01' or struct.unpack_from('<HH', wifi) != (0x104, 0) or \
            wifi[196:198] != b'\x01\x00':
        raise ValueError('stock_reader_library_layout')
    wifi[4:10] = bytes(6)
    bt[:6] = bytes(6)
    return {'WIFI.defaults': bytes(wifi), 'BT.defaults': bytes(bt)}


def checked(files):
    """Every reviewed file present, with the reviewed size and SHA-256."""
    for name, (size, sha) in ALL.items():
        data = files.get(name)
        if data is None or len(data) != size or digest(data) != sha:
            raise ValueError('firmware_mismatch:' + name)
    # The two Bluetooth patches in their download order (provision.py).
    order = sorted(files[n][24] & 15 for n in FILES if n.endswith('_hdr.bin'))
    if order != [1, 2]:
        raise ValueError('firmware_patch_order')
    return files


def from_system_image(path):
    """The seven files from the stock system.img (sparse or raw ext4)."""
    image = Image(path)
    try:
        fs = Ext4(image)
        files = {}
        for name, (size, _) in FILES.items():
            try:
                files[name] = fs.read('/etc/firmware/' + name, size)
            except FileNotFoundError:
                raise ValueError('not_the_y2_firmware_package') from None
        try:
            custom = fs.read(CUSTOM[0], CUSTOM[1])
        except FileNotFoundError:
            raise ValueError('not_the_y2_firmware_package') from None
        if len(custom) != CUSTOM[1] or digest(custom) != CUSTOM[2]:
            raise ValueError('firmware_mismatch:libcustom_nvram.so')
        files.update(defaults(custom))
        return checked(files)
    finally:
        image.close()


def from_directory(path):
    """The seven files from a directory (a root's /lib/firmware/mediatek/mt6582)."""
    files = {}
    for name, (size, _) in ALL.items():
        file = Path(path) / name
        info = file.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_size != size:
            raise ValueError('firmware_mismatch:' + name)
        files[name] = file.read_bytes()
    return checked(files)


def present(ctx):
    """Cheap check for status polling: every file there with its size."""
    target = ctx.path(TARGET)
    try:
        return all((target / n).lstat().st_size == size and stat.S_ISREG((target / n).lstat().st_mode)
                   for n, (size, _) in ALL.items())
    except OSError:
        return False


def installed(ctx):
    """True when every reviewed file is in place with the reviewed hash."""
    try:
        from_directory(ctx.path(TARGET))
        return True
    except (OSError, ValueError):
        return False


def install(ctx, files):
    """Write the checked files to Y2DATA: a fresh directory, then one rename."""
    checked(files)
    target = ctx.path(TARGET)
    parent = target.parent
    parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staging = parent / ('.mt6582.%d' % os.getpid())
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(mode=0o700)
    for name, data in files.items():
        fd = os.open(staging / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as out:
            out.write(data)
            out.flush()
            os.fsync(out.fileno())
    if target.is_symlink() or (target.exists() and not target.is_dir()):
        raise ValueError('firmware_target_unsafe')
    old = parent / ('.mt6582.old.%d' % os.getpid())
    if target.exists():
        target.rename(old)
    staging.rename(target)
    shutil.rmtree(old, ignore_errors=True)
    dfd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(dfd)
    finally:
        os.close(dfd)


# ------------------------------------------------------------ owner's copy

def roots(ctx):
    value = ctx.json('/run/y2/mtp-roots.json', {}) or {}
    return [ctx.path(r) for r in value.get('roots', []) if isinstance(r, str) and r.startswith('/')]


def internal(ctx):
    from .internal_media import ready
    return ready(ctx)


def inbox(ctx):
    """Where the copy goes, created with its note when needed: the Firmware
    folder on Internal Storage, else the Firmware drive's folder on Y2DATA."""
    if internal(ctx):
        folder = ctx.path(INTERNAL_INBOX)
        folder.mkdir(mode=0o755, exist_ok=True)
    else:
        folder = ctx.path(INBOX)
        folder.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        folder.mkdir(mode=0o700, exist_ok=True)
    note = folder / 'README.txt'
    if not note.exists():
        note.write_text(README)
    return folder


def candidates(ctx):
    """Where an owner's copy may be: the Firmware drive, or a Firmware folder
    on Internal Storage or the SD card (also while no computer is connected)."""
    seen = set()
    places = [ctx.path(INBOX), ctx.path(SD_INBOX)]
    if internal(ctx):
        places.insert(0, ctx.path(INTERNAL_INBOX))
    places += [root / FOLDER for root in roots(ctx) if root != ctx.path(INBOX)]
    for folder in places:
        if folder not in seen:
            seen.add(folder)
            yield folder / SOURCE


def remove(ctx, source):
    """Delete the used system.img; on the SD card inside a write window (the
    card is read-only between writes, media.py)."""
    import contextlib
    from .media import sd_writable
    on_sd = str(source).startswith(str(ctx.path(SD_INBOX)))
    try:
        with (sd_writable(ctx, 'radiofw', 30) if on_sd else contextlib.nullcontext()):
            source.unlink()
    except OSError:
        pass


def tidy(ctx):
    """After the install: the Internal Storage Firmware folder goes away again
    (only its note; anything the owner put there stays)."""
    folder = ctx.path(INTERNAL_INBOX)
    try:
        (folder / 'README.txt').unlink()
        folder.rmdir()
    except OSError:
        pass


def state(ctx):
    value = ctx.json(STATE, {}) or {}
    if present(ctx):
        return {'state': 'Ready'}
    if value.get('state') in ('Installing', 'Failed'):
        return {k: value.get(k) for k in ('state', 'reason')}
    missing = {'state': 'Missing', 'folder': FOLDER, 'file': SOURCE,
               'location': 'internal' if internal(ctx) else 'drive'}
    # The responder refuses a copy that does not fit before writing anything
    # (mtp-safe space()), so nothing is left behind; only say why (#39).
    if (ctx.json('/run/y2/mtp-events.json', {}) or {}).get('error') == ENOSPC:
        missing['reason'] = 'no_space'
    return missing


def scan(ctx, start_radios=True):
    """One pass of the owner flow: make the Firmware folder visible, then take
    a finished system.img copy from it. Returns the new state."""
    if installed(ctx):
        return leftover(ctx)
    previous = ctx.json(STATE, {}) or {}
    try:
        inbox(ctx)
    except OSError:
        pass
    for source in candidates(ctx):
        try:
            info = source.lstat()
        except OSError:
            continue
        if not stat.S_ISREG(info.st_mode):
            continue
        seen = [info.st_size, info.st_mtime_ns]
        # A copy still arriving over USB changes size or time between passes.
        if previous.get('seen') != seen or time.time() - info.st_mtime < 5:
            atomic_json(ctx.path(STATE), {'schema': 1, 'state': 'Missing', 'seen': seen}, mode=0o644)
            return {'state': 'Missing'}
        if previous.get('state') == 'Failed' and previous.get('seen') == seen:
            return {'state': 'Failed', 'reason': previous.get('reason')}
        atomic_json(ctx.path(STATE), {'schema': 1, 'state': 'Installing', 'seen': seen}, mode=0o644)
        try:
            install(ctx, from_system_image(source))
        except (OSError, ValueError) as error:
            reason = str(error)[:80]
            atomic_json(ctx.path(STATE), {'schema': 1, 'state': 'Failed', 'reason': reason, 'seen': seen},
                        mode=0o644)
            return {'state': 'Failed', 'reason': reason}
        remove(ctx, source)
        tidy(ctx)
        atomic_json(ctx.path(STATE), {'schema': 1, 'state': 'Ready', 'installed_from': 'system.img'}, mode=0o644)
        if start_radios:
            start(ctx)
        return {'state': 'Ready'}
    if previous.get('state') == 'Failed':
        # The rejected copy is gone: back to asking for it.
        atomic_json(ctx.path(STATE), {'schema': 1, 'state': 'Missing'}, mode=0o644)
    return {'state': 'Missing'}


def leftover(ctx):
    """With the firmware in place, a finished system.img copy that arrives
    later (copied again, or after the recovery kept the files) holds 520–860 MB
    of the owner's storage for nothing: delete it like the first one. Only the
    firmware package's image goes; any other file of that name stays."""
    previous = ctx.json(STATE, {}) or {}
    value = {'schema': 1, 'state': 'Ready', 'kept': previous.get('kept')}
    for source in candidates(ctx):
        try:
            info = source.lstat()
        except OSError:
            continue
        if not stat.S_ISREG(info.st_mode):
            continue
        seen = [info.st_size, info.st_mtime_ns]
        if seen == previous.get('kept'):
            continue
        # A copy still arriving changes size or time between passes.
        if previous.get('leftover') != seen or time.time() - info.st_mtime < 5:
            value['leftover'] = seen
            break
        try:
            from_system_image(source)
        except (OSError, ValueError):
            value['kept'] = seen
            break
        remove(ctx, source)
        tidy(ctx)
    if value != {k: previous.get(k) for k in value} or set(previous) - set(value):
        atomic_json(ctx.path(STATE), value, mode=0o644)
    return {'state': 'Ready'}


def leftover_waiting(ctx):
    """Cheap check for the service: a copy leftover() has not judged yet."""
    kept = (ctx.json(STATE, {}) or {}).get('kept')
    for source in candidates(ctx):
        try:
            info = source.lstat()
        except OSError:
            continue
        if stat.S_ISREG(info.st_mode) and [info.st_size, info.st_mtime_ns] != kept:
            return True
    return False


def start(ctx):
    """(Re)start the radios with the installed firmware. Stopping running
    radios can take longer than a short command limit, which on 49 cut the
    combined restart off after its stop half (archive 154)."""
    ctx.command(['/etc/init.d/S41y2-connectivity', 'stop'], timeout=90)
    ctx.command(['/etc/init.d/S41y2-connectivity', 'start'], timeout=30)


def main(ctx, action, path=None):
    if action == 'status':
        return state(ctx)
    if action == 'scan':
        return scan(ctx)
    if action == 'install':
        source = Path(path)
        files = from_directory(source) if source.is_dir() else from_system_image(source)
        install(ctx, files)
        start(ctx)
        return {'state': 'Ready'}
    raise ValueError('unknown_firmware_action')


if __name__ == '__main__':  # host check: python3 -m y2_platform.radiofw IMAGE
    import sys
    print(json.dumps({n: digest(d) for n, d in from_system_image(sys.argv[1]).items()}, indent=1))
