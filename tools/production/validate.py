#!/usr/bin/env python3
"""One host-only validation of layout, transports, filesystems and release contract."""
import argparse, hashlib, json, posixpath, struct, subprocess, sys, tarfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.layout import TARGETS, CAPACITY, digest, require, scatter_rows, sparse_identity
PROJECT=Path(__file__).resolve().parents[2]

def run(*argv):
    return subprocess.run(argv,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE).stdout

def validate_manifest(out):
    m=json.loads((out/'manifest.json').read_text());classification=json.loads((out/'metadata/partitions.json').read_text())
    require(m['schema']=='org.schulzcode.y2linux.release/v1' and m['layout_version']==1,'manifest/layout version')
    require(m['hardware_compatibility']['emmc_user_capacity_bytes']==CAPACITY,'board capacity')
    require(m['hardware_compatibility']['soc']=='MT6582' and m['hardware_compatibility']['board']=='Y2 eastaeon82_wet_kk','board identity')
    require(m['normal_update_allowlist']==['BOOTIMG','ANDROID'],'OTA allowlist excludes data initialization')
    require(m['runtime_kernel_write_allowlist']==['ANDROID','USRDATA'],'runtime allowlist')
    require(m['minimum_compatible_components']['layout_version']==1 and m['minimum_compatible_components']['data_schema_version']==1,'minimum compatibility')
    require(m['application']['partition'] is None and not m['application']['implemented'],'no app firmware partition')
    require(not m['debug_access']['private_key_packaged'],'no packaged private key')
    require(len(m['build_git_commit'])==40 and all(c in '0123456789abcdef' for c in m['build_git_commit']),'build commit')
    baseline={r['name']:r for r in classification['partitions']}
    spans=[]
    for name,r in baseline.items():
        if r['scatter_region']!='EMMC_USER' or name in ('BMTPOOL','FAT'):continue
        start=int(r['scatter_physical_bytes'],16);size=int(r['scatter_size_bytes'],16)
        require(start%512==size%512==0 and start+size<=CAPACITY,'partition alignment/capacity')
        spans.append((start,start+size,name))
    spans.sort()
    for a,b in zip(spans,spans[1:]):require(a[1]<=b[0],'partition overlap')
    require({p['target_partition'] for p in m['payloads']}==set(TARGETS) and len(m['payloads'])==3,'exact payload allowlist')
    for payload in m['payloads']:
        name=payload['target_partition'];t=TARGETS[name]
        require(payload['absolute_start_bytes']==t['start'] and payload['maximum_size_bytes']==t['size'] and payload['scatter_linear_start_bytes']==t['linear'],'payload coordinates')
        require(payload['region']=='EMMC_USER' and payload['partition_relative_offset_bytes']==0,'address space')
        require(payload['requires']['layout_version']==1 and payload['requires']['data_schema_version']==1,'payload dependencies')
        if name!='USRDATA':require(payload['requires']['kernel_module_release']==m['kernel_version'],'module dependency contract')
        require(payload['component']==t['type'] and payload['mandatory']==(name!='USRDATA'),'component/requirement')
        require(payload['preserves_existing_data'] is False,'images replace their contents')
        require(payload['raw']['file']==t['file'],'raw filename')
        for rep in ('raw','spft'):
            x=payload[rep];require(Path(x['file']).name==x['file'],'no file path traversal')
            file=out/x['file'];require(file.stat().st_size==x['size_bytes'] and digest(file)==x['sha256'],'payload identity '+file.name)
        require(payload['raw']['size_bytes']<=t['size'],'expanded payload size')
        if name!='BOOTIMG':
            require(payload['filesystem']=={'type':'ext4','label':t['label'],'uuid':t['uuid']},'filesystem contract')
            expanded=sparse_identity(out/payload['spft']['file'],strict=True)
            require(expanded['expanded_bytes']==payload['raw']['size_bytes'] and expanded['expanded_sha256']==payload['raw']['sha256'],'transport expanded/raw equality')
            require(all(payload['spft'][k]==v for k,v in expanded.items()),'manifest expanded identity')
            identity=run('blkid','-p','-o','export',str(out/t['file'])).decode()
            for k,v in [('TYPE','ext4'),('LABEL',t['label']),('UUID',t['uuid'])]:require(k+'='+v+'\n' in identity,'on-image '+k)
            fsck=run('e2fsck','-f','-n',str(out/t['file'])).decode()
            print(t['file']+': clean ext4, matching label/UUID; '+fsck.splitlines()[-1])
        else:
            raw=(out/t['file']).read_bytes();require(raw[:8]==b'ANDROID!','BOOTIMG magic')
            require(payload['raw']=={k:payload['spft'][k] for k in ('file','size_bytes','sha256')},'BOOTIMG transport')
    for filename,profile in m['profiles'].items():
        require(filename in ('MT6582_Android_scatter.txt','MT6582_preserve_data_scatter.txt'),'profile name')
        require(digest(out/filename)==profile['sha256'],'profile hash')
        rows=scatter_rows((out/filename).read_text());selected=[]
        for row in rows:
            name=row['partition_name'];r=baseline[name]
            for field,key in [('linear_start_addr','scatter_linear_bytes'),('physical_start_addr','scatter_physical_bytes'),('partition_size','scatter_size_bytes')]:require(int(row[field],16)==int(r[key],16),'stock scatter coordinates changed '+name)
            require(row['region']==r['scatter_region'],'scatter region changed')
            if name in TARGETS:
                p=next(p for p in m['payloads'] if p['target_partition']==name)
                require(row['file_name']==p['spft']['file'],'scatter image mapping')
                require(row['type']==('NORMAL_ROM' if name=='BOOTIMG' else 'YAFFS_IMG'),'stock image type')
            else:require(row['file_name']=='NONE' and row['is_download']=='false','protected mapping '+name)
            if row['is_download']=='true':selected.append(name)
        expected=['BOOTIMG','ANDROID','USRDATA'] if filename=='MT6582_Android_scatter.txt' else ['BOOTIMG','ANDROID']
        require(selected==expected==profile['selected_partitions'],'selected rows')
    # SHA256SUMS covers every deliverable, including the manifest and both profiles.
    listed={}
    for line in (out/'SHA256SUMS').read_text().splitlines():
        sha,name=line.split('  ',1);require(name not in listed and not Path(name).is_absolute() and '..' not in Path(name).parts,'checksum path/duplicate')
        require(digest(out/name)==sha,'package checksum '+name);listed[name]=sha
    require(set(listed)=={str(f.relative_to(out)) for f in out.rglob('*') if f.is_file() and f.name!='SHA256SUMS'},'checksum inventory')
    return m

def validate_rootfs(out,build,m):
    with tarfile.open(build/'buildroot/images/rootfs.tar') as tar:
        members={x.name.removeprefix('./').rstrip('/'):x for x in tar.getmembers()}
        def read(name):
            for _ in range(16):
                require(name in members,'missing rootfs '+name);x=members[name]
                if not (x.issym() or x.islnk()):return tar.extractfile(x).read()
                name=posixpath.normpath(x.linkname.lstrip('/') if x.linkname.startswith('/') or x.islnk() else posixpath.join(posixpath.dirname(name),x.linkname)).removeprefix('./')
            raise ValueError('symlink loop')
        require(members['root/.ssh'].issym() and members['root/.ssh'].linkname=='/data/ssh/authorized_keys.d','persistent public authorization')
        require(members['etc/dropbear'].issym() and members['etc/dropbear'].linkname=='/data/ssh/host-keys','persistent generated host keys')
        for name,x in members.items():
            require(not name.startswith(('root/.ssh/','etc/dropbear/')),'packaged SSH material')
            if x.isfile() and x.size<65536:require(b'PRIVATE KEY-----' not in read(name),'private key material')
            require((x.uid,x.gid)==((33,33) if name=='var/www' else (0,0)),'rootfs ownership')
        require(read('etc/y2linux/layout-version')==b'1\n','root layout marker')
        require(read('etc/y2linux/build-id')==b'Y2LINUX-STORAGE-01\n','root build identity')
        require(json.loads(read('etc/y2linux/versions.json'))==json.loads((build/'versions.json').read_text()),'versions root/build')
        require(json.loads(read('etc/y2linux/versions.json'))['build_git_commit']==m['build_git_commit'],'manifest root commit')
        for name in ('bin/busybox','sbin/init','sbin/blkid','sbin/e2fsck','sbin/ip','usr/sbin/dropbear','usr/bin/aplay','usr/bin/amixer','usr/bin/evtest','usr/bin/strace'):
            raw=read(name);require(raw[:6]==b'\x7fELF\x01\x01' and struct.unpack_from('<H',raw,18)[0]==40,'ARM userspace '+name)
            require(struct.unpack_from('<I',raw,36)[0]&0x400,'hard-float '+name)
        require(b'-s -g -j -k -p 10.42.0.1:22' in read('etc/default/dropbear'),'key-only SSH')
        require(b'root:$6$y2linux$disabled:' in read('etc/shadow'),'password disabled')
        release=(build/'kernel/include/config/kernel.release').read_text().strip()
        require(read('display.ko')==(build/'display.ko').read_bytes(),'exact DRM module')
        require(read('lib/modules/'+release+'/kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko')==read('display.ko'),'system module version')
        require(read('lib/modules/'+release+'/modules.dep').startswith(b'kernel/drivers/gpu/drm/mediatek/mediatek-drm.ko:'),'module index')
        for name in ('etc/y2linux/layout-version','etc/y2linux/versions.json','display.ko','etc/init.d/S02y2-data','usr/sbin/y2-media','usr/bin/aplay','etc/fstab'):
            require(run('debugfs','-R','cat /'+name,str(out/'Y2ROOT.img'))==read(name),'raw ext4/tar agreement '+name)
    require(run('debugfs','-R','cat /.y2data-schema',str(out/'Y2DATA.img'))==b'1\n','data schema')
    key=run('debugfs','-R','cat /ssh/authorized_keys.d/authorized_keys',str(out/'Y2DATA.img'))
    require(key.startswith(b'ssh-ed25519 ') and hashlib.sha256(key).hexdigest()==m['debug_access']['authorized_public_key_sha256'],'owner-specific data public key')
    for path in ('/ssh','/ssh/authorized_keys.d','/ssh/host-keys'):
        info=run('debugfs','-R','stat '+path,str(out/'Y2DATA.img')).decode()
        require('0700' in info and 'User:     0' in info and 'Group:     0' in info,'data SSH permissions '+path)
    print('PASS root/data contents, module ABI, per-owner public key and persistent SSH paths')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('package',type=Path);p.add_argument('--build',type=Path,default=PROJECT/'out/y2linux-production-build-v1');a=p.parse_args()
    m=validate_manifest(a.package);validate_rootfs(a.package,a.build,m)
    print('PASS offline Production Storage v1; physical no-SD qualification PENDING')
if __name__=='__main__':main()
