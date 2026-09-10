"""D08 physical layout contract. All intervals are half-open; never expand RAM."""
from dataclasses import dataclass, asdict

L = 0x80008000
RAM = ((0x80000000, 0x81800000), (0x84000000, 0x84080000))
RESERVED = ((0x80000000, 0x80004000),)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def u32(value):
    require(type(value) is int and 0 <= value < 2**32, '32-bit arithmetic overflow/type')
    return value


def up(value, alignment):
    return u32((u32(value) + alignment - 1) // alignment * alignment)


def down(value, alignment):
    return u32(value) // alignment * alignment


def span(start, size):
    require(size > 0, 'empty/negative interval')
    return [u32(start), u32(start + size)]


def overlaps(a, b):
    return a[0] < b[1] and b[0] < a[1]


@dataclass(frozen=True)
class Inputs:
    z: int
    dtb: int  # raw totalsize, rounded here exactly as head.S
    initramfs: int
    image: int
    kernel_bss: int
    kernel_span: int
    restart: int
    wont_overwrite: int
    edata: int
    reloc_code_end: int
    stack_top: int
    compressed_bss_start: int
    compressed_bss_end: int
    regular_file_bytes: int


def validate(x, *, initramfs_cap=0x80000, regular_file_cap=0x200000):
    for name, value in asdict(x).items():
        u32(value)
    d = up(x.dtb, 8)
    require(40 <= x.dtb and d <= 0x10000, 'DTB size cap')
    require(x.z > 0 and x.z % 8 == 0 and x.edata == x.z, 'zImage/_edata alignment/length')
    require(x.z + d <= 0x600000, 'packed kernel size cap')
    require(0 < x.initramfs <= initramfs_cap, 'initramfs size cap')
    require(0 < x.image <= 0xc00000, 'Image size cap')
    require(0 < x.kernel_bss <= 0x200000, 'kernel BSS size cap')
    require(x.image + x.kernel_bss <= x.kernel_span <= 0xe00000, 'resident kernel span cap')
    require(0 < x.regular_file_bytes <= regular_file_cap, 'unpacked regular files cap')
    require(0 < x.restart < x.wont_overwrite < x.reloc_code_end <= x.z, 'compressed code symbol order')
    require(x.z <= x.compressed_bss_start < x.compressed_bss_end <= x.stack_top, 'compressed BSS/stack order')
    q = down(x.reloc_code_end - x.restart + 256, 256)
    require(0 < q <= 0x10000, 'relocation code allowance')
    adjusted = u32(x.image + max(0, x.kernel_bss - (x.edata - x.wont_overwrite)))
    # First pass: neither safety branch may bypass relocation for this in-place contract.
    require(L < L + x.stack_top + d + 0x10000 + 0x4000, 'first collision branch')
    require(L + adjusted > L + x.wont_overwrite, 'first pass unexpectedly avoids relocation')
    source = down(L + x.restart, 32)
    target = down(L + adjusted + q, 256)
    copied = up(L + x.z + d - source, 32)
    delta = target - source
    require(delta > 0, 'non-positive relocation delta')
    image = span(L, x.image)
    resident = span(L, x.kernel_span)
    relocated_copy = span(target, copied)
    dtb = span(L + x.z + delta, d)
    bss_start = L + x.compressed_bss_start + d + delta
    bss = span(bss_start, up(x.compressed_bss_end - x.compressed_bss_start, 16))
    stack_top = u32(L + x.stack_top + d + delta)
    malloc = span(stack_top, 0x10000)
    workspace_end = max(bss[1], malloc[1], relocated_copy[1])
    require(workspace_end - dtb[1] <= 0x40000, 'compressed workspace extension cap')
    # Second pass uses the same inflated-size adjustment and must avoid another relocation.
    require(L + adjusted <= L + delta + x.wont_overwrite, 'second pass would relocate again')
    for name, interval in [('copy', relocated_copy), ('BSS', bss), ('malloc', malloc),
                           ('stack', [bss[1], stack_top])]:
        require(not overlaps(interval, image), 'live ' + name + ' overlaps Image output')
        require(L <= interval[0] <= interval[1] < RAM[0][1], name + ' outside low RAM')
    require(not overlaps(dtb, resident), 'surviving DTB overlaps resident kernel')
    k, r, p = 512 + x.z + d, 512 + x.initramfs, 2048
    t = u32(up(k, p) + up(r, p) + 2 * p)
    staging = span(L - 512, t)
    staged_initrd = span(L + up(k, p), x.initramfs)
    initrd = span(0x84000000, x.initramfs)
    require(staging[0] >= 0x80004000 and staging[1] < RAM[0][1], 'LK staging outside low RAM')
    require(staging[0] <= staged_initrd[0] < staged_initrd[1] <= staging[1], 'ramdisk staging bounds')
    require(not overlaps(staging, initrd), 'LK staging overwrites initramfs')
    require(p + t <= 0x1000000, 'BOOTIMG partition overread')
    return {'policy': 'D08', 'inputs': asdict(x), 'ram': RAM, 'reserved': RESERVED,
            'zimage': span(L, x.z), 'appended_dtb': span(L + x.z, d),
            'image': image, 'resident_kernel': resident, 'relocated_copy': relocated_copy,
            'relocated_dtb': dtb, 'compressed_bss': bss, 'stack_top': stack_top,
            'malloc': malloc, 'initramfs': initrd, 'lk_staging': staging,
            'staged_initramfs': staged_initrd, 'bootimg_read_end': p + t,
            'relocation_delta': delta, 'relocation_code_allowance': q,
            'adjusted_image_size': adjusted, 'workspace_end': workspace_end}
