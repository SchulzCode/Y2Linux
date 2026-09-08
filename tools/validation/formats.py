"""Strict binary readers for the small, deliberately fixed first-boot formats."""
import stat
import struct
import zlib
from tools.validation.d08 import require


def gunzip(data, limit):
    require(data[:3] == b'\x1f\x8b\x08', 'gzip signature')
    stream = zlib.decompressobj(31)
    raw = stream.decompress(data, limit + 1)
    require(len(raw) <= limit and stream.eof and not stream.unused_data and
            not stream.unconsumed_tail, 'gzip size/truncation/trailing member')
    return raw


def cpio(data):
    expected = [('.', stat.S_IFDIR | 0o755, 0, 0), ('dev', stat.S_IFDIR | 0o755, 0, 0),
                ('dev/console', stat.S_IFCHR | 0o600, 5, 1),
                ('dev/null', stat.S_IFCHR | 0o666, 1, 3),
                ('proc', stat.S_IFDIR | 0o555, 0, 0), ('sys', stat.S_IFDIR | 0o555, 0, 0),
                ('init', stat.S_IFREG | 0o755, 0, 0), ('TRAILER!!!', 0, 0, 0)]
    cursor = 0
    init = None
    for ino, (name, mode, major, minor) in enumerate(expected, 1):
        header = data[cursor:cursor + 110]
        require(len(header) == 110 and header[:6] == b'070701', 'newc header')
        fields = [int(header[i:i+8], 16) for i in range(6, 110, 8)]
        i, m, uid, gid, links, mtime, size, dmaj, dmin, rmaj, rmin, namesize, crc = fields
        require((i, m, uid, gid, links, mtime, dmaj, dmin, rmaj, rmin, crc) ==
                (ino, mode, 0, 0, 2 if stat.S_ISDIR(mode) else 1, 0, 0, 0, major, minor, 0),
                'newc metadata differs from reviewed archive')
        cursor += 110
        require(data[cursor:cursor+namesize] == name.encode() + b'\0', 'newc path/order')
        cursor += namesize
        aligned = (cursor + 3) & ~3
        require(not any(data[cursor:aligned]), 'newc name padding')
        cursor = aligned
        body = data[cursor:cursor+size]
        require(len(body) == size, 'newc truncated contents')
        if name == 'init':
            require(0 < size <= 0x200000, 'unpacked file cap')
            init = body
        else:
            require(size == 0, 'unexpected newc file contents')
        cursor += size
        aligned = (cursor + 3) & ~3
        require(not any(data[cursor:aligned]), 'newc content padding')
        cursor = aligned
    require(len(data) == (cursor + 511) & ~511 and not any(data[cursor:]), 'newc trailer padding')
    return init


def fdt(data):
    require(len(data) >= 40, 'FDT truncated header')
    magic, size, off_struct, off_strings, off_reserve, version, last, cpu, sz_strings, sz_struct = struct.unpack_from('>10I', data)
    require(magic == 0xd00dfeed and size == len(data) and version == 17 and last == 16 and cpu == 0, 'FDT header/version/size/bootcpu')
    require(40 <= off_reserve < off_struct and off_reserve % 8 == 0 and off_struct % 4 == 0,
            'FDT block offsets')
    require(off_struct + sz_struct <= off_strings and off_strings + sz_strings == size, 'FDT block bounds')
    cursor = off_reserve
    reservations = []
    while True:
        require(cursor + 16 <= off_struct, 'FDT reservation terminator')
        start, length = struct.unpack_from('>QQ', data, cursor); cursor += 16
        if not start and not length: break
        require(length > 0 and start + length <= 2**32, 'FDT reservation overflow')
        reservations.append((start, start + length))
    require(not any(data[cursor:off_struct]), 'FDT reserve padding')
    strings = data[off_strings:]
    nodes, stack = {}, []
    cursor, end = off_struct, off_struct + sz_struct
    while cursor + 4 <= end:
        token, = struct.unpack_from('>I', data, cursor); cursor += 4
        if token == 1:
            stop = data.find(b'\0', cursor, end)
            require(stop >= cursor, 'FDT unterminated node')
            name = data[cursor:stop].decode('ascii')
            require('/' not in name and (stack or name == ''), 'FDT node name/root')
            stack.append(name)
            path = '/'.join(stack) or '/'
            require(path not in nodes, 'duplicate FDT node')
            nodes[path] = {}
            cursor = (stop + 4) & ~3
        elif token == 2:
            require(bool(stack), 'FDT stack underflow'); stack.pop()
        elif token == 3:
            require(bool(stack) and cursor + 8 <= end, 'FDT property bounds')
            length, nameoff = struct.unpack_from('>II', data, cursor); cursor += 8
            require(cursor + length <= end and nameoff < len(strings), 'FDT property bounds')
            stop = strings.find(b'\0', nameoff)
            require(stop >= nameoff, 'FDT property name')
            name = strings[nameoff:stop].decode('ascii')
            props = nodes['/'.join(stack) or '/']
            require(name not in props, 'duplicate FDT property')
            props[name] = data[cursor:cursor+length]
            cursor = (cursor + length + 3) & ~3
        elif token == 9:
            require(not stack and cursor == end, 'FDT end/stack/trailing bytes')
            return nodes, reservations
        else:
            raise ValueError('Unexpected FDT token (including NOP)')
    raise ValueError('Missing FDT end token')
