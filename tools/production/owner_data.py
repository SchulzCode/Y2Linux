#!/usr/bin/env python3
"""Personalize an unused production Y2DATA template with an existing public key.

Creates a data-initialization-only package. Never opens devices, private keys,
or live data; never builds or changes BOOTIMG/Y2ROOT. Flashing is manual.
"""
import argparse
import base64
import copy
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.production.layout import TARGETS, addressing_contract, digest, make_data_scatter, require


def public_key(path):
    # Validate the path before reading: the private sibling is never opened.
    require(path.suffix == '.pub' and path.is_file() and not path.is_symlink(),
            'existing regular .pub file required; private keys are never inputs')
    key = path.read_bytes()
    fields = key.split()
    require(len(key.splitlines()) == 1 and key.endswith(b'\n') and
            len(fields) >= 2 and fields[0] == b'ssh-ed25519', 'one Ed25519 public key required')
    blob = base64.b64decode(fields[1], validate=True)
    require(len(blob) == 51 and blob[:19] == struct.pack('>I', 11)+b'ssh-ed25519'+struct.pack('>I', 32),
            'Ed25519 public key encoding')
    return key


def fs_read(image, command):
    return subprocess.check_output(['debugfs', '-R', command, str(image)], stderr=subprocess.DEVNULL)


def validate_seed(image, key):
    """An initialization seed must contain no live state or private host keys."""
    require(image.is_file() and not image.is_symlink() and image.stat().st_size == TARGETS['USRDATA']['size'],
            'regular bounded data template')
    expected = {
        '': {'lost+found', '.y2data-schema', 'settings', 'apps', 'y2player', 'logs', 'cache', 'updates', 'ssh'},
        'ssh': {'authorized_keys.d', 'host-keys'},
        'ssh/authorized_keys.d': {'authorized_keys'},
    }
    expected.update({name: set() for name in ('lost+found', 'settings', 'apps', 'y2player', 'logs',
                                             'cache', 'updates', 'ssh/host-keys')})
    for directory, names in expected.items():
        found = set()
        for line in fs_read(image, 'ls -p /'+directory).decode().splitlines():
            if not line: continue
            fields = line.split('/')
            require(len(fields) >= 7, 'data directory record')
            if fields[1] == '0' or fields[5] in ('.', '..'): continue
            require(fields[3:5] == ['0', '0'] and fields[2].startswith(('040', '100')),
                    'data type/ownership; no symlinks or device nodes')
            found.add(fields[5])
        require(found == names, 'unused initialization seed required: /'+directory)
    require(fs_read(image, 'cat /.y2data-schema') == b'1\n', 'data schema')
    require(fs_read(image, 'cat /ssh/authorized_keys.d/authorized_keys') == key,
            'packaged authorization must exactly match the selected existing public key')
    for path, mode in [('ssh', '0700'), ('ssh/authorized_keys.d', '0700'),
                       ('ssh/host-keys', '0700'), ('ssh/authorized_keys.d/authorized_keys', '0600')]:
        info = fs_read(image, 'stat /'+path).decode()
        require(mode in info and 'User:     0' in info and 'Group:     0' in info,
                'persistent SSH path permissions')


def validate_package(out, owner_public_key=None):
    m = json.loads((out/'manifest.json').read_text())
    require(m['schema'] == 'org.schulzcode.y2linux.release/v1' and
            m['installation_profile'] == 'data-initialization-only', 'explicit data initialization profile')
    require(m['preserves_existing_data'] is False and m['normal_update_allowlist'] == ['BOOTIMG', 'ANDROID'],
            'data initialization must never become a preserving system update')
    require(m['storage_addressing'] == addressing_contract(), 'stock logical/native addressing')
    system = json.loads((out/'metadata/system-manifest.json').read_text())
    base = json.loads((out/'metadata/template-base-manifest.json').read_text())
    require(digest(out/'metadata/system-manifest.json') == m['system_manifest_sha256'] and
            digest(out/'metadata/template-base-manifest.json') == m['template_base_manifest_sha256'],
            'retained system/template provenance')
    references = system['payloads'] + [p for p in system['installed_components'] if p['target_partition'] == 'ANDROID']
    require(m['installed_components'] == references and not (out/'BOOTIMG.img').exists() and
            not (out/'Y2ROOT.img').exists(), 'working BOOTIMG/Y2ROOT are references only')
    require(len(m['payloads']) == 1, 'one data payload')
    payload = m['payloads'][0]
    expected = copy.deepcopy(next(p for p in base['payloads'] if p['target_partition'] == 'USRDATA'))
    raw = {'file': 'Y2DATA.img', 'size_bytes': (out/'Y2DATA.img').stat().st_size,
           'sha256': digest(out/'Y2DATA.img')}
    expected.update(raw=raw, spft={'format': 'raw-ext4', **raw}, required_in_profiles=['data-initialization-only'])
    require(payload == expected and raw['size_bytes'] == TARGETS['USRDATA']['size'],
            'only Y2DATA contents change; exact stock geometry/schema')
    t = TARGETS['USRDATA']
    require(payload['target_partition'] == 'USRDATA' and payload['region'] == 'EMMC_USER' and
            payload['absolute_start_bytes'] == t['start'] and payload['scatter_linear_start_bytes'] == t['linear'] and
            payload['partition_relative_offset_bytes'] == 0 and payload['maximum_size_bytes'] == t['size'] and
            payload['filesystem'] == {'type': 'ext4', 'label': t['label'], 'uuid': t['uuid']},
            'exact stock data coordinates and filesystem identity')
    key = public_key(out/'metadata/owner-authorized.pub')
    if owner_public_key is not None:
        require(key == public_key(owner_public_key), 'package differs from requested owner public key')
    require(hashlib.sha256(key).hexdigest() == m['debug_access']['authorized_public_key_sha256'] and
            m['debug_access']['private_key_packaged'] is False, 'public authorization identity')
    validate_seed(out/'Y2DATA.img', key)
    identity = subprocess.check_output(['blkid', '-p', '-o', 'export', str(out/'Y2DATA.img')]).decode()
    for k, v in [('TYPE', 'ext4'), ('LABEL', 'Y2DATA'), ('UUID', TARGETS['USRDATA']['uuid'])]:
        require(k+'='+v+'\n' in identity, 'ext4 '+k)
    subprocess.run(['e2fsck', '-f', '-n', str(out/'Y2DATA.img')], check=True,
                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    name = 'MT6582_Y2DATA_only_scatter.txt'
    require((out/name).read_text() == make_data_scatter((PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()),
            'only USRDATA selected; all stock scatter fields unchanged')
    require(m['profiles'] == {name: {'sha256': digest(out/name), 'selected_partitions': ['USRDATA']}}, 'data-only profile')
    listed = {}
    for line in (out/'SHA256SUMS').read_text().splitlines():
        sha, filename = line.split('  ', 1)
        require(filename not in listed and not Path(filename).is_absolute() and '..' not in Path(filename).parts,
                'checksum path/inventory')
        require(digest(out/filename) == sha, 'package checksum '+filename)
        listed[filename] = sha
    require(set(listed) == {str(p.relative_to(out)) for p in out.rglob('*') if p.is_file() and p.name != 'SHA256SUMS'},
            'complete package inventory')
    print('PASS owner public key matches Y2DATA exactly; clean ext4, stock bounds, data-only selection, BOOTIMG/Y2ROOT preserved')
    return m


def package(base, system, out, owner_public_key):
    from tools.production.validate import validate_manifest
    key = public_key(owner_public_key)
    require(out.is_relative_to(PROJECT/'out') and not out.exists(), 'fresh package inside out required')
    require(not subprocess.check_output(['git', 'status', '--porcelain'], cwd=PROJECT).strip(),
            'commit reviewed personalization source before packaging')
    current = validate_manifest(system)
    require(current['installation_profile'] == 'boot-only' and
            current.get('storage_addressing') == addressing_contract(), 'corrected production kernel required')
    original = validate_manifest(base)
    require(digest(base/'manifest.json') == current['base_manifest_sha256'], 'same retained root/data base')
    old_data = next(p for p in original['payloads'] if p['target_partition'] == 'USRDATA')
    old_key = fs_read(base/'Y2DATA.img', 'cat /ssh/authorized_keys.d/authorized_keys')
    require(hashlib.sha256(old_key).hexdigest() == original['debug_access']['authorized_public_key_sha256'], 'base authorization')
    validate_seed(base/'Y2DATA.img', old_key)
    out.mkdir(); (out/'metadata').mkdir()
    pub = out/'metadata/owner-authorized.pub'; pub.write_bytes(key)
    image = out/'Y2DATA.img'
    subprocess.run(['cp', '--sparse=always', str(base/'Y2DATA.img'), str(image)], check=True)
    # debugfs edits only this regular, unused image copy. Source/path strings
    # are passed through its own command file, never interpreted by a shell.
    commands = ('rm /ssh/authorized_keys.d/authorized_keys\n'
                'write '+json.dumps(str(pub))+' /ssh/authorized_keys.d/authorized_keys\n'
                'set_inode_field /ssh/authorized_keys.d/authorized_keys uid 0\n'
                'set_inode_field /ssh/authorized_keys.d/authorized_keys gid 0\n'
                'set_inode_field /ssh/authorized_keys.d/authorized_keys mode 0100600\n')
    command_file = out/'metadata/personalize.debugfs'; command_file.write_text(commands)
    subprocess.run(['debugfs', '-w', '-f', str(command_file), str(image)], check=True,
                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    validate_seed(image, key) # Before publishing manifest/flash selections.
    require(digest(base/'Y2DATA.img') == old_data['raw']['sha256'], 'original template unchanged')
    shutil.copyfile(system/'manifest.json', out/'metadata/system-manifest.json')
    shutil.copyfile(base/'manifest.json', out/'metadata/template-base-manifest.json')
    payload = copy.deepcopy(old_data)
    raw = {'file': image.name, 'size_bytes': image.stat().st_size, 'sha256': digest(image)}
    payload.update(raw=raw, spft={'format': 'raw-ext4', **raw}, required_in_profiles=['data-initialization-only'])
    name = 'MT6582_Y2DATA_only_scatter.txt'
    (out/name).write_text(make_data_scatter((PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()))
    m = {'schema': 'org.schulzcode.y2linux.release/v1', 'installation_profile': 'data-initialization-only',
         'operation': 'owner-authorized replacement of initial Y2DATA state; not a normal system update',
         'status': 'offline-validated; manual data initialization and SSH acceptance pending',
         'build_git_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=PROJECT, text=True).strip(),
         'layout_version': 1, 'data_schema_version': 1, 'preserves_existing_data': False,
         'normal_update_allowlist': ['BOOTIMG', 'ANDROID'],
         'storage_addressing': addressing_contract(), 'payloads': [payload],
         'installed_components': current['payloads'] + [p for p in current['installed_components'] if p['target_partition'] == 'ANDROID'],
         'system_manifest_sha256': digest(system/'manifest.json'), 'template_base_manifest_sha256': digest(base/'manifest.json'),
         'debug_access': {'authorized_public_key_sha256': hashlib.sha256(key).hexdigest(),
                          'private_key_packaged': False, 'authorization_path': '/data/ssh/authorized_keys.d/authorized_keys',
                          'host_keys': 'generated per device and persisted in Y2DATA; none packaged'},
         'profiles': {name: {'sha256': digest(out/name), 'selected_partitions': ['USRDATA']}}}
    (out/'manifest.json').write_text(json.dumps(m, indent=2)+'\n')
    (out/'install.md').write_text(
        '# Owner-key data initialization\n\n'
        'This replaces Y2DATA, including initial state and device SSH host keys. '
        'The owner confirmed there are no saved files/settings to preserve. '
        'Use this explicit initialization only; ordinary BOOTIMG/Y2ROOT updates preserve Y2DATA.\n\n'
        'SPFT Download Only: load MT6582_Y2DATA_only_scatter.txt and select USRDATA → Y2DATA.img only. '
        'BOOTIMG, ANDROID and every other row stay unchecked. Do not Format or Firmware Upgrade. '
        'Keep the working Storage06 BOOTIMG and existing Y2ROOT.\n\n'
        'Boot without SD. USB host:10.42.0.2/24, Y2:10.42.0.1. Then '
        '`ssh -i ~/.ssh/y2linux_ed25519 root@10.42.0.1`. '
        'The owner private key is neither read nor packaged by these tools.\n')
    (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(out))+'\n'
        for p in sorted(out.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'))
    validate_package(out, owner_public_key)
    print('Prepared:', out)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--public-key', type=Path, default=Path.home()/'.ssh/y2linux_ed25519.pub')
    p.add_argument('--validate', type=Path)
    p.add_argument('--base', type=Path)
    p.add_argument('--system', type=Path)
    p.add_argument('--output', type=Path)
    a = p.parse_args()
    if a.validate:
        validate_package(a.validate.resolve(), a.public_key)
    else:
        if not all((a.base, a.system, a.output)): p.error('--base, --system and --output required')
        package(a.base.resolve(), a.system.resolve(), a.output.resolve(), a.public_key)


if __name__ == '__main__': main()
