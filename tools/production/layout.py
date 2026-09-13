"""Production v1 address and image format contract. Never opens block devices."""
import hashlib, re, struct
from pathlib import Path

STOCK_SCATTER_SHA256='e5fe03e9f3219cc9b5ead27892f2ddb722acc16adce3306d27feb87893cd977e'
CAPACITY=7784103936 # Stock Android exported layout bound, NOT physical SEC_COUNT.
PHYSICAL_CAPACITY=7818182656 # Selected DA reports full native EMMC_USER capacity.
USER_OFFSET_SECTORS=23552 # Own FM driver: (20 MiB - 8.5 MiB non-user areas)/512.
DA_OTHER_BYTES=0x880000
# start = stock logical disk/scatter physical_start_addr, NOT native USER bytes.
TARGETS={
 'BOOTIMG': {'start':0x1d80000,'size':0x1000000,'linear':0x3180000,'file':'BOOTIMG.img','type':'kernel/boot'},
 'ANDROID': {'start':0x5180000,'size':0x33400000,'linear':0x6580000,'file':'Y2ROOT.img','type':'rootfs','label':'Y2ROOT','uuid':'79324c69-6e75-4801-8000-000000000101'},
 'USRDATA': {'start':0x40380000,'size':0x32000000,'linear':0x41780000,'file':'Y2DATA.img','type':'data-template','label':'Y2DATA','uuid':'79324c69-6e75-4801-8000-000000000102'},
}

def require(ok,message):
    if not ok: raise ValueError(message)

def digest(path):
    require(Path(path).is_file() and not Path(path).is_symlink(),'regular file required: '+str(path))
    with open(path,'rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def addressing_contract():
    """Explicit coordinates for the single production kernel, including old fields."""
    return {
        'schema':'org.schulzcode.y2linux.stock-emmc-window/v1',
        'logical_sector_bytes':512,
        'native_user_sectors':PHYSICAL_CAPACITY//512,
        'logical_disk_sectors':CAPACITY//512,
        'logical_to_native_user_offset_sectors':USER_OFFSET_SECTORS,
        'excluded_native_tail_sectors':(PHYSICAL_CAPACITY-CAPACITY)//512-USER_OFFSET_SECTORS,
        'required_ext_csd':{'boot_size_mult':32,'rpmb_size_mult':4,'gp_size_mult':[0]*12},
        'ext_csd_modified':False,
        'translated_commands':[17,18,24,25],
        'write_guard_address_space':'stock logical disk; before translation',
        'payload_absolute_start_bytes_semantics':'stock logical disk / scatter physical_start_addr; not native EMMC_USER',
        'legacy_da':{'region':'EMMC_PART_UNKNOWN(0x00)','address_type':'NUTL_ADDR_LOGICAL',
                     'non_user_prefix_bytes':DA_OTHER_BYTES,'logical_disk_origin_bytes':0x1400000},
        'targets':{n:{'logical_start_sector':t['start']//512,'length_sectors':t['size']//512,
                      'native_user_start_bytes':t['start']+USER_OFFSET_SECTORS*512,
                      'legacy_da_start_bytes':t['linear']} for n,t in TARGETS.items()},
    }

def make_readback_plan(classification, table_dir):
    """Coordinate reference only. Acquisitions require the stated DA mode."""
    def entry(name,start,size):
        return {'partition':name,'file':name+'.bin','region':'EMMC_USER',
                'physical_start_bytes':start+USER_OFFSET_SECTORS*512,
                'stock_logical_start_bytes':start,'legacy_global_da_start_bytes':start+0x1400000,
                'size_bytes':size}
    keep=[]
    for row in classification['partitions']:
        if row['classification']!='KEEP' or row['scatter_region']!='EMMC_USER':continue
        item=entry(row['name'],int(row['scatter_physical_bytes'],16),int(row['scatter_size_bytes'],16))
        if row['name'] in ('MBR','EBR1','EBR2'):
            table=table_dir/row['name']
            item.update(stock_prefix_size_bytes=table.stat().st_size,stock_prefix_sha256=digest(table))
        keep.append(item)
    return {'address_space':'physical_start_bytes are native EMMC_USER offsets, valid only when the backend explicitly selects EMMC_USER',
            'observed_spft_mode':'For the retained MT6582 legacy DA (NUTL_ADDR_LOGICAL / EMMC_PART_UNKNOWN=0), use legacy_global_da_start_bytes instead',
            'mode_control':'Verify a known BOOTIMG prefix and logged mode before interpreting any samples',
            'preservation_ranges':keep,
            'target_readbacks':[entry(n,t['start'],t['size']) for n,t in TARGETS.items()],
            'special_regions':'PRELOADER hardware regions and BMTPOOL are not included or accessed'}

def scatter_rows(text):
    rows=[]
    for block in re.split(r'(?m)^- partition_index: ',text)[1:]:
        row={'partition_index':block.splitlines()[0].strip()}
        row.update(re.findall(r'^  ([a-z_]+):\s*([^\n]+)',block,re.M))
        rows.append(row)
    require(len(rows)==21 and len({r['partition_name'] for r in rows})==21,'stock 21-entry map required')
    return rows

def make_scatter(stock, initialize_data):
    require(hashlib.sha256(stock.encode()).hexdigest()==STOCK_SCATTER_SHA256,'stock scatter hash')
    blocks=re.split(r'(?m)(?=^- partition_index: )',stock)
    for i in range(1,len(blocks)):
        name=re.search(r'partition_name: (\S+)',blocks[i]).group(1)
        filename=TARGETS[name]['file'] if name in TARGETS else 'NONE'
        enabled=name in ('BOOTIMG','ANDROID') or (name=='USRDATA' and initialize_data)
        blocks[i]=re.sub(r'(?m)^  file_name: .*$', '  file_name: '+filename,blocks[i])
        blocks[i]=re.sub(r'(?m)^  is_download: .*$', '  is_download: '+str(enabled).lower(),blocks[i])
    return ''.join(blocks)

def make_boot_scatter(stock):
    """Normal production kernel update: only BOOTIMG is a downloadable payload."""
    blocks=re.split(r'(?m)(?=^- partition_index: )',make_scatter(stock,False))
    for i in range(1,len(blocks)):
        if 'partition_name: BOOTIMG\n' in blocks[i]:continue
        blocks[i]=re.sub(r'(?m)^  file_name: .*$', '  file_name: NONE',blocks[i])
        blocks[i]=re.sub(r'(?m)^  is_download: .*$', '  is_download: false',blocks[i])
    return ''.join(blocks)

def make_data_scatter(stock):
    """Explicit data initialization only; never a preserving system update."""
    blocks=re.split(r'(?m)(?=^- partition_index: )',make_scatter(stock,True))
    for i in range(1,len(blocks)):
        if 'partition_name: USRDATA\n' in blocks[i]:continue
        blocks[i]=re.sub(r'(?m)^  file_name: .*$', '  file_name: NONE',blocks[i])
        blocks[i]=re.sub(r'(?m)^  is_download: .*$', '  is_download: false',blocks[i])
    return ''.join(blocks)

def sparse_encode(raw, destination):
    # Android sparse v1 with RAW/FILL only: no DONT_CARE holes left unverified.
    # Logical contents exactly match the raw filesystem, including zero blocks.
    size=raw.stat().st_size; require(size%4096==0,'sparse block alignment')
    chunks=(size+1048575)//1048576
    with raw.open('rb') as src,destination.open('wb') as dst:
        dst.write(struct.pack('<IHHHHIIII',0xed26ff3a,1,0,28,12,4096,size//4096,chunks,0))
        while data:=src.read(1048576):
            if not any(data):
                dst.write(struct.pack('<HHII',0xcac2,0,len(data)//4096,16)+bytes(4))
            else:
                dst.write(struct.pack('<HHII',0xcac1,0,len(data)//4096,12+len(data))+data)

def sparse_identity(path, strict=False):
    # Also inspects stock sparse sources; only generated payloads forbid holes.
    with path.open('rb') as f:
        head=f.read(28);require(len(head)==28,'sparse header')
        magic,major,minor,fh,ch,bs,blocks,n,crc=struct.unpack('<IHHHHIIII',head)
        require(magic==0xed26ff3a and major==1 and fh>=28 and ch>=12 and bs==4096,'sparse v1 geometry')
        require(blocks*bs<=CAPACITY and n<=blocks+1,'sparse bounds')
        f.read(fh-28);h=hashlib.sha256();count=0
        for _ in range(n):
            hdr=f.read(ch);require(len(hdr)==ch,'truncated sparse chunk')
            typ,reserved,b,total=struct.unpack('<HHII',hdr[:12]);left=b*bs
            require(total>=ch and count+left<=blocks*bs,'sparse chunk bounds')
            if typ==0xcac1:
                require(total==ch+left,'RAW size')
                while left:
                    data=f.read(min(left,1048576));require(data,'truncated RAW');h.update(data);left-=len(data)
            elif typ==0xcac2:
                require(total==ch+4,'FILL size');pattern=f.read(4);require(len(pattern)==4,'FILL bytes')
                data=pattern*(1048576//4)
                while left:h.update(data[:min(left,len(data))]);left-=min(left,len(data))
            elif typ==0xcac3 and not strict:
                require(total==ch,'DONT_CARE size')
                while left:h.update(bytes(min(left,1048576)));left-=min(left,1048576)
            elif typ==0xcac4 and not strict:
                require(b==0 and total==ch+4 and len(f.read(4))==4,'CRC chunk')
            else:raise ValueError('unapproved sparse chunk')
            count+=b*bs
        require(count==blocks*bs and not f.read(1),'sparse extent/trailer')
        return {'expanded_bytes':count,'expanded_sha256':h.hexdigest()}
