"""Verified owner uploads: private staging, identity/space checks, durable no-replace publish."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import hashlib
import os
import re
import stat
import time
import uuid
from pathlib import Path
from .boot import private_directory
from .common import atomic_json, json_read
from .observe import storage
from .space import admission

MAX_BYTES = 512 * 1024**2


def data_volume(ctx):
    value = next(v for v in storage(ctx)['volumes'] if v['path'] == '/data')
    if value['state'] != 'Ready' or not value.get('generation'):
        raise ValueError('verified_data_unavailable')
    return value


def filename(name):
    if not isinstance(name, str) or len(name.encode()) > 512:
        raise ValueError('invalid_media_name')
    parts = name.split('/')
    if not 1 <= len(parts) <= 8 or any(not p or p.startswith('.') or len(p.encode()) > 120 or
                                     any(ord(c) < 32 or c == '\\' for c in p) for p in parts):
        raise ValueError('invalid_media_name')
    return parts


def stage_path(ctx, token):
    if not re.fullmatch(r'[0-9a-f]{32}', token):
        raise ValueError('invalid_transfer_id')
    return ctx.path('/data/transfer-staging') / token


def begin(ctx, name, size, sha256):
    filename(name)
    if type(size) is not int or not 0 < size <= MAX_BYTES or not re.fullmatch('[0-9a-f]{64}', sha256):
        raise ValueError('invalid_transfer_size_or_hash')
    volume = data_volume(ctx)
    if not admission(volume, size * 2, 'transfer')['allowed']:
        raise ValueError('insufficient_space_for_upload_seal_and_reserve')
    root = private_directory(ctx.path('/data/transfer-staging'))
    with os.scandir(root) as entries:
        if sum(1 for _, _entry in zip(range(9), entries)) >= 8:
            raise ValueError('staging_limit_remove_completed_or_aborted_transfers')
    token = uuid.uuid4().hex
    directory = root / token
    directory.mkdir(mode=0o700)
    value = {'schema': 1, 'id': token, 'name': name, 'bytes': size, 'sha256': sha256,
             'data_generation': volume['generation'], 'state': 'Staging',
             'record': ctx.record('usb-transfer', {'name': name, 'bytes': size})}
    atomic_json(directory/'manifest.json', value, durable=True)
    return {**value, 'upload_path': '/data/transfer-staging/' + token + '/payload.part'}


def open_directory(parent, component, create=False):
    if create:
        try:
            os.mkdir(component, 0o755, dir_fd=parent)
            os.fsync(parent)
        except FileExistsError:
            pass
    fd = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    if os.fstat(fd).st_dev != os.fstat(parent).st_dev:
        os.close(fd)
        raise ValueError('transfer_submount_refused')
    return fd


def commit(ctx, token):
    stage_path(ctx, token)  # validate before any path access
    volume = data_volume(ctx)
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    stage_root = stage = source = sealed = target = lock = None
    try:
        stage_root = open_directory(data, 'transfer-staging')
        stage = open_directory(stage_root, token)
        lock = os.open('lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600, dir_fd=stage)
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        anchored = Path('/proc/self/fd') / str(stage)
        value = json_read(anchored/'manifest.json', {})
        if value.get('schema') != 1 or value.get('id') != token or value.get('state') not in ('Staging', 'Committed'):
            raise ValueError('invalid_transfer_state')
        if value['state'] == 'Committed':
            return value
        parts = filename(value.get('name'))
        if value.get('data_generation') != volume['generation']:
            raise ValueError('data_generation_changed')
        size, expected = value.get('bytes'), value.get('sha256')
        if type(size) is not int or not 0 < size <= MAX_BYTES or not isinstance(expected, str) or not re.fullmatch('[0-9a-f]{64}', expected):
            raise ValueError('invalid_transfer_manifest')
        if not admission(volume, size, 'transfer')['allowed']:
            raise ValueError('insufficient_space_for_seal_and_reserve')
        source = os.open('payload.part', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=stage)
        original = os.fstat(source)
        if not stat.S_ISREG(original.st_mode) or original.st_nlink != 1 or original.st_size != size:
            raise ValueError('incomplete_or_invalid_upload')
        # Copy to an O_EXCL inode: an older SFTP FD cannot mutate the published
        # bytes after verification. Stale seal files are safe explicit retry artifacts.
        sealed = os.open('sealed.part', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=stage)
        digest = hashlib.sha256()
        remaining = size
        deadline = time.monotonic() + 600
        while remaining:
            if time.monotonic() >= deadline:
                raise ValueError('transfer_seal_deadline')
            chunk = os.read(source, min(1024**2, remaining))
            if not chunk:
                raise ValueError('upload_disappeared_or_truncated')
            free = os.fstatvfs(sealed)
            if free.f_bavail * free.f_frsize - len(chunk) < 96 * 1024**2:
                raise ValueError('transfer_storage_reserve')
            view = memoryview(chunk)
            while view:
                count = os.write(sealed, view)
                if count <= 0:
                    raise OSError('short_transfer_write')
                view = view[count:]
            digest.update(chunk)
            remaining -= len(chunk)
        if digest.hexdigest() != expected or os.fstat(source).st_size != size:
            raise ValueError('upload_hash_or_size_mismatch')
        os.fchmod(sealed, 0o644)
        os.fsync(sealed)
        if data_volume(ctx)['generation'] != value['data_generation']:
            raise ValueError('data_generation_changed')
        target = open_directory(data, 'music', create=True)
        for part in parts[:-1]:
            nextfd = open_directory(target, part, create=True)
            os.close(target)
            target = nextfd
        # linkat is atomic and fails on an existing name; never overwrite music.
        os.link('sealed.part', parts[-1], src_dir_fd=stage, dst_dir_fd=target, follow_symlinks=False)
        os.fsync(target)
        value['state'] = 'Committed'
        value['record'].update(result='OK', failure=None)
        atomic_json(anchored/'manifest.json', value, durable=True)
        os.unlink('sealed.part', dir_fd=stage)
        os.unlink('payload.part', dir_fd=stage)
        os.fsync(stage)
        return value
    finally:
        for fd in (target, sealed, source, lock, stage, stage_root, data):
            if fd is not None:
                os.close(fd)


def inspect(ctx, token):
    path = stage_path(ctx, token)
    if path.is_symlink() or path.parent.is_symlink():
        raise ValueError('staging_symlink_refused')
    return json_read(path/'manifest.json', {'state': 'Unavailable'})


def discard(ctx, token):
    """Explicit owner abort removes only a known stage; never follows a symlink."""
    data_volume(ctx)
    stage_path(ctx, token)
    root = os.open(ctx.path('/data/transfer-staging'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    fd = open_directory(root, token)
    lock = os.open('lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600, dir_fd=fd)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for name in ('manifest.json', 'payload.part', 'sealed.part', 'lock'):
            try:
                os.unlink(name, dir_fd=fd)
            except FileNotFoundError:
                pass
        os.rmdir(token, dir_fd=root)
        os.fsync(root)
        return {'state': 'Discarded', 'id': token}
    finally:
        os.close(lock); os.close(fd); os.close(root)
