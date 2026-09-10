"""DEV-01 large-bank successor; D08 relocation checks remain the boot constraint."""
from tools.validation.d08 import validate, require, overlaps
RAM=((0x80000000,0xbe000000),)
RESERVED=((0x80000000,0x80004000),(0x81800000,0x84000000),(0xbdf00000,0xc0000000))

def check_layout(inputs):
    result=validate(inputs,initramfs_cap=0x400000,regular_file_cap=0x800000)
    result.update(policy='DEV-01',ram=RAM,reserved=RESERVED)
    for name in ('image','resident_kernel','relocated_copy','relocated_dtb','compressed_bss','malloc','initramfs','lk_staging'):
        region=result[name]
        require(any(a<=region[0]<region[1]<=b for a,b in RAM),name+' outside physical bank')
        require(not any(overlaps(region,r) for r in RESERVED),name+' overlaps reservation')
    excluded=sum(max(0,min(b,y)-max(a,x)) for a,b in RAM for x,y in RESERVED)
    result['allocator_before_kernel_bytes']=sum(b-a for a,b in RAM)-excluded
    require(result['allocator_before_kernel_bytes']==951*1024*1024-16384,'unexpected practical bank size')
    return result
