"""Independent parser for the single reviewed legacy Android/MTK boot format."""
import hashlib
import struct
from tools.validation.d08 import require, up

PAGE = 2048
MAGIC = 0x58881688
NAME = b'Y2Linux-M1'.ljust(16, b'\0')


def check(data, kernel, ramdisk, layout):
    require(2048 <= len(data) <= 0x1000000, 'BOOTIMG file/partition size')
    fields = struct.unpack_from('<8s10I16s', data)
    k, r = len(kernel) + 512, len(ramdisk) + 512
    require(fields == (b'ANDROID!',k,0x10008000,r,0x11000000,0,0x10f00000,
                       0x10000100,PAGE,0,0,NAME), 'legacy Android header fields')
    require(not any(data[64:576]) and not any(data[596:PAGE]), 'Android commandline/ID/header padding')
    ko, ro = PAGE, PAGE + up(k,PAGE)
    def unwrap(offset, length, label, payload):
        require(offset+length <= len(data), 'MTK wrapper bounds')
        magic, size, name = struct.unpack_from('<II32s',data,offset)
        require((magic,size,name) == (MAGIC,len(payload),label.ljust(32,b'\0')), 'MTK wrapper magic/size/name')
        require(data[offset+40:offset+512] == b'\xff'*472, 'MTK wrapper padding')
        require(data[offset+512:offset+length] == payload, 'MTK payload bytes')
    unwrap(ko,k,b'KERNEL',kernel); unwrap(ro,r,b'ROOTFS',ramdisk)
    require(not any(data[ko+k:ro]), 'kernel page padding')
    end = PAGE + up(k,PAGE) + up(r,PAGE) + 2*PAGE
    require(len(data) == end == layout['bootimg_read_end'], 'LK complete overread/file extent')
    require(not any(data[ro+r:]), 'ramdisk page/overread padding')
    sha = hashlib.sha1()
    for offset, size in [(ko,k),(ro,r),(0,0)]:
        sha.update(data[offset:offset+size]); sha.update(struct.pack('<I',size))
    require(data[576:596] == sha.digest(), 'Android SHA-1 ID')
    return {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),
            'android_id_sha1':sha.hexdigest(),'partition_bytes':0x1000000,
            'kernel_wrapper_offset':ko,'ramdisk_wrapper_offset':ro,
            'lk_complete_read_end':end,'zero_filled_read_tail_bytes':end-(ro+r),
            'authentication':'unsigned image; offline checks do not establish this candidate hardware acceptance'}
