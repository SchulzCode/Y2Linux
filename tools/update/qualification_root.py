#!/usr/bin/env python3
"""Prepare an explicit local failed-application OTA fixture, never install or sign it.

Copies a hash-matched candidate root to a new regular file and removes Reborn.
The derived image is intentionally unusable as a player and only tests rescue
boot-health rollback. No block device, active root or original image is written.
"""
import argparse
import hashlib
import json
from pathlib import Path
import stat
import subprocess


def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()


def debugfs(image,command,write=False):
    result=subprocess.run(['debugfs',*(['-w'] if write else []),'-R',command,str(image)],
                          capture_output=True,text=True,check=True,timeout=20)
    return result.stdout+result.stderr


def prepare(candidate,output):
    candidate=Path(candidate).resolve();output=Path(output).absolute()
    manifest=json.loads((candidate/'manifest.json').read_text())
    roots=[p for p in manifest['payloads'] if p['target_partition']=='ANDROID']
    if len(roots)!=1 or roots[0]['raw']['file']!='Y2ROOT.img':raise ValueError('unique_candidate_root_required')
    source=candidate/'Y2ROOT.img';metadata=roots[0]['raw']
    if not stat.S_ISREG(source.lstat().st_mode) or source.stat().st_size!=metadata['size_bytes'] or sha(source)!=metadata['sha256']:
        raise ValueError('regular_candidate_image_hash_required')
    if any(c in str(output) for c in '\n\r\0"\\'):raise ValueError('simple_output_path_required')
    output.mkdir(mode=0o700,parents=True,exist_ok=False)
    image=output/'INTENTIONALLY-BROKEN-APP.ext4'
    subprocess.run(['cp','--sparse=always','--reflink=never','--',str(source),str(image)],check=True,timeout=120)
    versions=json.loads((candidate/'metadata/versions.json').read_text())
    versions.update(release_version=versions['release_version']+'.qualification-no-app',
                    rootfs_version=versions['rootfs_version']+'.qualification-no-app',
                    qualification_fault='Reborn executable deliberately absent; not a release image')
    versions_file=output/'fault-versions.json';versions_file.write_text(json.dumps(versions,indent=2)+'\n')
    if 'Type: regular' not in debugfs(image,'stat /usr/bin/reborn'):raise ValueError('source_app_not_regular')
    debugfs(image,'rm /usr/bin/reborn',True)
    debugfs(image,'rm /etc/y2linux/versions.json',True)
    debugfs(image,f'write "{versions_file}" /etc/y2linux/versions.json',True)
    if 'File not found' not in debugfs(image,'stat /usr/bin/reborn'):raise ValueError('fixture_app_removal_failed')
    loaded=debugfs(image,'cat /etc/y2linux/versions.json')
    if versions['rootfs_version'] not in loaded:raise ValueError('fixture_identity_write_failed')
    checked=subprocess.run(['e2fsck','-fn',str(image)],capture_output=True,text=True,timeout=60)
    (output/'e2fsck.log').write_text(checked.stdout+checked.stderr)
    if checked.returncode:raise ValueError('fixture_filesystem_not_clean')
    if sha(source)!=metadata['sha256']:raise ValueError('original_source_changed')
    receipt={'schema':'org.y2linux.update-qualification-fixture/v1','not_a_release':True,
             'fault':'application_missing','base_root_sha256':metadata['sha256'],
             'derived_root_sha256':sha(image),'derived_bytes':image.stat().st_size,
             'versions':versions,'signed':False,'device_accessed':False,
             'expected':'No application readiness; PendingHealth times out; next owner reboot restores verified previous root'}
    (output/'fixture.json').write_text(json.dumps(receipt,indent=2)+'\n')
    return receipt


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--candidate',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();print(json.dumps(prepare(args.candidate,args.output),indent=2))
