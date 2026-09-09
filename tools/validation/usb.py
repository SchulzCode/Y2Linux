"""Linked implementations required by the first guarded CDC ACM candidate."""
import struct
from tools.validation.d08 import require
from tools.validation.sleep_syscall import slot_offset


def check(kernel):
    calls={}
    for number,name in ((14,'sys_mknod'),(19,'sys_lseek'),(54,'sys_ioctl')):
        entry=struct.unpack_from('<I',kernel.data,slot_offset(kernel,number))[0]
        require(entry==kernel.sym(name) and entry!=kernel.sym('sys_ni_syscall'),
                f'USB relay syscall {number} must have a real {name} implementation')
        symbols=kernel.elf.get_section_by_name('.symtab').get_symbol_by_name(name)
        require(any(s['st_info']['bind']=='STB_GLOBAL' and s['st_info']['type']=='STT_FUNC' and
                    s['st_size']>0 and isinstance(s['st_shndx'],int) for s in symbols),
                'USB relay syscall must not be a weak alias: '+name)
        calls[str(number)]={'target':name,'address':hex(entry)}
    names=('y2_usb_worker','y2_musb_init','y2_musb_interrupt','y2_session_write',
           'musb_start','musb_interrupt','musb_g_ep0_irq','gs_bind','acm_bind','gserial_connect')
    linked={name:hex(kernel.sym(name)) for name in names}
    require(not any(name in kernel.syms for name in
        ('musb_dma_controller_create','dma_controller_irq','musb_host_setup','mtk_musb_init')),
        'Unexpected DMA, host, or sibling MediaTek glue implementation')
    return {'syscalls':calls,'linked':linked,'status':'PASS offline; enumeration untested'}
