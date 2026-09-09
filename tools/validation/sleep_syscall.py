"""Check the linked ARM EABI syscall 162, including weak ENOSYS aliases."""
import struct
from tools.build.check_config import parse
from tools.validation.d08 import require


def slot_offset(kernel, number=162):
    address = kernel.sym('sys_call_table') + number * 4
    sections = [s for s in kernel.elf.iter_sections()
                if s['sh_flags'] & 2 and s['sh_type'] != 'SHT_NOBITS'
                and s['sh_addr'] <= address
                and address + 4 <= s['sh_addr'] + s['sh_size']]
    require(len(sections) == 1, 'sleep syscall table slot must be file-backed')
    section = sections[0]
    return section['sh_offset'] + address - section['sh_addr']


def check(kernel, config):
    require(parse(config).get('CONFIG_COMPAT_32BIT_TIME') == 'y',
            'PID1 syscall 162 requires CONFIG_COMPAT_32BIT_TIME=y')
    entry = struct.unpack_from('<I', kernel.data, slot_offset(kernel))[0]
    implementation = kernel.sym('sys_nanosleep_time32')
    unavailable = kernel.sym('sys_ni_syscall')
    require(entry != unavailable and implementation != unavailable,
            'PID1 sleep syscall 162 resolves to sys_ni_syscall (ENOSYS)')
    require(entry == implementation, 'PID1 syscall 162 must target sys_nanosleep_time32')
    symbols = kernel.elf.get_section_by_name('.symtab').get_symbol_by_name('sys_nanosleep_time32')
    require(any(s['st_info']['bind'] == 'STB_GLOBAL' and
                s['st_info']['type'] == 'STT_FUNC' and s['st_size'] > 0
                and isinstance(s['st_shndx'], int) for s in symbols),
            'nanosleep_time32 must have a real, non-weak function definition')
    return {'number': 162, 'target': 'sys_nanosleep_time32',
            'entry': hex(entry), 'sys_ni_syscall': hex(unavailable),
            'compat_32bit_time': True, 'status': 'PASS linked implementation, not ENOSYS'}
