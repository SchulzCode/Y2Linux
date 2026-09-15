#!/usr/bin/env python3
"""Package BOOTIMG + Y2ROOT using the established preserving SPFT contract.

No device access and no Y2DATA payload, seed, formatting or migration image.
"""
import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.production.layout import TARGETS, digest, require, make_scatter
from tools.production.boot_update import installed_components


def preserving_scatter(stock):
    text = make_scatter(stock, False)
    # No stale data-image filename even on an unchecked row.
    return text.replace('file_name: Y2DATA.img', 'file_name: NONE')


def validate_preservation(out, manifest):
    from tools.connectivity.provision import DEFAULTS, INVENTORY
    previous = json.loads((out/'metadata/base-manifest.json').read_text())
    require(digest(out/'metadata/base-manifest.json') == manifest['base_manifest_sha256'], 'base identity')
    require(manifest['profiles'].keys() == {'MT6582_preserve_data_scatter.txt'}, 'only preserving update profile')
    stock = (PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    require((out/'MT6582_preserve_data_scatter.txt').read_text() == preserving_scatter(stock), 'exact preserving scatter')
    require(not (out/'Y2DATA.img').exists(), 'no data image in a system update')
    root = next(p for p in manifest['payloads'] if p['target_partition']=='ANDROID')
    boot = next(p for p in manifest['payloads'] if p['target_partition']=='BOOTIMG')
    versions = json.loads((out/'metadata/versions.json').read_text())
    require(all(manifest.get(k)==v for k,v in versions.items()), 'build version receipt')
    require(manifest['build_git_commit']==manifest['rootfs_build_git_commit']==
            (out/'metadata/kernel-source-commit').read_text().strip(), 'kernel/root source receipt')
    require(root['version']==manifest['rootfs_version'] and boot['version']==manifest['kernel_version'], 'component versions')
    data = next(p for p in installed_components(previous) if p['target_partition']=='USRDATA')
    require(manifest['installed_components'] == [root,data], 'new root plus unchanged data contract')
    require(manifest['data_schema_version'] == previous['data_schema_version'] == 1, 'no data schema reset')
    require(manifest['minimum_compatible_components'] == previous['minimum_compatible_components'], 'platform compatibility')
    oldroot = next(p for p in installed_components(previous) if p['target_partition']=='ANDROID')
    oldboot = next(p for p in previous['payloads'] if p['target_partition']=='BOOTIMG')
    for old in (oldboot, oldroot):
        path=out/'fallback'/old['raw']['file']
        require(path.stat().st_size == old['raw']['size_bytes'] and digest(path)==old['raw']['sha256'], 'fallback image identity')
    require(manifest['fallback']['images']==[
        {'file':'fallback/'+p['raw']['file'],'size_bytes':p['raw']['size_bytes'],'sha256':p['raw']['sha256']}
        for p in (oldboot,oldroot)], 'fallback identity receipt')
    require((out/'fallback/MT6582_preserve_data_scatter.txt').read_text()==preserving_scatter(stock), 'fallback preserves data')
    require(manifest['owner_firmware']['redistribution_permission_established'] is False, 'owner-only provisioning')
    files={x['filename']:{'size_bytes':x['bytes'],'sha256':x['sha256']}
           for x in json.loads(INVENTORY.read_text())['files']}
    files.update({name:{'size_bytes':v[0],'sha256':v[1]} for name,v in DEFAULTS.items()})
    require(manifest['owner_firmware']==json.loads((out/'metadata/owner-firmware.json').read_text())==
            {'schema':'org.schulzcode.y2linux.owner-firmware/v1',
             'redistribution_permission_established':False,'files':files}, 'reviewed firmware receipt')


def package(build, base, fallback_root, out):
    from tools.production.validate import validate_manifest, validate_rootfs
    require(out.is_relative_to(PROJECT/'out') and not out.exists(), 'fresh package inside out')
    require(not subprocess.check_output(['git','status','--porcelain'],cwd=PROJECT).strip(), 'commit reviewed source before packaging')
    head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=PROJECT,text=True).strip()
    versions=json.loads((build/'versions.json').read_text())
    require(versions['build_git_commit']==head==(build/'kernel-source-commit').read_text().strip(), 'source identity')
    previous=validate_manifest(base)
    components=installed_components(previous)
    oldroot=next(p for p in components if p['target_partition']=='ANDROID')
    require(fallback_root.stat().st_size==oldroot['raw']['size_bytes'] and digest(fallback_root)==oldroot['raw']['sha256'], 'accepted root fallback')
    out.mkdir(); (out/'metadata').mkdir(); (out/'fallback').mkdir()
    shutil.copyfile(build/'BOOTIMG.img',out/'BOOTIMG.img')
    subprocess.run(['cp','--sparse=always',str(build/'buildroot/images/rootfs.ext4'),str(out/'Y2ROOT.img')],check=True)
    shutil.copyfile(base/'BOOTIMG.img',out/'fallback/BOOTIMG.img')
    subprocess.run(['cp','--sparse=always',str(fallback_root),str(out/'fallback/Y2ROOT.img')],check=True)
    shutil.copyfile(base/'manifest.json',out/'metadata/base-manifest.json')
    for name in ('partitions.json','storage-addressing.json','readback-plan.json','kernel-COPYING','Buildroot-COPYING'):
        shutil.copyfile(base/'metadata'/name,out/'metadata'/name)
    for name in ('versions.json','layout.json','rescue-manifest.json','kernel-source-commit','owner-firmware.json'):
        shutil.copyfile(build/name,out/'metadata'/name)
    for source,name in [(build/'kernel/.config','kernel.config'),(build/'buildroot/.config','buildroot.config'),
                        (PROJECT/'tools/build/inputs.lock.json','kernel-inputs.lock.json'),
                        (PROJECT/'tools/build/connectivity-host.lock.json','connectivity-host.lock.json'),
                        (PROJECT/'buildroot/inputs.lock.json','buildroot-inputs.lock.json')]:
        shutil.copyfile(source,out/'metadata'/name)
    manifest=copy.deepcopy(previous); manifest.update(versions)
    for name in ('fallback','userspace_source','rootfs_build_git_commit','installed_components_policy'):
        manifest.pop(name,None)
    manifest['rootfs_build_git_commit']=head
    boot=copy.deepcopy(next(p for p in previous['payloads'] if p['target_partition']=='BOOTIMG'))
    root=copy.deepcopy(oldroot)
    for payload,version in ((boot,versions['kernel_version']),(root,versions['rootfs_version'])):
        name=payload['target_partition']; target=TARGETS[name]; path=out/target['file']
        require(path.stat().st_size<=target['size'], 'partition size ceiling')
        payload['version']=version; payload['required_in_profiles']=['preserve-data']
        payload['raw']={'file':path.name,'size_bytes':path.stat().st_size,'sha256':digest(path)}
        payload['spft']={'format':'raw-android-mtk-bootimg' if name=='BOOTIMG' else 'raw-ext4',**payload['raw']}
    boot['modules']['release']=versions['kernel_version']
    manifest['payloads']=[boot,root]
    manifest['installed_components']=[copy.deepcopy(root),copy.deepcopy(next(p for p in components if p['target_partition']=='USRDATA'))]
    manifest['installation_profile']='system-update'
    manifest['base_manifest_sha256']=digest(out/'metadata/base-manifest.json')
    manifest['status']='M5 integrated production candidate; physical radio qualification pending'
    manifest['data_policy']='Preserve existing Y2DATA in place. New private directories are created on first normal boot. No data payload.'
    manifest['owner_firmware']=json.loads((build/'owner-firmware.json').read_text())
    manifest['fallback']={'policy':'restore accepted POWER-03 BOOTIMG and previous Y2ROOT; preserve Y2DATA',
        'images':[{'file':'fallback/'+p['raw']['file'],'size_bytes':p['raw']['size_bytes'],'sha256':p['raw']['sha256']}
                  for p in (next(p for p in previous['payloads'] if p['target_partition']=='BOOTIMG'),oldroot)]}
    scatter='MT6582_preserve_data_scatter.txt'
    stock=(PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    (out/scatter).write_text(preserving_scatter(stock)); (out/'fallback'/scatter).write_text(preserving_scatter(stock))
    manifest['profiles']={scatter:{'sha256':digest(out/scatter),'selected_partitions':['BOOTIMG','ANDROID']}}
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    shutil.copyfile(PROJECT/'docs/knowledge/m5-connectivity-implementation.md',out/'install-and-qualification.md')
    (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(out))+'\n'
        for p in sorted(out.rglob('*')) if p.is_file() and p.name!='SHA256SUMS'))
    validate_manifest(out); validate_rootfs(out,build,manifest)
    print('PASS preserving production system update:',out)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build',type=Path,required=True)
    parser.add_argument('--base',type=Path,required=True)
    parser.add_argument('--fallback-rootfs',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    package(args.build.resolve(),args.base.resolve(),args.fallback_rootfs.resolve(),args.output.resolve())


if __name__=='__main__': main()
