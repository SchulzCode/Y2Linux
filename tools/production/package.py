#!/usr/bin/env python3
"""Create an owner-personalized SPFT candidate from built files; no device access."""
import argparse, json, os, shutil, subprocess, sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.layout import TARGETS, STOCK_SCATTER_SHA256, digest, require, make_scatter, sparse_encode, sparse_identity, addressing_contract, make_readback_plan
from tools.production.owner_data import public_key, validate_seed
PROJECT=Path(__file__).resolve().parents[2]
BUILDROOT_SOURCE=PROJECT/'.cache/sources' / ('buildroot-' + json.loads((PROJECT/'buildroot/inputs.lock.json').read_text())['version'])
STOCK=Path('/home/luca/Dokumente/Code/Y2Player/y2_v3.2.0_FM-20260813')
RESTORE={'boot.img':'5ef1bdf28481ee0bf5f3528c1ddd91cf3f4d2d5f39e4d0ea049a8137a30f6af6','system.img':'5a7a92f3a95374f31abe8b3ddbd68c5c2ce5677547db3d1cbc21653d24c76989','userdata.img':'552e325ecf2faffeb9351022147bbfda3b7cebe25df491b2d732e85c5c68f27c'}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--build',type=Path,default=PROJECT/'out/y2linux-production-build-v1-r6');p.add_argument('--output',type=Path,default=PROJECT/'out/y2linux-production-v1-r6');p.add_argument('--public-key',type=Path,default=Path.home()/'.ssh/y2linux_ed25519.pub',help='existing owner public key; never generates an identity');p.add_argument('--data-template',type=Path,help='reuse a verified layout-1 data template unchanged');a=p.parse_args()
    out=a.output.resolve();require(out.is_relative_to(PROJECT/'out') and not out.exists(),'fresh package directory required')
    key=public_key(a.public_key)
    subprocess.run(['ssh-keygen','-lf',str(a.public_key)],check=True)
    versions=json.loads((a.build/'versions.json').read_text());head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip()
    require(versions['build_git_commit']==head and (a.build/'kernel-source-commit').read_text().strip()==head,'build commit mismatch')
    require(not subprocess.check_output(['git','status','--porcelain'],cwd=PROJECT,text=True).strip(),'release packaging requires committed source')
    out.mkdir();(out/'metadata').mkdir()
    for name,expected in RESTORE.items():require(digest(STOCK/name)==expected,'restore source changed: '+name)
    stock=(STOCK/'MT6582_Android_scatter.txt').read_text();require(digest(STOCK/'MT6582_Android_scatter.txt')==STOCK_SCATTER_SHA256,'scatter identity')
    # Keep host sparse allocation while copying the raw ext4 image.
    subprocess.run(['cp','--sparse=always',str((a.build/'buildroot/images/rootfs.ext4').resolve()),str(out/'Y2ROOT.img')],check=True)
    shutil.copyfile(a.build/'BOOTIMG.img',out/'BOOTIMG.img')
    if a.data_template:
        template=a.data_template.resolve()
        require(template.is_file() and template.stat().st_size==TARGETS['USRDATA']['size'],'data template bounds')
        for field,expected in [('TYPE','ext4'),('LABEL','Y2DATA'),('UUID',TARGETS['USRDATA']['uuid'])]:
            require(subprocess.check_output(['blkid','-p','-s',field,'-o','value',str(template)],text=True).strip()==expected,'data template '+field)
        def contents(name):
            return subprocess.check_output(['debugfs','-R','cat '+name,str(template)],stderr=subprocess.DEVNULL)
        require(contents('/.y2data-schema')==b'1\n','data template schema')
        require(contents('/ssh/authorized_keys.d/authorized_keys')==key,'data template authorization')
        # Only an unused initialization seed may be redistributed. A live data
        # backup can contain private host keys, databases and device identities.
        empty={'settings','apps','y2player','logs','cache','updates','lost+found','ssh/host-keys'}
        expected={'': {'lost+found','.y2data-schema','settings','apps','y2player','logs','cache','updates','ssh'},
                  'ssh': {'authorized_keys.d','host-keys'},
                  'ssh/authorized_keys.d': {'authorized_keys'}}
        expected.update({name:set() for name in empty})
        for directory,names in expected.items():
            listing=subprocess.check_output(['debugfs','-R','ls -p /'+directory,str(template)],stderr=subprocess.DEVNULL,text=True)
            found=set()
            for line in listing.splitlines():
                if not line:continue
                fields=line.split('/')
                require(len(fields)>=7,'data template directory record')
                if fields[1]=='0' or fields[5] in ('.','..'):continue
                require(fields[3:5]==['0','0'] and fields[2].startswith(('040','100')),'data template type/owner')
                found.add(fields[5])
            require(found==names,'data template contains unexpected state: /'+directory)

        subprocess.run(['e2fsck','-fn',str(template)],check=True,stdout=subprocess.DEVNULL)
        subprocess.run(['cp','--sparse=always',str(template),str(out/'Y2DATA.img')],check=True)
        require(digest(out/'Y2DATA.img')==digest(template),'unchanged data template hash')
    else:
        seed=a.build/('data-seed-'+out.name);require(not seed.exists(),'fresh data seed required')
        seed.mkdir();(seed/'.y2data-schema').write_text('1\n')
        for directory in ('settings','apps','y2player','logs','cache','updates','ssh/authorized_keys.d','ssh/host-keys'):(seed/directory).mkdir(parents=True,exist_ok=True)
        for directory in ('ssh','ssh/authorized_keys.d','ssh/host-keys'):(seed/directory).chmod(0o700)
        (seed/'ssh/authorized_keys.d/authorized_keys').write_bytes(key);(seed/'ssh/authorized_keys.d/authorized_keys').chmod(0o600)
        # fakeroot applies root ownership to the filesystem without host privilege.
        fakeroot=a.build/'buildroot/host/bin/fakeroot';mkfs=a.build/'buildroot/host/sbin/mkfs.ext4'
        image=out/'Y2DATA.img'
        with image.open('wb') as f:f.truncate(TARGETS['USRDATA']['size'])
        import shlex
        script=a.build/'make-data.sh'
        script.write_text('#!/bin/sh\nset -eu\nchown -R 0:0 '+shlex.quote(str(seed))+'\n'+shlex.join([str(mkfs),'-q','-F','-L','Y2DATA','-U',TARGETS['USRDATA']['uuid'],'-O','^64bit','-E','lazy_itable_init=0,lazy_journal_init=0,hash_seed='+TARGETS['USRDATA']['uuid'],'-d',str(seed),str(image)])+'\n')
        env=os.environ.copy();env['E2FSPROGS_FAKE_TIME']='1787518700';env['FAKEROOTDONTTRYCHOWN']='1'
        subprocess.run([str(fakeroot),'--','sh',str(script)],env=env,check=True)
    # The key in the finished filesystem must match the selected existing .pub,
    # including on first initialization. Never publish an unchecked template.
    validate_seed(out/'Y2DATA.img',key)
    payloads=[]
    for name,target in TARGETS.items():
        raw=out/target['file'];entry={'component':target['type'],'target_partition':name,'region':'EMMC_USER','absolute_start_bytes':target['start'],'scatter_linear_start_bytes':target['linear'],'maximum_size_bytes':target['size'],'partition_relative_offset_bytes':0,'mandatory':name!='USRDATA','preserves_existing_data':False,'install_policy':'initialize-only; NEVER select during preserving reinstall or OTA' if name=='USRDATA' else 'replace','version':versions['kernel_version' if name=='BOOTIMG' else 'rootfs_version' if name=='ANDROID' else 'data_schema_version'],'raw':{'file':raw.name,'size_bytes':raw.stat().st_size,'sha256':digest(raw)}}
        require(raw.stat().st_size<=target['size'],'image bounds')
        entry['required_in_profiles']=['first-install'] if name=='USRDATA' else ['first-install','preserve-data']
        entry['requires']={'layout_version':1,'data_schema_version':1}
        if name!='USRDATA':entry['requires']['platform_contract']='y2-platform-v1'
        if name=='BOOTIMG':entry['modules']={'ownership':'BOOTIMG','release':versions['kernel_version'],'runtime_mount':'/lib/modules','ram_source':'/run/y2/modules'}
        if name!='BOOTIMG':
            # This exact MT6582 legacy DA rejects Android sparse FILL chunks.
            # Raw ext4 follows the ordinary byte-write path and defines all bytes,
            # including zero extents, without DONT_CARE/readback ambiguity.
            entry.update(filesystem={'type':'ext4','label':target['label'],'uuid':target['uuid']},spft={'format':'raw-ext4',**entry['raw']})
        else:entry['spft']={'file':raw.name,'format':'raw-android-mtk-bootimg',**{k:v for k,v in entry['raw'].items() if k!='file'}}
        payloads.append(entry)
    (out/'MT6582_Android_scatter.txt').write_text(make_scatter(stock,True))
    (out/'MT6582_preserve_data_scatter.txt').write_text(make_scatter(stock,False))
    restoration=[]
    for partition,filename in [('BOOTIMG','boot.img'),('ANDROID','system.img'),('USRDATA','userdata.img')]:
        entry={'target_partition':partition,'source':str(STOCK/filename),'size_bytes':(STOCK/filename).stat().st_size,'sha256':RESTORE[filename],'kind':'owner-proven FM factory source; not personalized Android backup'}
        if filename!='boot.img':
            identity=sparse_identity(STOCK/filename);entry['expanded_bytes']=identity['expanded_bytes'];entry['dont_care_policy']='stock holes are unspecified; no whole expanded readback hash asserted'
        restoration.append(entry)
    shutil.copyfile(PROJECT/'docs/architecture/production-partitions.json',out/'metadata/partitions.json')
    partition_map=json.loads((out/'metadata/partitions.json').read_text())
    plan=make_readback_plan(partition_map,PROJECT/'tests/fixtures/production')
    (out/'metadata/storage-addressing.json').write_text(json.dumps(addressing_contract(),indent=2)+'\n')
    (out/'metadata/readback-plan.json').write_text(json.dumps(plan,indent=2)+'\n')
    for name in ['layout.json','rescue-manifest.json','versions.json','kernel-source-commit']:
        shutil.copyfile(a.build/name,out/'metadata'/name)
    for source,dest in [('docs/knowledge/storage04-production-platform.md','metadata/storage04-production-platform.md'),('docs/architecture/production-install.md','install.md'),('docs/architecture/production-recovery.md','recovery.md'),('docs/architecture/update-model.md','ota-layout.md'),('docs/architecture/production-storage-v1.md','metadata/partition-audit.md')]:shutil.copyfile(PROJECT/source,out/dest)
    shutil.copyfile(PROJECT/'buildroot/inputs.lock.json',out/'metadata/buildroot-inputs.lock.json')
    shutil.copyfile(PROJECT/'tools/build/inputs.lock.json',out/'metadata/kernel-inputs.lock.json')
    shutil.copyfile(a.build/'kernel/.config',out/'metadata/kernel.config')
    shutil.copyfile(a.build/'buildroot/.config',out/'metadata/buildroot.config')
    shutil.copyfile(PROJECT/'.cache/sources/linux-6.18/COPYING',out/'metadata/kernel-COPYING')
    shutil.copyfile(BUILDROOT_SOURCE/'COPYING',out/'metadata/Buildroot-COPYING')
    (out/'metadata/source-notice.md').write_text('# Source and copyright\n\nCanonical source: https://github.com/SchulzCode/Y2Linux at manifest build_git_commit. Locked Linux6.18 and Buildroot inputs are recorded alongside this notice. Existing upstream/donor copyright and GPL notices remain in their source files; no source attribution has been removed. This is an owner-local manual test package, not a public distribution/legal qualification. Third-party package sources and full license collection must accompany a later public release. No device calibration or private key is included.\n')
    manifest={'schema':'org.schulzcode.y2linux.release/v1',**versions,'status':'offline-validated candidate; manual physical acceptance pending','hardware_compatibility':{'vendor':'Innioasis','board':'Y2 eastaeon82_wet_kk','soc':'MT6582','emmc_user_capacity_bytes':7784103936,'capacity_semantics':'stock Android exported layout bound','emmc_physical_user_capacity_bytes':7818182656,'accepted_linux_user_sector_counts':[15203328],'stock_scatter_sha256':STOCK_SCATTER_SHA256,'other_revisions':'unsupported until their layout and hardware are verified'},'payloads':payloads,'normal_update_allowlist':['BOOTIMG','ANDROID'],'data_policy':'USRDATA persists; data template only for explicit first initialization/factory reset','runtime_kernel_write_allowlist':['ANDROID','USRDATA'],'application':{'name':'Y2PlayerNative','version':None,'implemented':False,'builtin_path':'/usr/bin/y2player','future_payload_root':'/data/apps/y2player','state_root':'/data/y2player','partition':None},'minimum_compatible_components':{'layout_version':1,'data_schema_version':1,'rootfs_abi':'armv7-eabihf-glibc','kernel_contract':'y2-platform-v1','rootfs_contract':'y2-platform-v1'},'preserved_paths':['/data','external microSD'],'restoration_sources':restoration,'debug_access':{'scope':'owner-personalized first-install data image; NOT a universal production credential','authorized_public_key_sha256':digest(a.public_key),'private_key_packaged':False,'host_keys':'generated on device and persisted on Y2DATA'},'future_security':{'signature':None,'signing_key_id':None,'update_channel':None,'rollback_index':None},'profiles':{n:{'sha256':digest(out/n),'selected_partitions':parts} for n,parts in [('MT6582_Android_scatter.txt',['BOOTIMG','ANDROID','USRDATA']),('MT6582_preserve_data_scatter.txt',['BOOTIMG','ANDROID'])]}}
    manifest['storage_addressing']=addressing_contract()
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    (out/'SHA256SUMS').write_text(''.join(digest(f)+'  '+str(f.relative_to(out))+'\n' for f in sorted(out.rglob('*')) if f.is_file() and f.name!='SHA256SUMS'))
    print('Prepared:',out)
if __name__=='__main__':main()
