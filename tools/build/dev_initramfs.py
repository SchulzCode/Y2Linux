"""Create the DEV rescue archive from built Buildroot binaries and exact module."""
import gzip, hashlib, io, json, posixpath, stat
from pathlib import Path
from elftools.elf.elffile import ELFFile
from tools.validation.d08 import require


def encode(entries):
    result=bytearray()
    ordered=sorted(entries.items(),key=lambda item:(item[0].count('/'),item[0]))
    ordered.append(('TRAILER!!!',(0,b'',0,0)))
    for ino,(name,(mode,data,major,minor)) in enumerate(ordered,1):
        raw=name.encode()+b'\0'
        fields=[ino,mode,0,0,2 if stat.S_ISDIR(mode) else 1,0,len(data),0,0,major,minor,len(raw),0]
        result+=b'070701'+''.join(f'{v:08x}' for v in fields).encode()+raw
        result+=bytes(-len(result)%4);result+=data;result+=bytes(-len(result)%4)
    result+=bytes(-len(result)%512)
    return bytes(result)


def build(root, project):
    target=root/'buildroot/target'; entries={}
    def put(name,mode,data=b'',major=0,minor=0):
        parent=posixpath.dirname(name)
        if parent and parent not in entries: put(parent,stat.S_IFDIR|0o755)
        entries[name]=(mode,data,major,minor)
    def binary(name, path):
        if name in entries:return
        raw=path.read_bytes();put(name,stat.S_IFREG|0o755,raw)
        elf=ELFFile(io.BytesIO(raw))
        require(elf.elfclass==32 and elf.little_endian and elf['e_machine']=='EM_ARM','rescue ELF architecture')
        for segment in elf.iter_segments():
            if segment['p_type']=='PT_INTERP':
                lib=segment.get_interp_name().lstrip('/');binary(lib,target/lib)
            if segment['p_type']=='PT_DYNAMIC':
                for tag in segment.iter_tags():
                    if tag.entry.d_tag!='DT_NEEDED':continue
                    candidates=[target/'lib'/tag.needed,target/'usr/lib'/tag.needed]
                    path=next((p for p in candidates if p.is_file()),None)
                    require(path is not None,'missing rescue library '+tag.needed)
                    # Flatten this known library symlink into its SONAME. No absolute host links.
                    binary(str(path.relative_to(target)),path)
    for name in ('.','dev','proc','sys','run','tmp','newroot','bin','sbin'):put(name,stat.S_IFDIR|0o755)
    put('dev/console',stat.S_IFCHR|0o600,major=5,minor=1)
    put('dev/null',stat.S_IFCHR|0o666,major=1,minor=3)
    put('init',stat.S_IFREG|0o755,(project/'initramfs/rescue/init').read_bytes())
    binary('bin/busybox',target/'bin/busybox')
    binary('sbin/blkid',target/'sbin/blkid')
    binary('sbin/y2-observer',root/'y2-observer')
    binary('sbin/y2-fbtest',root/'y2-fbtest')
    module=(root/'kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko').read_bytes()
    put('display.ko',stat.S_IFREG|0o400,module);(root/'display.ko').write_bytes(module)
    for applet in ('sh','mount','mkdir','mknod','sleep','readlink','chroot','umount','switch_root','kill'):
        put('bin/'+applet,stat.S_IFLNK|0o777,b'busybox')
    regular=sum(len(v[1]) for v in entries.values() if stat.S_ISREG(v[0]))
    require(regular<=0x800000,'DEV rescue regular-file budget')
    raw=encode(entries);packed=gzip.compress(raw,compresslevel=9,mtime=0)
    require(0<len(packed)<=0x400000,'DEV rescue compressed budget')
    (root/'initramfs.cpio').write_bytes(raw);(root/'initramfs.cpio.gz').write_bytes(packed)
    (root/'initrd-size.h').write_text(f'#define Y2_INITRD_END 0x{0x84000000+len(packed):08x}\n')
    manifest={k:{'mode':v[0],'bytes':len(v[1]),'sha256':hashlib.sha256(v[1]).hexdigest(),'rdev':[v[2],v[3]]} for k,v in entries.items()}
    (root/'rescue-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'DEV rescue: {regular} regular bytes; gzip {len(packed)} bytes; {len(entries)} entries')
    return entries

if __name__=='__main__':
    build(Path('/build'),Path('/project'))
