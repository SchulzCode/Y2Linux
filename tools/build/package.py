"""Package only a freshly validated offline D08 artifact; never access a device."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
from tools.validation.artifacts import check as check_artifacts
from tools.validation.bootimg import check as check_package


def pack(kernel, ramdisk):
    def wrapper(name, data):
        return struct.pack('<II32s',0x58881688,len(data),name.ljust(32,b'\0')) + b'\xff'*472 + data
    k, r = wrapper(b'KERNEL',kernel), wrapper(b'ROOTFS',ramdisk)
    header = bytearray(2048)
    struct.pack_into('<8s10I16s',header,0,b'ANDROID!',len(k),0x10008000,len(r),0x11000000,
                     0,0x10f00000,0x10000100,2048,0,0,b'Y2Linux-M1'.ljust(16,b'\0'))
    identity = hashlib.sha1()
    for payload in (k,r,b''):
        identity.update(payload); identity.update(struct.pack('<I',len(payload)))
    header[576:596] = identity.digest()
    return bytes(header) + k + bytes(-len(k)%2048) + r + bytes(-len(r)%2048) + bytes(4096)


def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('root',type=Path)
    args = parser.parse_args(); project = Path(__file__).resolve().parents[2]
    layout,kernel = check_artifacts(args.root,project)
    if not (args.root/'zImage-dtb').exists(): raise ValueError('validated appended payload required')
    ramdisk = (args.root/'initramfs.cpio.gz').read_bytes()
    image = pack(kernel,ramdisk)
    result = check_package(image,kernel,ramdisk,layout)
    (args.root/'BOOTIMG.img').write_bytes(image)
    layout['bootimg'] = result
    (args.root/'layout.json').write_text(json.dumps(layout,indent=2)+'\n')
    print(json.dumps(result,indent=2))

if __name__ == '__main__': main()
