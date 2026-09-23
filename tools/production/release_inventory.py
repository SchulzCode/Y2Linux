#!/usr/bin/env python3
"""Emit source/package/footprint evidence from one built pair; never prune files."""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

PROJECT=Path(__file__).resolve().parents[2]
REBORN=PROJECT.parent/'Y2Reborn'

def digest(path,algorithm='sha256'):
    with path.open('rb') as stream:return hashlib.file_digest(stream,algorithm).hexdigest()


def inventory(project,reborn,build):
    versions=json.loads((build/'versions.json').read_text())
    for repo,key in ((project,'build_git_commit'),(reborn,'reborn_source_commit')):
        actual=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
        if actual!=versions[key]:raise ValueError('build/source pair mismatch')
    brlock=json.loads((project/'buildroot/inputs.lock.json').read_text())
    source=project/'.cache/sources'/('buildroot-'+brlock['version'])
    information=json.loads(subprocess.check_output(['make','-s','-C',str(source),
        'O='+str(build/'buildroot'),'BR2_EXTERNAL='+str(project/'buildroot'),'show-info'],timeout=60))
    packages={};missing=[]
    for name,item in sorted(information.items()):
        if item.get('virtual'):continue
        value={k:item.get(k) for k in ('type','version','licenses','license_files','redistributable','dependencies','install_target')}
        value['downloads']=[];hashes={};patches=[]
        for hashfile in item.get('hashes',[]):
            path=Path(hashfile);path=path if path.is_absolute() else source/path
            for line in path.read_text().splitlines():
                parts=line.split()
                if len(parts)==3 and parts[0] in ('sha256','sha512'):
                    hashes.setdefault(parts[2],[]).append((parts[0],parts[1]))
        local=any(u.startswith('local+') for d in item.get('downloads',[]) for u in d.get('uris',[]))
        for download in item.get('downloads',[]):
            if local:continue
            filename=download['source'];expected=hashes.get(filename,[])
            archive=project/'.cache/buildroot-dl'/item['dl_dir']/filename
            # Some virtual/generated packages have no archive. Do not invent a hash.
            record=dict(download,expected_hashes=dict(expected),cached=archive.is_file())
            if archive.is_file():
                record.update(bytes=archive.stat().st_size,sha256=digest(archive))
                for algorithm,wanted in expected:
                    if digest(archive,algorithm)!=wanted:raise ValueError('source archive hash mismatch: '+name)
            if not expected:missing.append(name+':'+filename)
            value['downloads'].append(record)
        for namepath in item.get('patches',[]):
            path=Path(namepath);path=path if path.is_absolute() else source/path
            patches.append({'path':namepath,'bytes':path.stat().st_size,'sha256':digest(path)})
        value['patches']=patches
        if local:value['source']='paired repository commit; not a public Buildroot mirror archive'
        packages[name]=value
    if missing:raise ValueError('unhashed selected remote sources: '+','.join(missing))
    files=[];top=collections.Counter();library_names=set();needed=set();elf_count=0;setids=[]
    target=build/'buildroot/target'
    for path in sorted(target.rglob('*')):
        if path.is_symlink() or not path.is_file():continue
        relative=path.relative_to(target).as_posix();size=path.stat().st_size
        files.append({'path':relative,'bytes':size,'sha256':digest(path)})
        top['/'.join(relative.split('/')[:2])]+=size
        if path.stat().st_mode & 0o6000:setids.append(relative)
        with path.open('rb') as stream:iself=stream.read(4)==b'\x7fELF'
        if not iself:continue
        elf_count+=1
        dynamic=subprocess.check_output(['readelf','-d',str(path)],stderr=subprocess.DEVNULL,text=True)
        needed.update(re.findall(r'\(NEEDED\).*?\[(.*?)\]',dynamic))
        library_names.update(re.findall(r'\(SONAME\).*?\[(.*?)\]',dynamic))
    # Executables loaded by service scripts and dlopen/plugin families are not
    # inferred unused from DT_NEEDED absence. This is only a review inventory.
    host={}
    for name,args in [('cc',['cc','--version']),('make',['make','--version']),('python',[sys.executable,'--version']),
                      ('rustc',['rustc','+1.90.0','-Vv']),('cargo',['cargo','+1.90.0','-V'])]:
        host[name]=subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=10).strip()
    inputs={}
    for repo,prefix,names in ((project,'Y2Linux',('tools/build/inputs.lock.json','tools/build/connectivity-host.lock.json',
          'tools/build/rust-toolchain.lock.json','buildroot/inputs.lock.json','kernel/patches/manifest.json',
          'tools/production/modernize_buildroot.py','tools/production/ffmpeg9.py','tools/production/release.json')),
          (reborn,'Y2Reborn',('Cargo.lock','rust-toolchain.toml','.cargo/config.toml','FFMPEG_VERSION','docs/architecture/dependencies.json'))):
        for name in names:inputs[prefix+'/'+name]={'sha256':digest(repo/name),'bytes':(repo/name).stat().st_size}
    checksums=sorted(reborn.glob('vendor/*/.cargo-checksum.json'))
    vendor=hashlib.sha256()
    for path in checksums:vendor.update(str(path.relative_to(reborn)).encode()+b'\0'+path.read_bytes())
    return {'schema':'org.y2linux.release-inventory/v1','versions':versions,'inputs':inputs,'packages':packages,
            'rust_vendor':{'packages':len(checksums),'checksum_manifest_set_sha256':vendor.hexdigest(),
                           'authority':'Cargo.lock plus checked-in vendor/.cargo-checksum.json; cargo --locked --offline'},
            'owner_provision':json.loads((build/'owner-firmware.json').read_text()),
            'host_tools':host,'byte_reproducibility_proven':False,
            'footprint':{'regular_file_bytes':sum(x['bytes'] for x in files),'regular_files':len(files),
                         'elf_files':elf_count,'largest_files':sorted(files,key=lambda x:x['bytes'],reverse=True)[:40],
                         'directory_bytes':dict(top),'setid_files':setids,
                         'sonames_without_direct_DT_NEEDED_reference':sorted(library_names-needed),
                         'unused_library_claim':False,'removed_packages':[],
                         'interpretation':'Logical regular bytes, not ext4 allocation; plugins and dynamic loading need runtime proof before removal'},
            'files':files}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--build',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();value=inventory(PROJECT,REBORN,args.build.resolve())
    with args.output.open('x') as stream:json.dump(value,stream,indent=2);stream.write('\n')
    print(json.dumps({'packages':len(value['packages']),'regular_file_bytes':value['footprint']['regular_file_bytes'],
                      'inventory_sha256':digest(args.output),'byte_reproducibility_proven':False}))

if __name__=='__main__':main()
