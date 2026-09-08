#!/usr/bin/env python3
"""Run a command in the locked, networkless, device-isolated build userspace."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PROJECT = Path(__file__).resolve().parents[2]


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
    for key, value in env.items():
        invocation += ['--setenv', key, value]
    raise SystemExit(subprocess.call(invocation + ['--'] + command))


if __name__ == '__main__':
    main()
