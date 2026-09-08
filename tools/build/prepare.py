#!/usr/bin/env python3
"""Fetch locked inputs, then reconstruct the build rootfs without network access."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.request

PROJECT = Path(__file__).resolve().parents[2]
LOCK_PATH = PROJECT / 'tools/build/inputs.lock.json'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def extract(archive, destination):
    # Rootfs absolute links are made relative without changing their in-root meaning.
    def safe(member, root):
        member = copy.copy(member)
        if member.issym() and member.linkname.startswith('/'):
            member.linkname = os.path.relpath(member.linkname.lstrip('/'),
                                               os.path.dirname(member.name))
        return tarfile.data_filter(member, root)
    with tarfile.open(archive) as stream:
        stream.extractall(destination, filter=safe)


def installed(root):
    result = []
    for block in (root / 'lib/apk/db/installed').read_text().split('\n\n'):
        fields = dict(line.split(':', 1) for line in block.splitlines() if ':' in line)
        if 'P' in fields:
            result.append(fields['P'] + '=' + fields['V'])
    return sorted(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--offline', action='store_true', help='require cached archives')
    args = parser.parse_args()
    lock = json.loads(LOCK_PATH.read_text())
    cache = PROJECT / '.cache'
    for group, folder in [('base_archives', 'downloads'), ('apk_packages', 'apks')]:
        directory = cache / folder
        directory.mkdir(parents=True, exist_ok=True)
        for item in lock[group]:
            target = directory / item['name']
            if not target.exists():
                if args.offline:
                    raise SystemExit(f'Missing offline input: {target}')
                partial = target.with_suffix(target.suffix + '.partial')
                print(f"Fetching {item['name']}", flush=True)
                with urllib.request.urlopen(item['url'], timeout=120) as src, partial.open('wb') as dst:
                    while chunk := src.read(1024 * 1024):
                        dst.write(chunk)
                if sha(partial) != item['sha256']:
                    raise SystemExit(f'Checksum mismatch: {partial}')
                partial.replace(target)
            if sha(target) != item['sha256']:
                raise SystemExit(f'Checksum mismatch: {target}; no automatic replacement')
    rootfs = cache / 'environment'
    marker = rootfs / '.y2-build-lock'
    fingerprint = sha(LOCK_PATH)
    if not rootfs.exists():
        rootfs.mkdir()
        base = lock['base_archives'][0]
        extract(cache / 'downloads' / base['name'], rootfs)
        for name in ('project', 'src', 'build'):
            (rootfs / name).mkdir(exist_ok=True)
        subprocess.run([
            'bwrap', '--unshare-all', '--uid', '0', '--gid', '0', '--die-with-parent',
            '--bind', str(rootfs), '/', '--proc', '/proc', '--dev', '/dev',
            '--ro-bind', str(cache / 'apks'), '/packages', '--', '/sbin/apk',
            'add', '--no-network', '--force-non-repository', '--repositories-file', '/dev/null',
            *['/packages/' + p['name'] for p in lock['apk_packages']],
        ], check=True)
        if installed(rootfs) != lock['installed_packages']:
            raise SystemExit('Reconstructed package inventory differs from lock')
        marker.write_text(fingerprint + '\n')
    if not marker.exists() or marker.read_text().strip() != fingerprint:
        raise SystemExit('Environment incomplete or lock changed; use a fresh cache')
    if installed(rootfs) != lock['installed_packages']:
        raise SystemExit('Environment package inventory changed')
    sources = cache / 'sources'
    kernel = sources / 'linux-6.18'
    if not kernel.exists():
        sources.mkdir(exist_ok=True)
        extract(cache / 'downloads/linux-6.18.tar.xz', sources)
    for row in (PROJECT / 'docs/knowledge/linux-6.18-sources.tsv').read_text().splitlines()[1:]:
        name, size, digest, _ = row.split('\t')
        if name.endswith('.listing.json'):
            continue
        path = kernel / name
        if path.stat().st_size != int(size) or sha(path) != digest:
            raise SystemExit(f'Audited upstream source mismatch: {name}')
    print('Locked environment and audited v6.18 sources verified.')


if __name__ == '__main__':
    main()
