#!/usr/bin/env python3
"""Offline Ed25519 key generation and signed full-system (OTA-35) package creation.

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
import struct
import subprocess
import zlib


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


def compress(source, target, limit):
    """Deterministic single-member gzip; returns the payload record."""
    source = Path(source)
    if not 4096 <= source.stat().st_size <= limit:
        raise ValueError(f'{source.name} outside its partition budget')
    raw_hash = hashlib.sha256()
    with source.open('rb') as stream_in, target.open('wb') as encoded:
        with gzip.GzipFile(fileobj=encoded, mode='wb', filename='', mtime=0) as stream:
            while block := stream_in.read(1024**2):
                raw_hash.update(block)
                stream.write(block)
        encoded.flush()
        os.fsync(encoded.fileno())
    with target.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return dict(name=target.name, encoding='gzip', bytes=target.stat().st_size, sha256=digest,
                raw_bytes=source.stat().st_size, raw_sha256=raw_hash.hexdigest())


SEGMENT = 1024 * 1024
GZIP_HEADER = bytes([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3])


def compress_segmented(source, target, limit):
    """The root payload as independent 1 MiB raw-deflate segments in one gzip member
    (the pigz -i layout; core.c SEGMENT). Any gzip reader decodes it as one stream; the
    recovery decodes the segments on all cores from the signed lengths. Every segment
    is decoded once here, so a wrong index can never reach a player."""
    source = Path(source)
    size = source.stat().st_size
    if not 4096 <= size <= limit:
        raise ValueError(f'{source.name} outside its partition budget')
    raw_hash, crc, segments = hashlib.sha256(), 0, []
    with source.open('rb') as stream_in, target.open('wb') as encoded:
        encoded.write(GZIP_HEADER)
        done = 0
        while done < size:
            block = stream_in.read(SEGMENT)
            if not block:
                raise ValueError(f'{source.name} shorter than its size')
            done += len(block)
            raw_hash.update(block)
            crc = zlib.crc32(block, crc)
            deflate = zlib.compressobj(9, zlib.DEFLATED, -15, 9)
            data = deflate.compress(block) + deflate.flush(zlib.Z_FINISH if done == size else zlib.Z_FULL_FLUSH)
            inflate = zlib.decompressobj(-15)
            if (inflate.decompress(data) != block or inflate.unconsumed_tail or inflate.unused_data or
                    inflate.eof != (done == size)):
                raise ValueError(f'{source.name}: segment {len(segments)} does not decode on its own')
            encoded.write(data)
            segments.append(len(data))
        encoded.write(struct.pack('<II', crc, size & 0xffffffff))
        encoded.flush()
        os.fsync(encoded.fileno())
    with target.open('rb') as stream:
        if gzip.decompress(stream.read()) != source.read_bytes():
            raise ValueError(f'{target.name}: not one valid gzip stream')
        stream.seek(0)
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return dict(name=target.name, encoding='gzip', bytes=target.stat().st_size, sha256=digest,
                raw_bytes=size, raw_sha256=raw_hash.hexdigest(), raw_crc32=crc, segment_bytes=SEGMENT,
                segments=segments)


def boot_name(image, expected, label):
    """The reviewed MTK boot layout and our image name, as y2-update-core checks it."""
    data = Path(image).read_bytes()[:1 << 24]
    if data[:8] != b'ANDROID!' or data[48:64].rstrip(b'\0') != expected:
        raise ValueError(f'{Path(image).name}: not a {expected.decode()} image')
    kernel = int.from_bytes(data[8:12], 'little')
    ramdisk = 2048 + (kernel + 2047) // 2048 * 2048
    if data[ramdisk + 8:ramdisk + 40] != label.ljust(32, b'\0'):
        raise ValueError(f'{Path(image).name}: ramdisk is not {label.decode()}')


def build(image, versions, output, private, key_id, sequence, minimum_sequence=0,
          allow_downgrade=False, rollback_allowed=False, bootimg=None, recovery=None, notes=''):
    """Signed full-system package: Y2ROOT + BOOTIMG, optionally RECOVERY."""
    output = Path(output)
    if output.exists() or not re.fullmatch(r'[a-zA-Z0-9._-]{1,64}', key_id):
        raise ValueError('fresh output directory and safe key ID required')
    if type(sequence) is not int or sequence < 1 or not 0 <= minimum_sequence < sequence:
        raise ValueError('invalid release sequence')
    if bootimg is None:
        raise ValueError('a full update needs the matching BOOTIMG')
    if not isinstance(notes, str) or len(notes) > 2000:
        raise ValueError('release notes must be short text')
    image = Path(image)
    if not 8 * 1024**2 <= image.stat().st_size <= 820 * 1024**2:
        raise ValueError('root image extent outside Y2ROOT budget')
    boot_name(bootimg, b'Y2Linux-M1', b'ROOTFS')
    if recovery is not None:
        boot_name(recovery, b'Y2Recovery-1', b'RECOVERY')
    output.mkdir(mode=0o700, parents=True)
    payloads = dict(rootfs=compress_segmented(image, output / 'rootfs.ext4.gz', 820 * 1024**2),
                    bootimg=compress(bootimg, output / 'bootimg.img.gz', 16 * 1024**2))
    if recovery is not None:
        payloads['recovery'] = compress(recovery, output / 'recovery.img.gz', 16 * 1024**2)
    manifest = dict(schema=2, product='Y2', hardware_revision='innioasis-y2-mt6582',
                    release_version=versions['release_version'], sequence=sequence,
                    minimum_sequence=minimum_sequence, kernel=versions['kernel_version'],
                    rootfs_version=versions['rootfs_version'], rootfs_contract='y2-platform-v1',
                    layout_schema=1, data_schema=1, recovery_api=1, reborn_database_schema=1,
                    y2linux_commit=versions['build_git_commit'], reborn_commit=versions['reborn_source_commit'],
                    reborn_version=versions.get('reborn_version', ''), notes=notes,
                    key_id=key_id, signature_format='Ed25519-detached-manifest.sig',
                    allow_downgrade=allow_downgrade, rollback_allowed=rollback_allowed,
                    payloads=payloads)
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
    for arg in ('image', 'bootimg', 'versions', 'output', 'private'):
        package.add_argument('--' + arg, type=Path, required=True)
    package.add_argument('--recovery', type=Path)
    package.add_argument('--notes', default='')
    candidate = sub.add_parser('candidate', help='sign the OTA package of a sealed SPFT candidate folder')
    candidate.add_argument('folder', type=Path)
    candidate.add_argument('--output', type=Path, required=True)
    candidate.add_argument('--private', type=Path, required=True)
    candidate.add_argument('--key-id', required=True)
    candidate.add_argument('--notes', default='')
    candidate.add_argument('--rollback-allowed', action='store_true')
    candidate.add_argument('--development-reinstall', action='store_true',
                           help='development key only: allow installing the same release again (device test)')
    package.add_argument('--key-id', required=True)
    package.add_argument('--sequence', type=int, required=True)
    package.add_argument('--minimum-sequence', type=int, default=0)
    package.add_argument('--development-downgrade', action='store_true')
    package.add_argument('--rollback-allowed', action='store_true')
    args = parser.parse_args()
    if args.command == 'keygen':
        print(json.dumps({'public_key_hex': keygen(args.private)}))
    elif args.command == 'candidate':
        # The exact bytes that were flashed and checked with a computer.
        folder = args.folder
        versions = json.loads((folder / 'metadata/versions.json').read_text())
        sums = dict(reversed(line.split('  ', 1)) for line in (folder / 'SHA256SUMS').read_text().splitlines())
        for name in ('BOOTIMG.img', 'Y2ROOT.img', 'recovery-install/RECOVERY.img'):
            with (folder / name).open('rb') as stream:
                if hashlib.file_digest(stream, 'sha256').hexdigest() != sums.get(name):
                    raise ValueError('candidate file changed since sealing: ' + name)
        print(json.dumps(build(folder / 'Y2ROOT.img', versions, args.output, args.private, args.key_id,
                               versions['update_sequence'], allow_downgrade=args.development_reinstall,
                               rollback_allowed=args.rollback_allowed,
                               bootimg=folder / 'BOOTIMG.img', recovery=folder / 'recovery-install/RECOVERY.img',
                               notes=args.notes), indent=2))
    else:
        print(json.dumps(build(args.image, json.loads(args.versions.read_text()), args.output,
                               args.private, args.key_id, args.sequence, args.minimum_sequence,
                               args.development_downgrade, args.rollback_allowed, args.bootimg,
                               args.recovery, args.notes), indent=2))


if __name__ == '__main__':
    main()
