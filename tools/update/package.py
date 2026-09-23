#!/usr/bin/env python3
"""Offline Ed25519 key generation and signed root-only release creation.

Private keys must live outside all repositories/artifacts and never go to Y2.
Uses libsodium's standard detached signature API; no custom cryptography.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import ctypes
import ctypes.util
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess


def sodium():
    name = ctypes.util.find_library('sodium')
    if not name:
        raise RuntimeError('install the host libsodium runtime')
    lib = ctypes.CDLL(name)
    lib.sodium_init.restype = ctypes.c_int
    lib.crypto_sign_keypair.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    lib.crypto_sign_detached.argtypes = [ctypes.c_void_p, ctypes.c_void_p,
                                       ctypes.c_void_p, ctypes.c_ulonglong, ctypes.c_void_p]
    if lib.sodium_init() < 0:
        raise RuntimeError('libsodium initialization failed')
    return lib


def keygen(private):
    private = Path(private).absolute()
    # Reject repositories and their out directories, including linked worktrees.
    ancestor = private.parent
    while not ancestor.exists():
        ancestor = ancestor.parent
    probe = subprocess.run(['git', '-C', str(ancestor), 'rev-parse', '--show-toplevel'],
                           capture_output=True)
    if probe.returncode == 0:
        raise ValueError('private signing keys must be outside repositories')
    private.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if private.parent.stat().st_mode & 0o077:
        raise ValueError('private key parent must have mode 0700')
    public, secret = ctypes.create_string_buffer(32), ctypes.create_string_buffer(64)
    if sodium().crypto_sign_keypair(public, secret) != 0:
        raise RuntimeError('key generation failed')
    fd = os.open(private, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(secret.raw)
        stream.flush()
        os.fsync(stream.fileno())
    ctypes.memset(secret, 0, 64)
    return public.raw.hex()


def sign(raw, private):
    fd = os.open(private, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077 or info.st_size != 64:
            raise ValueError('expected private mode-0600 libsodium Ed25519 key')
        secret = ctypes.create_string_buffer(stream.read(64), 64)
    signature = ctypes.create_string_buffer(64)
    length = ctypes.c_ulonglong()
    try:
        if sodium().crypto_sign_detached(signature, ctypes.byref(length), raw, len(raw), secret) or length.value != 64:
            raise RuntimeError('Ed25519 signing failed')
        return signature.raw
    finally:
        ctypes.memset(secret, 0, 64)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True,
                      allow_nan=False).encode()


def build(image, versions, output, private, key_id, sequence, minimum_sequence=0,
          allow_downgrade=False, rollback_allowed=False):
    output = Path(output)
    if output.exists() or not re.fullmatch(r'[a-zA-Z0-9._-]{1,64}', key_id):
        raise ValueError('fresh output directory and safe key ID required')
    if type(sequence) is not int or sequence < 1 or not 0 <= minimum_sequence < sequence:
        raise ValueError('invalid release sequence')
    image = Path(image)
    if not 8 * 1024**2 <= image.stat().st_size <= 820 * 1024**2:
        raise ValueError('root image extent outside Y2ROOT budget')
    output.mkdir(mode=0o700, parents=True)
    raw_hash = hashlib.sha256()
    with image.open('rb') as source, (output / 'rootfs.ext4.gz').open('wb') as encoded:
        with gzip.GzipFile(fileobj=encoded, mode='wb', filename='', mtime=0) as stream:
            while block := source.read(1024**2):
                raw_hash.update(block)
                stream.write(block)
        encoded.flush()
        os.fsync(encoded.fileno())
    compressed = output / 'rootfs.ext4.gz'
    with compressed.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    manifest = dict(schema=1, product='Y2', hardware_revision='innioasis-y2-mt6582',
                    release_version=versions['release_version'], sequence=sequence,
                    minimum_sequence=minimum_sequence, kernel=versions['kernel_version'],
                    rootfs_version=versions['rootfs_version'], rootfs_contract='y2-platform-v1',
                    layout_schema=1, data_schema=1, rescue_api=1, reborn_database_schema=1,
                    y2linux_commit=versions['build_git_commit'], reborn_commit=versions['reborn_source_commit'],
                    key_id=key_id, signature_format='Ed25519-detached-manifest.sig',
                    allow_downgrade=allow_downgrade, rollback_allowed=rollback_allowed,
                    payload=dict(name='rootfs.ext4.gz', target='Y2ROOT', encoding='gzip',
                                 bytes=compressed.stat().st_size, sha256=digest,
                                 raw_bytes=image.stat().st_size, raw_sha256=raw_hash.hexdigest()))
    raw = canonical(manifest)
    (output / 'manifest.json').write_bytes(raw)
    (output / 'manifest.sig').write_bytes(sign(raw, private))
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    key = sub.add_parser('keygen')
    key.add_argument('--private', type=Path, required=True)
    package = sub.add_parser('package')
    for arg in ('image', 'versions', 'output', 'private'):
        package.add_argument('--' + arg, type=Path, required=True)
    package.add_argument('--key-id', required=True)
    package.add_argument('--sequence', type=int, required=True)
    package.add_argument('--minimum-sequence', type=int, default=0)
    package.add_argument('--development-downgrade', action='store_true')
    package.add_argument('--rollback-allowed', action='store_true')
    args = parser.parse_args()
    if args.command == 'keygen':
        print(json.dumps({'public_key_hex': keygen(args.private)}))
    else:
        print(json.dumps(build(args.image, json.loads(args.versions.read_text()), args.output,
                               args.private, args.key_id, args.sequence, args.minimum_sequence,
                               args.development_downgrade, args.rollback_allowed), indent=2))


if __name__ == '__main__':
    main()
