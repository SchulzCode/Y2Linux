#!/usr/bin/env python3
"""Run a command in the locked, networkless, device-isolated build userspace."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

PROJECT = Path(__file__).resolve().parents[2]


def prepare_overlay(project, spec):
    """Publish verified immutable overlay bytes; never rewrite a bound inode."""
    original = (project / '.cache/sources/linux-6.18' / spec['path']).read_bytes()
    if hashlib.sha256(original).hexdigest() != spec['base_sha256']:
        raise ValueError('Reviewed overlay base mismatch: ' + spec['path'])
    overlay = project / '.cache/overlays' / spec['result_sha256']
    overlay.parent.mkdir(parents=True, exist_ok=True)
    if overlay.exists():
        if hashlib.sha256(overlay.read_bytes()).hexdigest() != spec['result_sha256']:
            raise ValueError('Existing overlay cache mismatch: ' + spec['path'])
        return overlay
    patch = (project / 'kernel/patches' / spec['patch']).read_bytes()
    with tempfile.NamedTemporaryFile(dir=overlay.parent, prefix='.overlay-', delete=False) as f:
        temporary = Path(f.name)
        f.write(original)
    try:
        subprocess.run(['patch', '--batch', '--fuzz=0', str(temporary)], input=patch, check=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != spec['result_sha256']:
            raise ValueError('Reviewed overlay result mismatch: ' + spec['path'])
        os.replace(temporary, overlay)
    finally:
        temporary.unlink(missing_ok=True)
    return overlay


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', default='out/build', help='project-relative build directory')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a command is required')
    lock_path = PROJECT / 'tools/build/inputs.lock.json'
    lock = json.loads(lock_path.read_text())
    rootfs = PROJECT / '.cache/environment'
    fingerprint = hashlib.sha256(lock_path.read_bytes()).hexdigest()
    marker = rootfs / '.y2-build-lock'
    if not marker.exists() or marker.read_text().strip() != fingerprint:
        raise SystemExit('Run tools/build/prepare.py first; environment lock mismatch')
    output = (PROJECT / args.output).resolve()
    if not output.is_relative_to(PROJECT / 'out'):
        raise SystemExit('Build output must be below project/out')
    output.mkdir(parents=True, exist_ok=True)
    # Each existing-file overlay requires exact base/result hashes and a zero-fuzz patch.
    specs = json.loads((PROJECT / 'kernel/patches/manifest.json').read_text())['overlays']
    overlays = []
    for spec in specs:
        overlays.append((prepare_overlay(PROJECT,spec), spec['path']))
    env = {
        'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C', 'TZ': 'UTC',
        'ARCH': 'arm', 'LLVM': '1', 'CROSS_COMPILE': 'arm-linux-gnueabi-',
        'SOURCE_DATE_EPOCH': str(lock['source_date_epoch']),
        'KBUILD_BUILD_TIMESTAMP': '@' + str(lock['source_date_epoch']),
        'KBUILD_BUILD_USER': lock['build_user'], 'KBUILD_BUILD_HOST': lock['build_host'],
        'KBUILD_BUILD_VERSION': '1', 'PYTHONDONTWRITEBYTECODE': '1',
    }
    invocation = [
        'bwrap', '--unshare-all', '--uid', '0', '--gid', '0', '--die-with-parent',
        '--new-session', '--hostname', 'y2-build', '--clearenv',
        '--ro-bind', str(rootfs), '/', '--proc', '/proc', '--dev', '/dev',
        '--tmpfs', '/tmp', '--ro-bind', str(PROJECT), '/project',
        '--ro-bind', str(PROJECT / '.cache/sources/linux-6.18'), '/src',
        '--bind', str(output), '/build', '--chdir', '/build',
    ]
    # A private parent mount permits adding Y2 sources without creating files
    # in the locked upstream tree or replacing another platform's directory.
    invocation += ['--tmpfs', '/src/drivers']
    for child in sorted((PROJECT / '.cache/sources/linux-6.18/drivers').iterdir()):
        invocation += ['--ro-bind', str(child), '/src/drivers/' + child.name]
    invocation += ['--ro-bind', str(PROJECT / 'kernel/platform'), '/src/drivers/y2']
    for overlay, path in overlays:
        invocation += ['--ro-bind', str(overlay), '/src/' + path]
    for key, value in env.items():
        invocation += ['--setenv', key, value]
    raise SystemExit(subprocess.call(invocation + ['--'] + command))


if __name__ == '__main__':
    main()
