#!/usr/bin/env python3
"""Generate an empty owner-personalized data seed, offline and separate from updates.

No maintainer data template, private key, device, calibration or firmware is read.
Only the owner's existing public key enters the initialization filesystem.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.production.layout import TARGETS, addressing_contract, digest, make_data_scatter, require
from tools.production.owner_data import public_key, validate_seed

PROJECT = Path(__file__).resolve().parents[2]
DIRECTORIES = ('settings', 'apps', 'y2player', 'logs', 'cache', 'updates',
               'ssh', 'ssh/authorized_keys.d', 'ssh/host-keys')


def make_seed(image, key):
    require(not image.exists(), 'fresh data image required')
    target = TARGETS['USRDATA']
    with tempfile.TemporaryDirectory(prefix='y2-owner-seed-') as temporary:
        seed = Path(temporary)
        (seed/'.y2data-schema').write_text('1\n')
        for name in DIRECTORIES:
            (seed/name).mkdir(mode=0o700 if name.startswith('ssh') else 0o755, parents=True, exist_ok=True)
        (seed/'ssh/authorized_keys.d/authorized_keys').write_bytes(key)
        (seed/'ssh/authorized_keys.d/authorized_keys').chmod(0o600)
        with image.open('xb') as stream:
            stream.truncate(target['size'])
        subprocess.run(['mke2fs', '-q', '-t', 'ext4', '-F', '-L', 'Y2DATA', '-U', target['uuid'],
                        '-O', '^64bit', '-E', 'lazy_itable_init=0,lazy_journal_init=0,root_owner=0:0',
                        '-d', str(seed), str(image)], check=True, capture_output=True)
        commands = seed/'ownership.debugfs'
        commands.write_text(''.join('set_inode_field /'+name+' '+field+' 0\n'
                                   for name in ('.y2data-schema', *DIRECTORIES, 'ssh/authorized_keys.d/authorized_keys')
                                   for field in ('uid', 'gid')))
        subprocess.run(['debugfs', '-w', '-f', str(commands), str(image)], check=True, capture_output=True)
    validate_seed(image, key)
    subprocess.run(['e2fsck', '-f', '-n', str(image)], check=True, capture_output=True)


def validate(out):
    from tools.production.validate import validate_checksums
    out = Path(out)
    allowed = {'manifest.json', 'system-manifest.json', 'owner-authorized.pub',
               'Y2DATA.img', 'MT6582_Y2DATA_only_scatter.txt', 'README.txt', 'SHA256SUMS'}
    require({path.name for path in out.iterdir()} == allowed and
            all(path.is_file() and not path.is_symlink() for path in out.iterdir()),
            'exact first-owner file allowlist; no extra images, directories or symlinks')
    manifest = json.loads((out/'manifest.json').read_text())
    require(manifest['schema'] == 'org.y2linux.first-owner/v1' and
            manifest['initializes_data'] is True and manifest['preserves_existing_data'] is False,
            'explicit destructive first-initialization scope')
    require(manifest['selected_partitions'] == ['USRDATA'] and
            manifest['normal_update_allowlist'] == ['BOOTIMG', 'ANDROID'], 'isolated initialization profile')
    require(manifest['storage_addressing'] == addressing_contract(), 'stock addressing')
    require(manifest['image'] == {'file': 'Y2DATA.img', 'bytes': TARGETS['USRDATA']['size'],
                                 'sha256': digest(out/'Y2DATA.img')}, 'seed identity')
    require(manifest['system_manifest_sha256'] == digest(out/'system-manifest.json'), 'system receipt identity')
    system = json.loads((out/'system-manifest.json').read_text())
    require(system['installation_profile'] == 'system-update' and system['data_schema_version'] == 1,
            'matching paired system schema')
    require({p['target_partition'] for p in system['payloads']} == {'BOOTIMG', 'ANDROID'}, 'paired system payloads')
    stock = (PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    require((out/'MT6582_Y2DATA_only_scatter.txt').read_text() == make_data_scatter(stock), 'only data selected')
    key = public_key(out/'owner-authorized.pub')
    require(digest(out/'owner-authorized.pub') == manifest['owner_public_key_sha256'], 'owner public key identity')
    validate_seed(out/'Y2DATA.img', key)
    identity = subprocess.check_output(['blkid', '-p', '-o', 'export', str(out/'Y2DATA.img')]).decode()
    for field, expected in [('TYPE', 'ext4'), ('LABEL', 'Y2DATA'), ('UUID', TARGETS['USRDATA']['uuid'])]:
        require(field+'='+expected+'\n' in identity, 'first-owner filesystem '+field)
    subprocess.run(['e2fsck', '-f', '-n', str(out/'Y2DATA.img')], check=True, capture_output=True)
    require(not any((out/name).exists() for name in ('BOOTIMG.img', 'Y2ROOT.img', 'NVRAM.img')), 'no extra partition images')
    validate_checksums(out)
    return manifest


def package(system, out, public):
    from tools.production.validate import validate_manifest
    key = public_key(public)
    receipt = validate_manifest(system)
    require(receipt['installation_profile'] == 'system-update' and receipt['data_schema_version'] == 1,
            'validated paired preserving system package required')
    require(not out.exists(), 'fresh output directory required')
    out.mkdir(parents=True, mode=0o700)
    shutil.copyfile(system/'manifest.json', out/'system-manifest.json')
    (out/'owner-authorized.pub').write_bytes(key)
    make_seed(out/'Y2DATA.img', key)
    stock = (PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    (out/'MT6582_Y2DATA_only_scatter.txt').write_text(make_data_scatter(stock))
    manifest = {'schema': 'org.y2linux.first-owner/v1', 'initializes_data': True,
                'preserves_existing_data': False, 'selected_partitions': ['USRDATA'],
                'normal_update_allowlist': ['BOOTIMG', 'ANDROID'], 'data_schema_version': 1,
                'storage_addressing': addressing_contract(), 'system_manifest_sha256': digest(out/'system-manifest.json'),
                'owner_public_key_sha256': digest(out/'owner-authorized.pub'),
                'image': {'file': 'Y2DATA.img', 'bytes': TARGETS['USRDATA']['size'], 'sha256': digest(out/'Y2DATA.img')},
                'calibration_policy': 'No calibration copied; original device-local protected partitions remain untouched.',
                'physical_installation': 'NOT_RUN'}
    (out/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    (out/'README.txt').write_text('FIRST INITIALIZATION ONLY. Replaces existing Y2DATA. Never use for an update.\n'
                                'Owner must separately approve data initialization and confirm the exact board/layout.\n'
                                'Install BOOTIMG/Y2ROOT from the paired system package; this folder selects only USRDATA.\n'
                                'Never select Format All or Firmware Upgrade; leave every protected partition unchecked.\n')
    (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+p.name+'\n' for p in sorted(out.iterdir()) if p.is_file() and p.name != 'SHA256SUMS'))
    validate(out)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--system', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--public-key', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(package(args.system.resolve(), args.output.resolve(), args.public_key.resolve())))


if __name__ == '__main__': main()
