#!/usr/bin/env python3
"""Attach exact current build source/licenses/receipts to a validated owner package.

No device access or publishing. Archives contain tracked source at the recorded
build commits, never untracked owner files, firmware provisioning or host keys.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.production.layout import digest, require
from tools.production.validate import validate_manifest, validate_checksums

PROJECT = Path(__file__).resolve().parents[2]


def seal(package, build, reborn, logs):
    manifest = validate_manifest(package)
    require(not (package/'sources').exists(), 'fresh source attachment required')
    require(json.loads((build/'versions.json').read_text())['build_git_commit'] == manifest['build_git_commit'], 'build source identity')
    exits = json.loads((logs/'validation-exit-codes.json').read_text())
    require(exits and all(value == 0 for value in exits.values()), 'all integrated checks must pass')
    inventory = json.loads((build/'release-inventory.json').read_text())
    require(inventory['versions']['build_git_commit'] == manifest['build_git_commit'] and
            inventory['versions']['reborn_source_commit'] == manifest['reborn_source_commit'], 'inventory source pair')
    legal = build/'buildroot/legal-info'
    require((legal/'manifest.csv').is_file() and (legal/'README').is_file(), 'legal-info manifest and limitations required')
    source = package/'sources';source.mkdir()
    receipts = package/'validation';receipts.mkdir(exist_ok=True)
    for repo, label, commit in ((PROJECT,'Y2Linux',manifest['build_git_commit']),
                                (reborn,'Y2Reborn',manifest['reborn_source_commit'])):
        actual = subprocess.check_output(['git','rev-parse',commit+'^{commit}'],cwd=repo,text=True).strip()
        require(actual == commit, 'full source commit')
        subprocess.run(['git','archive','--format=tar.gz','--prefix='+label+'/',
                        '-o',str(source/(label+'.tar.gz')),commit],cwd=repo,check=True)
    for lockfile, filename in ((PROJECT/'tools/build/inputs.lock.json','linux-6.18.tar.xz'),
                                (PROJECT/'buildroot/inputs.lock.json',None)):
        lock = json.loads(lockfile.read_text())
        if filename is None: filename='buildroot-'+lock['version']+'.tar.xz'
        original = PROJECT/'.cache/downloads'/filename
        require(original.is_file(), 'locked source archive available:'+filename)
        expected = (next(item['sha256'] for item in lock['base_archives'] if item['name'] == filename)
                    if 'base_archives' in lock else lock['sha256'])
        require(digest(original) == expected, 'locked source archive hash:'+filename)
        shutil.copyfile(original, source/filename)
    shutil.copytree(legal, source/'Buildroot-legal-info')
    shutil.copyfile(build/'release-inventory.json', receipts/'release-inventory.json')
    shutil.copyfile(logs/'validation-exit-codes.json', receipts/'validation-exit-codes.json')
    for path in sorted(logs.glob('*.log')):
        shutil.copyfile(path, receipts/path.name)
    for name in ('kernel-build.log','buildroot-build.log','artifact-validation.log','ffmpeg-verify.log'):
        if (build/name).is_file(): shutil.copyfile(build/name,receipts/name)
    for name in ('Y2-COMMUNITY-TESTER-GUIDE.md','Y2-COMMUNITY-SOURCE-DELIVERY.md','Y2-COMMUNITY-BETA-IMPLEMENTATION-PASS.md'):
        src=PROJECT/'docs/release'/name
        shutil.copyfile(src,package/name)
    # Record every source/license byte, including collector warnings. Neither
    # a successful collector nor this inventory asserts legal clearance.
    source_manifest = {'schema':'org.y2linux.source-delivery/v1',
                       'source_pair':{'linux':manifest['build_git_commit'],'reborn':manifest['reborn_source_commit']},
                       'legal_review_complete':False,
                       'legal_info_limitations':'Buildroot-legal-info/README',
                       'files':[{'path':str(p.relative_to(source)),'bytes':p.stat().st_size,'sha256':digest(p)}
                                for p in sorted(source.rglob('*')) if p.is_file()]}
    (source/'manifest.json').write_text(json.dumps(source_manifest,indent=2)+'\n')
    (package/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(package))+'\n'
        for p in sorted(package.rglob('*')) if p.is_file() and p != package/'SHA256SUMS'))
    validate_checksums(package)
    return {'sources':len(source_manifest['files']),'validation':exits,'public_distribution_ready':False}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package',type=Path,required=True)
    parser.add_argument('--build',type=Path,required=True)
    parser.add_argument('--reborn',type=Path,default=PROJECT.parent/'Y2Reborn')
    parser.add_argument('--logs',type=Path,required=True)
    args=parser.parse_args()
    print(json.dumps(seal(args.package.resolve(),args.build.resolve(),args.reborn.resolve(),args.logs.resolve())))


if __name__=='__main__': main()
