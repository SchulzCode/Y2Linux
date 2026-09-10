"""Validate DEV-01 emitted ARM code, rescue/DT memory ownership and BOOTIMG."""
import argparse, hashlib, io, json, stat, struct
from pathlib import Path
from elftools.elf.elffile import ELFFile
from tools.build.check_config import check as check_config, parse
from tools.validation.artifacts import Elf
from tools.validation.formats import gunzip
from tools.validation.dev_dtb import check as check_dtb
from tools.validation.d08 import Inputs, require
from tools.validation.dev_memory import check_layout
from tools.validation.sleep_syscall import check as check_sleep, slot_offset
from tools.validation.bootimg import check as check_bootimg
from tools.build.package import pack


def rescue_entries(raw):
    entries={};offset=0
    while True:
        require(raw[offset:offset+6]==b'070701','rescue newc signature')
        fields=[int(raw[i:i+8],16) for i in range(offset+6,offset+110,8)]
        ino,mode,uid,gid,links,mtime,size,dmaj,dmin,rmaj,rmin,namesize,crc=fields
        require(uid==gid==mtime==dmaj==dmin==crc==0,'rescue metadata')
        require(0<namesize<256 and size<=0x800000,'rescue entry bounds')
        offset+=110;name_raw=raw[offset:offset+namesize]
        require(name_raw.endswith(b'\0'),'rescue path terminator')
        name=name_raw[:-1].decode();offset=(offset+namesize+3)&~3
        require(name not in entries and not name.startswith('/') and '..' not in name.split('/'),'unsafe rescue path')
        data=raw[offset:offset+size];require(len(data)==size,'rescue truncated file');offset=(offset+size+3)&~3
        if name=='TRAILER!!!':
            require(size==0 and not any(raw[offset:]) and len(raw)==(offset+511)&~511,'rescue trailer')
            return entries
        require(stat.S_IFMT(mode) in (stat.S_IFDIR,stat.S_IFREG,stat.S_IFLNK,stat.S_IFCHR),'rescue file type')
        if stat.S_ISCHR(mode): require((name,rmaj,rmin) in [('dev/console',5,1),('dev/null',1,3)],'unexpected rescue device')
        entries[name]=(mode,data)


def check(root,project):
    check_config(root/'kernel/.config',project/'kernel/config/first-boot.config')
    config=parse(root/'kernel/.config')
    require(config['CONFIG_PAGE_OFFSET']=='0xC0000000' and config['CONFIG_HIGHMEM']=='y','ARM split/HIGHMEM')
    for key in ('CONFIG_VFP','CONFIG_NEON','CONFIG_EXT4_FS','CONFIG_MMC_BLOCK','CONFIG_USB_CDC_COMPOSITE','CONFIG_INET','CONFIG_UNIX98_PTYS'):
        require(config.get(key)=='y','missing development prerequisite '+key)
    kernel=Elf(root/'kernel/vmlinux');comp=Elf(root/'kernel/arch/arm/boot/compressed/vmlinux')
    image=(root/'kernel/arch/arm/boot/Image').read_bytes();z=(root/'kernel/arch/arm/boot/zImage').read_bytes()
    tree=(root/'y2.dtb').read_bytes();rd=(root/'initramfs.cpio.gz').read_bytes()
    text=kernel.sym('_text');require(text==0xc0008000 and kernel.elf['e_entry']==text,'kernel entry')
    require(kernel.binary(text,0xc00000)==image,'Image vs ELF')
    require(comp.sym('_start')==0 and comp.binary(0,0x600000)==z,'zImage vs ELF')
    sym=comp.sym;bss=kernel.sym('__bss_stop')-kernel.sym('__bss_start')
    require(sym('_kernel_bss_size')==bss,'compressed BSS contract')
    require(struct.unpack_from('<III',z,0x24)==(0x016f2818,0,len(z)),'zImage header')
    require(sym('_edata_real')==sym('_edata')==len(z) and len(z)%8==0,'appended DT alignment')
    require(struct.unpack_from('<I',z,sym('__piggy_size_addr'))[0]==len(image),'inflated length')
    require(gunzip(z[sym('input_data'):sym('input_data_end')],0xc00000)==image,'compressed kernel bytes')
    require(struct.unpack_from('<7I',z,sym('_table_start'))==(6,0x5a534c4b,sym('__piggy_size_addr'),bss,0x8000,0x10000,0),'relocation table')
    lc=sym('LC1');stack,edata=struct.unpack_from('<ii',z,lc);stack+=lc;edata+=lc
    require(edata==len(z),'LC1 input end')
    ss=comp.elf.get_section_by_name('.stack')
    require(ss is not None and ss['sh_size']==4096 and ss['sh_addr']+4096==stack,'decompressor stack')
    begin,end,continuation=(sym(n) for n in ('y2_wdt_stop_begin','y2_wdt_stop_end','y2_wdt_continue'))
    expected=struct.pack('<10I',0xe3070000,0xe3410000,0xe5901000,0xe3c11001,0xe3811422,0xe5801000,0xf57ff04f,0xe5901000,0xe3110001,0x1afffffe)
    require(0x30<begin<end<=continuation<sym('restart'),'early watchdog execution order')
    expected+=struct.pack('<I',0xea000000|(((continuation-(end-4)-8)//4)&0xffffff))
    require(z[begin:end]==expected,'early watchdog emitted instructions')
    entries=rescue_entries(gunzip(rd,0x810000))
    regular=sum(len(data) for mode,data in entries.values() if stat.S_ISREG(mode))
    require(entries['init']==(stat.S_IFREG|0o755,(project/'initramfs/rescue/init').read_bytes()),'rescue PID1 differs from reviewed source')
    require(entries['sbin/y2-observer'][1]==(root/'y2-observer').read_bytes(),'observer archive bytes')
    module=(root/'kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko').read_bytes()
    require(entries['display.ko'][1]==module==(root/'display.ko').read_bytes(),'exact DRM module packaging')
    elf=ELFFile(io.BytesIO(module));info=elf.get_section_by_name('.modinfo').data().split(b'\0')
    release=(root/'kernel/include/config/kernel.release').read_text().strip()
    require(b'depends=' in info and any(x.startswith(('vermagic='+release+' ').encode()) for x in info),'module ABI/dependencies')
    require((root/'kernel/modules.order').read_text().splitlines()==['drivers/gpu/drm/mediatek/mediatek-drm.o'],'unexpected modules')
    for name in ('bin/busybox','sbin/blkid','sbin/y2-observer','sbin/y2-fbtest','lib/ld-linux-armhf.so.3','lib/libc.so.6'):
        elf=ELFFile(io.BytesIO(entries[name][1]));require(elf.elfclass==32 and elf.little_endian and elf['e_machine']=='EM_ARM','rescue ARM ABI '+name)
    require(b'Y2LINUX-DEV-01' in entries['sbin/y2-observer'][1],'observer identity')
    args=Inputs(len(z),len(tree),len(rd),len(image),bss,kernel.sym('_end')-text,
        sym('restart'),sym('wont_overwrite'),sym('_edata'),sym('reloc_code_end'),stack,sym('__bss_start'),sym('_end'),regular)
    layout=check_layout(args);layout['dt']=check_dtb(tree,len(rd));layout['sleep_syscall']=check_sleep(kernel,root/'kernel/.config')
    for number in (11,14,19,21,54,114,120,192,221,240,241,281,282,284,379):
        entry=struct.unpack_from('<I',kernel.data,slot_offset(kernel,number))[0]
        require(entry!=kernel.sym('sys_ni_syscall'),'missing development syscall '+str(number))
    for name in ('y2_usb_worker','y2_musb_init','y2_musb_interrupt','musb_start','musb_g_disconnect','cdc_bind','ecm_bind','acm_bind','msdc_drv_probe','mtk_i2c_probe'):
        require(kernel.sym(name)>=text,'missing subsystem '+name)
    require(not any(name in kernel.syms for name in ('musb_dma_controller_create','dma_controller_irq','musb_host_setup','mtk_musb_init')),'unexpected USB DMA/host/glue')
    payload=z+tree+bytes(-len(tree)%8)
    layout['kernel_symbols']={n:kernel.sym(n) for n in ('_text','_edata','__bss_start','__bss_stop','_end')}
    layout['artifacts']={name:{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()} for name,raw in [('Image',image),('zImage',z),('y2.dtb',tree),('zImage-dtb',payload),('initramfs.cpio.gz',rd),('display.ko',module)]}
    layout['status']='PASS offline DEV-01; physical expanded RAM/display/wheel/SD/USB/SSH acceptance pending'
    return layout,payload,rd


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);p.add_argument('--package',action='store_true');a=p.parse_args()
    project=Path(__file__).resolve().parents[2];layout,payload,rd=check(a.root,project)
    if a.package:
        boot=pack(payload,rd);layout['bootimg']=check_bootimg(boot,payload,rd,layout)
        (a.root/'zImage-dtb').write_bytes(payload);(a.root/'BOOTIMG.img').write_bytes(boot)
    elif (a.root/'BOOTIMG.img').exists():layout['bootimg']=check_bootimg((a.root/'BOOTIMG.img').read_bytes(),payload,rd,layout)
    (a.root/'layout.json').write_text(json.dumps(layout,indent=2)+'\n');print(json.dumps(layout,indent=2))
if __name__=='__main__':main()
