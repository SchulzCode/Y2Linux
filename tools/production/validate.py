#!/usr/bin/env python3
"""One host-only validation of layout, transports, filesystems and release contract."""
import argparse, hashlib, json, posixpath, struct, subprocess, sys, tarfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.layout import (TARGETS, CAPACITY, digest, require, scatter_rows,
                                     sparse_identity, make_boot_scatter,
                                     addressing_contract, make_readback_plan)
PROJECT=Path(__file__).resolve().parents[2]

def run(*argv):
    return subprocess.run(argv,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE).stdout

def validate_addressing(out, m):
    # Retained historical packages describe the pre-correction port. Never
    # rewrite their manifests or mistake their capacity allowance for current IO.
    historical = m['kernel_version'] in ('6.18.0-y2linux-storage03',
                                        '6.18.0-y2linux-storage04', '6.18.0-y2linux-storage05')
    if historical and 'storage_addressing' not in m:
        return False
    require(m.get('storage_addressing')==addressing_contract(), 'stock logical/native addressing contract')
    require(json.loads((out/'metadata/storage-addressing.json').read_text())==addressing_contract(),
            'addressing metadata consistency')
    require(m['hardware_compatibility']['accepted_linux_user_sector_counts']==[CAPACITY//512],
            'current kernel exports the stock logical capacity only')
    classification=json.loads((out/'metadata/partitions.json').read_text())
    require(json.loads((out/'metadata/readback-plan.json').read_text())==
            make_readback_plan(classification,PROJECT/'tests/fixtures/production'),
            'readback coordinates/mode must match actual stock addressing')
    return True

def validate_manifest(out):
    m=json.loads((out/'manifest.json').read_text())
    if m.get('installation_profile')=='data-initialization-only':
        from tools.production.owner_data import validate_package
        return validate_package(out)
    classification=json.loads((out/'metadata/partitions.json').read_text())
    if m.get('installation_profile')=='boot-only':return validate_boot_update(out)
    require(m['schema']=='org.schulzcode.y2linux.release/v1' and m['layout_version']==1,'manifest/layout version')
    require(m['hardware_compatibility']['emmc_user_capacity_bytes']==CAPACITY,'layout capacity')
    mapped=validate_addressing(out,m)
    require(m['hardware_compatibility']['emmc_physical_user_capacity_bytes']==7818182656 and
            m['hardware_compatibility']['accepted_linux_user_sector_counts']==([15203328] if mapped else [15203328,15269888]),'physical vs exported capacity')
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
    system_update=m.get('installation_profile')=='system-update'
    expected_payloads={'BOOTIMG','ANDROID'} if system_update else set(TARGETS)
    require({p['target_partition'] for p in m['payloads']}==expected_payloads and len(m['payloads'])==len(expected_payloads),'exact payload allowlist')
    if system_update:
        from tools.production.system_update import validate_preservation
        validate_preservation(out,m)
    for payload in m['payloads']:
        name=payload['target_partition'];t=TARGETS[name]
        require(payload['absolute_start_bytes']==t['start'] and payload['maximum_size_bytes']==t['size'] and payload['scatter_linear_start_bytes']==t['linear'],'payload coordinates')
        require(payload['region']=='EMMC_USER' and payload['partition_relative_offset_bytes']==0,'address space')
        require(payload['requires']['layout_version']==1 and payload['requires']['data_schema_version']==1,'payload dependencies')
        if name!='USRDATA':require(payload['requires']['platform_contract']=='y2-platform-v1' and 'kernel_module_release' not in payload['requires'],'platform dependency contract')
        if name=='BOOTIMG':require(payload['modules']=={'ownership':'BOOTIMG','release':m['kernel_version'],'runtime_mount':'/lib/modules','ram_source':'/run/y2/modules'},'boot module ownership')
        require(payload['component']==t['type'] and payload['mandatory']==(name!='USRDATA'),'component/requirement')
        require(payload['preserves_existing_data'] is False,'images replace their contents')
        require(payload['raw']['file']==t['file'],'raw filename')
        for rep in ('raw','spft'):
            x=payload[rep];require(Path(x['file']).name==x['file'],'no file path traversal')
            file=out/x['file'];require(file.stat().st_size==x['size_bytes'] and digest(file)==x['sha256'],'payload identity '+file.name)
        require(payload['raw']['size_bytes']<=t['size'],'expanded payload size')
        if name!='BOOTIMG':
            require(payload['filesystem']=={'type':'ext4','label':t['label'],'uuid':t['uuid']},'filesystem contract')
            require(payload['spft']['format']=='raw-ext4','legacy DA requires raw ext4 transport')
            require(all(payload['spft'][k]==v for k,v in payload['raw'].items()),'raw ext4 transport identity')
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
            if name in expected_payloads:
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

def validate_boot_update(out, base=None):
    """Validate independently versioned BOOTIMG against retained installed images."""
    m=json.loads((out/'manifest.json').read_text())
    previous=json.loads((out/'metadata/base-manifest.json').read_text())
    require(m['schema']==previous['schema']=='org.schulzcode.y2linux.release/v1','release schema')
    require(m['installation_profile']=='boot-only' and len(m['payloads'])==1,'one boot payload')
    require(digest(out/'metadata/base-manifest.json')==m['base_manifest_sha256'],'base manifest hash')
    mapped=validate_addressing(out,m)
    expected_hardware={**previous['hardware_compatibility']}
    if mapped:expected_hardware['accepted_linux_user_sector_counts']=[CAPACITY//512]
    require(m['hardware_compatibility']==expected_hardware,'retained hardware; corrected stock disk view')
    for field in ('layout_version','data_schema_version','rootfs_version',
                  'normal_update_allowlist','runtime_kernel_write_allowlist','minimum_compatible_components',
                  'debug_access','application'):
        require(m[field]==previous[field],'retained production contract '+field)
    require(m['runtime_kernel_write_allowlist']==['ANDROID','USRDATA'] and
            m['minimum_compatible_components']['kernel_contract']=='y2-platform-v1','production write/module contract')
    require(m['rootfs_build_git_commit']==previous.get('rootfs_build_git_commit',previous['build_git_commit']),'independent root provenance')
    installed=(previous['installed_components'] if previous.get('installation_profile') in ('boot-only','system-update') else
               [p for p in previous['payloads'] if p['target_partition']!='BOOTIMG'])
    require(m['installed_components']==installed and {p['target_partition'] for p in installed}=={'ANDROID','USRDATA'},
            'unchanged root/data image references')
    for entry in installed:
        t=TARGETS[entry['target_partition']]
        require(entry['absolute_start_bytes']==t['start'] and entry['maximum_size_bytes']==t['size'] and
                entry['filesystem']=={'type':'ext4','label':t['label'],'uuid':t['uuid']},'retained filesystem geometry/identity')
        require(not (out/entry['raw']['file']).exists(),'root/data must not be update payloads')
    p=m['payloads'][0];t=TARGETS['BOOTIMG']
    require(p['target_partition']=='BOOTIMG' and p['region']=='EMMC_USER' and p['partition_relative_offset_bytes']==0,
            'BOOTIMG stock logical/scatter address space')
    require(p['absolute_start_bytes']==t['start'] and p['maximum_size_bytes']==t['size'] and
            p['scatter_linear_start_bytes']==t['linear'],'BOOTIMG coordinates')
    require(p['requires']=={'layout_version':1,'data_schema_version':1,'platform_contract':'y2-platform-v1'},'boot dependencies')
    require(p['modules']=={'ownership':'BOOTIMG','release':m['kernel_version'],
            'runtime_mount':'/lib/modules','ram_source':'/run/y2/modules'},'boot module contract')
    require(p['version']==m['kernel_version'] and p['required_in_profiles']==['boot-only'],'boot component version/profile')
    require(p['raw']=={'file':'BOOTIMG.img','size_bytes':(out/'BOOTIMG.img').stat().st_size,
            'sha256':digest(out/'BOOTIMG.img')},'BOOTIMG identity')
    require(p['spft']=={'format':'raw-android-mtk-bootimg',**p['raw']} and p['raw']['size_bytes']<=t['size'],'BOOTIMG transport/bounds')
    layout=json.loads((out/'metadata/layout.json').read_text())
    require(layout['bootimg']['sha256']==p['raw']['sha256'] and layout['bootimg']['bytes']==p['raw']['size_bytes'],
            'artifact validation identity')
    # Reparse the emitted Android/MTK envelopes, including LK's bounded read tail.
    from tools.validation.bootimg import check as check_bootimg
    image=(out/'BOOTIMG.img').read_bytes()
    ksize=struct.unpack_from('<I',image,8)[0];rsize=struct.unpack_from('<I',image,16)[0]
    ro=2048+(ksize+2047)//2048*2048
    require(ksize>=512 and rsize>=512 and ro+rsize<=len(image),'wrapper extents')
    check_bootimg(image,image[2560:2048+ksize],image[ro+512:ro+rsize],layout)
    versions=json.loads((out/'metadata/versions.json').read_text())
    require(all(m.get(k)==v for k,v in versions.items()),'manifest component versions')
    require((out/'metadata/kernel-source-commit').read_text().strip()==m['build_git_commit'] and
            len(m['build_git_commit'])==40,'kernel source commit')
    name='MT6582_BOOTIMG_only_scatter.txt'
    expected=make_boot_scatter((PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text())
    require((out/name).read_text()==expected,'exact stock geometry and BOOTIMG-only selection')
    require(m['profiles']=={name:{'sha256':digest(out/name),'selected_partitions':['BOOTIMG']}},'only boot update profile')
    require(digest(out/'metadata/partitions.json')==digest(PROJECT/'docs/architecture/production-partitions.json'),
            'unchanged partition classification')
    fallback=m['fallback'];oldboot=next(p for p in previous['payloads'] if p['target_partition']=='BOOTIMG')
    fallback_name='fallback/BOOTIMG-storage04.img'
    if 'manifest_sha256' in fallback:
        fm=json.loads((out/'metadata/fallback-manifest.json').read_text())
        require(digest(out/'metadata/fallback-manifest.json')==fallback['manifest_sha256'],'fallback manifest identity')
        for field in ('rootfs_version','layout_version','data_schema_version','minimum_compatible_components'):
            require(fm[field]==m[field],'fallback compatibility '+field)
        oldboot=next(p for p in fm['payloads'] if p['target_partition']=='BOOTIMG')
        require(fallback['kernel_version']==fm['kernel_version'],'fallback kernel version')
        fallback_name='fallback/BOOTIMG-previous.img'
    require(fallback['file']==fallback_name and
            fallback['sha256']==oldboot['raw']['sha256']==digest(out/fallback['file']) and
            fallback['size_bytes']==oldboot['raw']['size_bytes'],'observed fallback identity')
    receipt=json.loads((out/'metadata/userspace-source.json').read_text())
    require(receipt['base_manifest_sha256']==m['base_manifest_sha256'] and
            receipt['retained_images']=={p['target_partition']:p['raw'] for p in installed},'userspace reuse receipt')
    listed={}
    for line in (out/'SHA256SUMS').read_text().splitlines():
        sha,name=line.split('  ',1)
        require(name not in listed and not Path(name).is_absolute() and '..' not in Path(name).parts,'checksum path')
        require(digest(out/name)==sha,'package checksum '+name);listed[name]=sha
    require(set(listed)=={str(p.relative_to(out)) for p in out.rglob('*') if p.is_file() and p.name!='SHA256SUMS'},'checksum inventory')
    if base:
        require(digest(base/'manifest.json')==m['base_manifest_sha256'],'retained installed package identity')
        # A prior BOOTIMG-only package intentionally has no ext4 payloads.
        # Validate that package's complete reference contract; the equality
        # check above carries its original root/data identities unchanged.
        if previous.get('installation_profile') == 'boot-only':
            validate_manifest(base)
    if base and previous.get('installation_profile') != 'boot-only':
        for entry in installed:
            image=base/entry['raw']['file']
            require(digest(image)==entry['raw']['sha256'] and image.stat().st_size==entry['raw']['size_bytes'],
                    'retained ext4 bytes unchanged')
        root=base/'Y2ROOT.img';data=base/'Y2DATA.img'
        for path,expected in [('/etc/y2linux/platform-contract',b'y2-platform-v1\n'),
                              ('/etc/y2linux/layout-version',b'1\n')]:
            require(run('debugfs','-R','cat '+path,str(root))==expected,'installed root contract '+path)
        root_versions=json.loads(run('debugfs','-R','cat /etc/y2linux/versions.json',str(root)))
        require(root_versions['rootfs_version']==m['rootfs_version'] and
                root_versions['build_git_commit']==m['rootfs_build_git_commit'],'on-image root provenance')
        require(run('debugfs','-R','cat /.y2data-schema',str(data))==b'1\n','installed data schema')
    print('PASS production BOOTIMG-only manifest, stock geometry, unchanged root/data, module ABI contract, bounds and hashes')
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
        dbus_uid=dbus_gid=None
        for row in read('etc/passwd').decode().splitlines():
            fields=row.split(':')
            if fields[0]=='dbus':dbus_uid,dbus_gid=map(int,fields[2:4])
        for name,x in members.items():
            require(not name.startswith(('root/.ssh/','etc/dropbear/')),'packaged SSH material')
            if x.isfile() and x.size<65536:require(b'PRIVATE KEY-----' not in read(name),'private key material')
            expected=(33,33) if name=='var/www' else (0,0)
            if dbus_gid is not None and name=='usr/libexec/dbus-daemon-launch-helper':expected=(0,dbus_gid)
            if dbus_uid is not None and name in ('run/dbus','var/run/dbus'):expected=(dbus_uid,dbus_gid)
            require((x.uid,x.gid)==expected,'rootfs ownership '+name)
        require(read('etc/y2linux/layout-version')==b'1\n','root layout marker')
        require(read('etc/y2linux/platform-contract')==b'y2-platform-v1\n','root platform contract')
        connectivity=m.get('installation_profile')=='system-update'
        require(read('etc/y2linux/build-id')==(b'Y2LINUX-M5-CONNECTIVITY-01\n' if connectivity else b'Y2LINUX-STORAGE-04\n'),'root build identity')
        require(json.loads(read('etc/y2linux/versions.json'))==json.loads((build/'versions.json').read_text()),'versions root/build')
        require(json.loads(read('etc/y2linux/versions.json'))['build_git_commit']==m['build_git_commit'],'manifest root commit')
        if connectivity:
            from tools.connectivity.provision import DEFAULTS, INVENTORY
            files={x['filename']:(x['bytes'],x['sha256']) for x in json.loads(INVENTORY.read_text())['files']}
            files.update(DEFAULTS)
            for name,(size,sha) in files.items():
                content=read('lib/firmware/mediatek/mt6582/'+name)
                require(len(content)==size and hashlib.sha256(content).hexdigest()==sha,'reviewed owner firmware '+name)
            for name in members:
                require(not name.startswith(('data/network/','data/bluetooth/','data/connectivity/','var/lib/bluetooth/','var/lib/bluealsa/')),'no mutable radio state in Y2ROOT')
            for name in ('usr/sbin/y2-factory','usr/sbin/y2-calibration','usr/sbin/y2-radio-activate',
                         'usr/sbin/y2-bt-reconnect','usr/sbin/y2-a2dp-check','usr/sbin/wpa_supplicant',
                         'usr/sbin/iw','usr/libexec/bluetooth/bluetoothd','usr/bin/bluealsa','usr/bin/dbus-daemon'):
                content=read(name)
                require(content[:6]==b'\x7fELF\x01\x01' and struct.unpack_from('<H',content,18)[0]==40,'connectivity ARM binary '+name)
        for name in ('bin/busybox','sbin/init','sbin/blkid','sbin/e2fsck','sbin/ip','usr/sbin/dropbear','usr/bin/aplay','usr/bin/amixer','usr/bin/evtest','usr/bin/strace'):
            raw=read(name);require(raw[:6]==b'\x7fELF\x01\x01' and struct.unpack_from('<H',raw,18)[0]==40,'ARM userspace '+name)
            require(struct.unpack_from('<I',raw,36)[0]&0x400,'hard-float '+name)
        require(b'-s -g -j -k -p 10.42.0.1:22' in read('etc/default/dropbear'),'key-only SSH')
        require(b'root:$6$y2linux$disabled:' in read('etc/shadow'),'password disabled')
        release=(build/'kernel/include/config/kernel.release').read_text().strip()
        require('display.ko' not in members and not any('.ko' in n and n.startswith('lib/modules/') for n in members),'kernel modules must be owned by BOOTIMG')
        for name in ('etc/y2linux/layout-version','etc/y2linux/versions.json','etc/y2linux/platform-contract','usr/sbin/y2-platform-start','usr/sbin/y2-status','etc/init.d/S02y2-data','usr/sbin/y2-media','usr/bin/aplay','etc/fstab'):
            require(run('debugfs','-R','cat /'+name,str(out/'Y2ROOT.img'))==read(name),'raw ext4/tar agreement '+name)
    if m.get('installation_profile')=='system-update':
        print('PASS new root contents; existing Y2DATA preserved without a data payload')
        return
    require(run('debugfs','-R','cat /.y2data-schema',str(out/'Y2DATA.img'))==b'1\n','data schema')
    key=run('debugfs','-R','cat /ssh/authorized_keys.d/authorized_keys',str(out/'Y2DATA.img'))
    require(key.startswith(b'ssh-ed25519 ') and hashlib.sha256(key).hexdigest()==m['debug_access']['authorized_public_key_sha256'],'owner-specific data public key')
    for path in ('/ssh','/ssh/authorized_keys.d','/ssh/host-keys'):
        info=run('debugfs','-R','stat '+path,str(out/'Y2DATA.img')).decode()
        require('0700' in info and 'User:     0' in info and 'Group:     0' in info,'data SSH permissions '+path)
    print('PASS root/data contents, module ABI, per-owner public key and persistent SSH paths')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('package',type=Path);p.add_argument('--build',type=Path,default=PROJECT/'out/y2linux-production-build-v1-r4');a=p.parse_args()
    m=validate_manifest(a.package)
    if m.get('installation_profile') not in ('boot-only','data-initialization-only'):validate_rootfs(a.package,a.build,m)
    print('PASS offline Production Storage v1; physical no-SD qualification PENDING')
if __name__=='__main__':main()
