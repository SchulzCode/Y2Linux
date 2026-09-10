"""Validate real ARM ELF, DTB, gzip and appended payload against D08."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
from elftools.elf.elffile import ELFFile
from tools.build.check_config import check as check_config
from tools.validation import dtb
from tools.validation.d08 import Inputs, validate, require
from tools.validation.formats import gunzip, cpio
from tools.validation.bootimg import check as check_bootimg
from tools.validation.sleep_syscall import check as check_sleep
from tools.validation.usb import check as check_usb


def digest(data): return hashlib.sha256(data).hexdigest()


class Elf:
    def __init__(self, path):
        self.data = path.read_bytes()
        self.elf = ELFFile(io.BytesIO(self.data))
        require(self.elf.elfclass == 32 and self.elf.little_endian and
                self.elf['e_machine'] == 'EM_ARM' and self.elf['e_type'] == 'ET_EXEC', 'ELF ARM32 LE executable')
        require(self.elf['e_entry'] % 4 == 0, 'ELF entry must be ARM instruction aligned')
        table = self.elf.get_section_by_name('.symtab')
        require(table is not None, 'ELF symbol table required')
        self.syms = {}
        for s in table.iter_symbols():
            if s['st_shndx'] != 'SHN_UNDEF' and s.name:
                self.syms.setdefault(s.name, set()).add(s['st_value'])
        require(not any(n.startswith('__hyp_') for n in self.syms), 'unexpected hyp startup symbols')

    def sym(self, name):
        values = self.syms.get(name, set())
        require(len(values) == 1, 'missing/ambiguous ELF symbol ' + name)
        return next(iter(values))

    def binary(self, base, limit):
        sections = [s for s in self.elf.iter_sections() if s['sh_flags'] & 2 and
                    s['sh_type'] != 'SHT_NOBITS' and s['sh_size']]
        mapped = []
        for s in sections:
            segments = [p for p in self.elf.iter_segments() if p['p_type'] == 'PT_LOAD'
                and p['p_offset'] <= s['sh_offset']
                and s['sh_offset'] + s['sh_size'] <= p['p_offset'] + p['p_filesz']
                and p['p_vaddr'] <= s['sh_addr']
                and s['sh_addr'] + s['sh_size'] <= p['p_vaddr'] + p['p_memsz']]
            require(len(segments) == 1, 'ELF section must have one load segment')
            seg = segments[0]
            # ARM vectors/stubs have high VMAs but contiguous Image load addresses.
            lma = seg['p_paddr'] + s['sh_addr'] - seg['p_vaddr']
            mapped.append((lma, s))
        end = max(a + s['sh_size'] for a, s in mapped)
        require(0 < end - base <= limit, 'ELF binary bounds')
        raw = bytearray(end - base)
        occupied = []
        for lma, s in mapped:
            a, b = lma - base, lma + s['sh_size'] - base
            require(0 <= a < b <= len(raw) and not any(a < y and x < b for x,y in occupied), 'ELF section overlap/bounds')
            raw[a:b] = s.data(); occupied.append((a,b))
        return bytes(raw)


def check(root, project):
    check_config(root / 'kernel/.config', project / 'kernel/config/first-boot.config')
    image = (root / 'kernel/arch/arm/boot/Image').read_bytes()
    z = (root / 'kernel/arch/arm/boot/zImage').read_bytes()
    tree = (root / 'y2.dtb').read_bytes()
    initrd = (root / 'initramfs.cpio.gz').read_bytes()
    require(0 < len(initrd) <= 0x80000 and len(tree) <= 0x10000 and len(z) <= 0x600000, 'artifact input caps')
    module = (root/'display.ko').read_bytes()
    require(module == (root/'kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko').read_bytes(),
            'module differs from compiled output')
    module_elf = ELFFile(io.BytesIO(module))
    require(module_elf.elfclass == 32 and module_elf.little_endian and
            module_elf['e_machine'] == 'EM_ARM' and module_elf['e_type'] == 'ET_REL', 'module ARM32 ABI')
    info = module_elf.get_section_by_name('.modinfo').data().split(b'\0')
    require(b'name=mediatek_drm' in info and b'depends=' in info and
            any(x.startswith(b'vermagic=6.18.0-y2-m2-baseline3 ') for x in info), 'module identity/dependencies')
    require((root/'kernel/modules.order').read_text().splitlines() ==
            ['drivers/gpu/drm/mediatek/mediatek-drm.o'], 'exactly one permitted module')
    init = cpio(gunzip(initrd, 0x210000), module)
    require(init == (root / 'init').read_bytes(), 'archive /init differs from executable')
    initelf = Elf(root / 'init')
    require(not any(s['p_type'] in ('PT_INTERP','PT_DYNAMIC') for s in initelf.elf.iter_segments()), 'init must be static')
    require(initelf.elf['e_flags'] & 0xff000600 == 0x05000200, 'init EABI5 soft float')
    decoded = dtb.check(tree, len(initrd))
    kernel = Elf(root / 'kernel/vmlinux')
    sleep = check_sleep(kernel, root / 'kernel/.config')
    usb = check_usb(kernel)
    from tools.validation.sleep_syscall import slot_offset
    loader_calls = {}
    for nr, name in ((114, 'sys_wait4'), (120, 'sys_clone'),
                     (241, 'sys_sched_setaffinity'), (379, 'sys_finit_module')):
        target = struct.unpack_from('<I', kernel.data, slot_offset(kernel, nr))[0]
        require(target == kernel.sym(name) and target != kernel.sym('sys_ni_syscall'),
                'display loader syscall unavailable: ' + name)
        loader_calls[str(nr)] = name
    comp = Elf(root / 'kernel/arch/arm/boot/compressed/vmlinux')
    text = kernel.sym('_text')
    require(text == 0xc0008000 and kernel.elf['e_entry'] == text, 'kernel virtual entry/TEXT_OFFSET')
    require(kernel.sym('_edata') == kernel.sym('__bss_start'), 'unexpected gap before kernel BSS')
    require(kernel.binary(text, 0xc00000) == image, 'Image differs from kernel ELF')
    start = comp.sym('_start')
    require(start == 0 and comp.elf['e_entry'] == start, 'compressed link/entry address')
    require(comp.binary(start, 0x600000) == z, 'zImage differs from compressed ELF')
    off = lambda name: comp.sym(name) - start
    # D12: inspect emitted ARM instructions, not only config/source intent.
    begin, end = off('y2_wdt_stop_begin'), off('y2_wdt_stop_end')
    expected_wdt = struct.pack('<10I', 0xe3070000, 0xe3410000, 0xe5901000,
        0xe3c11001, 0xe3811422, 0xe5801000, 0xf57ff04f, 0xe5901000,
        0xe3110001, 0x1afffffe)
    continuation = off('y2_wdt_continue')
    branch_pc = end - 4
    require(end <= continuation < off('restart') and (continuation - branch_pc - 8) % 4 == 0,
            'D12 continuation must skip linker padding')
    branch = 0xea000000 | (((continuation - branch_pc - 8) // 4) & 0xffffff)
    expected_wdt += struct.pack('<I', branch)
    require(z[begin:end] == expected_wdt, 'D12 early watchdog instructions and continuation')
    require(0x30 < begin < end < off('restart'), 'watchdog must precede relocation/inflation')
    require(kernel.sym('y2_diagnostic_init') >= text, 'built-in guarded video diagnostic missing')
    require(kernel.sym('y2_text_write') >= text, 'D14 guarded text writer missing')
    require(kernel.sym('y2_power_snapshot') >= text, 'M2 cached PWRAP snapshot missing')
    require(b'M2-BASELINE-03\0' in init, 'M2 prerequisite build identifier missing')
    require(b'Y2BASELINE M2-BASELINE-03' in image, 'Diagnostic heading/build mismatch')
    require(b'/dev/y2diag\0' in init, 'PID1 diagnostic endpoint missing')
    require(b'/dev/y2input0\0' in init, 'PID1 evdev endpoint missing')
    for symbol in ('pins_probe', 'wrap_probe', 'y2_clocks_probe', 'gc9503v_probe', 'mt6582_keypad_probe', 'mtk_i2c_probe', 'msdc_drv_probe', 'gpio_keys_polled_probe', 'evdev_read', 'evdev_ioctl'):
        require(kernel.sym(symbol) >= text, 'GPIO/evdev implementation missing: ' + symbol)
    require(struct.unpack_from('<III', z, 0x24) == (0x016f2818, start, comp.sym('_edata')), 'zImage header')
    require(off('_edata_real') == off('_edata') == len(z), 'zImage real end')
    bss = kernel.sym('__bss_stop') - kernel.sym('__bss_start')
    require(comp.sym('_kernel_bss_size') == bss, 'embedded kernel BSS size symbol')
    require(struct.unpack_from('<I', z, off('__piggy_size_addr'))[0] == len(image), 'inflated-size word')
    require(off('__piggy_size_addr') == off('input_data_end') - 4, 'piggy size location')
    require(gunzip(z[off('input_data'):off('input_data_end')], 0xc00000) == image, 'embedded gzip differs from Image')
    table = struct.unpack_from('<7I', z, off('_table_start'))
    require(table == (6,0x5a534c4b,off('__piggy_size_addr'),bss,0x8000,0x10000,0), 'compressed layout table/TEXT_OFFSET/malloc')
    lc = off('LC1')
    stack, edata = struct.unpack_from('<ii', z, lc)
    stack += lc; edata += lc
    require(edata == len(z), 'LC1 edata disagrees')
    stack_section = comp.elf.get_section_by_name('.stack')
    require(stack_section is not None and stack_section['sh_size'] == 4096 and
            stack_section['sh_addr'] + stack_section['sh_size'] - start == stack, 'LC1 stack decode')
    x = Inputs(len(z),len(tree),len(initrd),len(image),bss,kernel.sym('_end')-text,
        off('restart'),off('wont_overwrite'),off('_edata'),off('reloc_code_end'),stack,
        off('__bss_start'),off('_end'),len(init)+len(module))
    layout = validate(x)
    padded = tree + bytes(-len(tree) % 8)
    expected = z + padded
    appended = root / 'zImage-dtb'
    if appended.exists(): require(appended.read_bytes() == expected, 'appended payload differs from zImage + padded DTB')
    # Caller may create the payload only after all evidence passes.
    layout['diagnostic'] = {'watchdog_entry_offset':begin, 'watchdog_end_offset':end, 'watchdog_continuation_offset':continuation,
        'watchdog_instruction_sha256':digest(expected_wdt), 'framebuffer':'DRM allocated; no legacy raw framebuffer access',
        'policy':'M2-BASELINE-03 shared core; 300s owner limit; offline only'}
    source_paths = ['kernel/diagnostic/board.c', 'kernel/diagnostic/policy.h', 'kernel/diagnostic/text.h',
        'kernel/diagnostic/pwrap.h', 'kernel/diagnostic/usb_clock.h', 'kernel/diagnostic/usb_state.h', 'kernel/diagnostic/usb_wake.h', 'initramfs/status.h',
        'initramfs/init.c', 'initramfs/start.S', 'initramfs/relay.h', 'initramfs/evdev.h', 'initramfs/display.h',
        'kernel/gpio/gpio-mt6582-input.c',
        'kernel/usb/y2_musb.c', 'kernel/usb/session.h', 'kernel/usb/gate.h', 'kernel/usb/live.h',
        'kernel/config/first-boot.config',
        'kernel/dts/innioasis-y2-first-boot.dts', 'kernel/patches/manifest.json']
    source_paths += [str(p.relative_to(project)) for p in sorted((project/'kernel/platform').glob('*')) if p.is_file()]
    source_paths += ['kernel/patches/' + item['patch'] for item in
        json.loads((project/'kernel/patches/manifest.json').read_text())['overlays']]
    layout['diagnostic']['source_sha256'] = {
        name:digest((project/name).read_bytes()) for name in source_paths}
    layout['dt'] = decoded
    layout['sleep_syscall'] = sleep
    layout['usb'] = usb
    layout['display_loader'] = {'syscalls': loader_calls, 'module_bytes': len(module),
                                'module_sha256': digest(module), 'trigger': 'once after LOG1'}
    layout['artifacts'] = {name: {'bytes':len(data),'sha256':digest(data)} for name,data in
        [('Image',image),('zImage',z),('y2.dtb',tree),('initramfs.cpio.gz',initrd),('init',init),('display.ko',module),('zImage-dtb',expected)]}
    layout['kernel_symbols'] = {name:kernel.sym(name) for name in ['_text','_edata','__bss_start','__bss_stop','_end']}
    layout['compressed_symbols'] = {name:comp.sym(name) for name in ['_start','restart','wont_overwrite','reloc_code_end','_edata','__bss_start','_end','LC1','input_data','input_data_end']}
    if (root / 'BOOTIMG.img').exists():
        layout['bootimg'] = check_bootimg((root / 'BOOTIMG.img').read_bytes(), expected, initrd, layout)
    layout['status'] = 'PASS offline D08; owner hardware test pending'
    return layout, expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('--append', action='store_true', help='write payload only after validation')
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[2]
    report, payload = check(args.root, project)
    if args.append: (args.root / 'zImage-dtb').write_bytes(payload)
    (args.root / 'layout.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__': main()
