"""Reject private state in delivered files and raw ext4 images, without mounting."""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import tempfile

PRIVATE_KEY = re.compile(rb'-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----[\r\n]+[A-Za-z0-9+/=\r\n]{40,}')
CREDENTIAL = re.compile(rb'(?im)^\s*(?:psk|password|passphrase)\s*=\s*(?:"[^"\r\n]+"|[0-9a-f]{64})\s*$|^\[(?:LinkKey|LongTermKey|IdentityResolvingKey)\]\r?\nKey=[0-9a-f]{32}')
PROTECTED = re.compile(r'^(?:preloader|lk|uboot|nvram|protect[12fs]*|pro_info|proinfo|seccfg|sec_ro|logo|expdb|fat|mbr|ebr[12]?|userdata|y2data|factory|calibration)(?:[._-].*)?\.(?:img|bin|dat)$', re.I)
PUBLIC_WIFI_TEMPLATE = b'ctrl_interface=/var/run/wpa_supplicant\nap_scan=1\n\nnetwork={\n  key_mgmt=NONE\n}'
PRIVATE_TREES = ('data/ssh/', 'data/network/', 'data/bluetooth/', 'data/factory/',
                 'data/calibration/', 'data/reborn/', 'var/lib/bluetooth/', 'var/lib/bluealsa/',
                 'root/.ssh/', 'home/')


def check_name(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts:
        raise ValueError('privacy_unsafe_path')
    low = str(path).lower()
    if (PROTECTED.fullmatch(path.name) or low.startswith(PRIVATE_TREES)
            or low == 'etc/wpa_supplicant.conf'
            or path.name in ('authorized_keys', 'authorized_keys2', 'library.db', 'session.json',
                             '.ash_history', '.bash_history', '.python_history')
            or re.fullmatch(r'(?:id_(?:rsa|dsa|ecdsa|ed25519)|ssh_host_.*_key|dropbear_.*_host_key)', path.name)):
        raise ValueError('private_state_path:'+name)


def check_stream(stream, name):
    previous = b''
    while chunk := stream.read(65536):
        raw = previous+chunk
        if PRIVATE_KEY.search(raw) or CREDENTIAL.search(raw):
            raise ValueError('private_state_content:'+name)
        previous = raw[-4096:]


def check_json(value, name):
    if isinstance(value, dict):
        for key, item in value.items():
            if key.lower() in ('password', 'passphrase', 'psk', 'private_key', 'linkkey',
                                'longtermkey', 'identityresolvingkey') and item not in (None, '', False):
                raise ValueError('private_state_json:'+name)
            check_json(item, name)
    elif isinstance(value, list):
        for item in value:
            check_json(item, name)


def tree(root, already_checked=()):
    root = Path(root)
    count = 0
    for path in sorted(root.rglob('*')):
        name = path.relative_to(root).as_posix()
        if path.is_symlink():
            # Persistent-state links are expected, but never traverse any link.
            continue
        if not path.is_file():
            continue
        # Buildroot's credential-free open-network sample is public source.
        # Only that exact template (with its optional comment) is exempt;
        # owner network files, SSIDs and any changed content are rejected.
        sample = (name == 'etc/wpa_supplicant.conf' and
                  path.read_bytes().replace(b'\r\n', b'\n').strip() in
                  (PUBLIC_WIFI_TEMPLATE, b'#'+PUBLIC_WIFI_TEMPLATE))
        if not sample:
            check_name(name)
        if name in already_checked:
            continue
        with path.open('rb') as stream:
            check_stream(stream, name)
        if path.suffix.lower() == '.json':
            try:
                value = json.loads(path.read_bytes())
            except (ValueError, UnicodeError):
                raise ValueError('privacy_invalid_json:'+name) from None
            check_json(value, name)
        count += 1
    return {'regular_files_checked': count, 'private_state_found': False}


def release_tree(root, manifest):
    """No protected extras or external links alongside the checked payloads."""
    root = Path(root)
    images = {p['raw']['file'] for p in manifest['payloads']}
    images.update(p['file'] for p in manifest['fallback']['images'])
    for path in root.rglob('*'):
        name = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ValueError('release_symlink:'+name)
        if path.is_file():
            check_name(name)
            if path.suffix.lower() in ('.img', '.bin', '.dat') and name not in images:
                raise ValueError('release_unexpected_image:'+name)
    # Both ext4 image contents and every byte (including free blocks) already
    # have hash-bound scan receipts. Still scan BOOTIMG and all other files.
    return tree(root, already_checked={'Y2ROOT.img', 'fallback/Y2ROOT.img'})


def ext4(image):
    image = Path(image)
    if not stat.S_ISREG(image.lstat().st_mode):
        raise ValueError('regular_root_image_required')
    image = image.resolve(strict=True)
    # The delivered bytes include journal and free blocks too: deleting a key
    # from a reused image does not remove its private contents from release.
    with image.open('rb') as stream:
        check_stream(stream, image.name)
    checked = subprocess.run(['e2fsck', '-f', '-n', str(image)], capture_output=True, timeout=300)
    if checked.returncode:
        raise ValueError('privacy_image_not_clean')
    with tempfile.TemporaryDirectory(prefix='y2-privacy-') as temporary:
        target = Path(temporary)/'root'
        target.mkdir()
        result = subprocess.run(['debugfs', '-R', 'rdump / '+str(target), str(image)],
                                capture_output=True, timeout=300)
        # debugfs can report an extraction failure but still exit zero. An
        # unprivileged reader cannot preserve inode owners; that does not lose
        # file content. Every other extraction error remains fatal.
        errors = [line for line in result.stderr.decode(errors='replace').splitlines()
                  if line and not re.fullmatch(r'debugfs [0-9][^\n]*', line)
                  and not any(line.startswith(prefix+str(target)+'/') for prefix in (
                      'rdump: Operation not permitted while changing ownership of ',
                      'dump_file: Operation not permitted while changing ownership of '))]
        if result.returncode or errors or not (target/'etc').is_dir():
            raise ValueError('privacy_image_read_failed')
        return {**tree(target), 'image_bytes_checked': image.stat().st_size,
                'image_sha256': image_digest(image), 'filesystem_checked': True}


def image_digest(image):
    import hashlib
    with Path(image).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def public_distribution(manifest, codecs):
    blockers = []
    if manifest.get('owner_firmware', {}).get('redistribution_permission_established') is not True:
        blockers.append('firmware_redistribution_unresolved')
    records = codecs.get('codecs') if isinstance(codecs, dict) else None
    if (not isinstance(codecs, dict) or codecs.get('schema') != 1 or
            not isinstance(records, dict) or set(records) != {'SBC', 'AAC', 'aptX', 'aptX-HD', 'LDAC'} or
            any(not isinstance(value, dict) or any(type(value.get(key)) is not bool
                for key in ('compiled_locally', 'distribution_approved')) for value in records.values())):
        blockers.append('codec_distribution_inventory_invalid')
        records = {}
    for name, value in records.items():
        if value['compiled_locally'] and not value['distribution_approved']:
            blockers.append('codec_distribution_unresolved:'+name)
    return {'public_distribution_ready': not blockers, 'blockers': blockers,
            'interpretation': 'Packaging gate, not a legal opinion.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--tree', type=Path)
    source.add_argument('--root-image', type=Path)
    args = parser.parse_args()
    print(json.dumps(tree(args.tree) if args.tree else ext4(args.root_image)))


if __name__ == '__main__':
    main()
