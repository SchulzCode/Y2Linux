"""Y2DATA admission policy and bounded cleanup of explicitly disposable files."""
# SPDX-License-Identifier: GPL-2.0-only
import errno
import os
import re
import stat
import time
from .common import atomic_json
from .observe import storage


def admission(volume, requested_bytes=0, purpose='cache'):
    if requested_bytes < 0 or purpose not in ('cache', 'diagnostic', 'update', 'user_state', 'database', 'transfer'):
        raise ValueError('invalid_space_request')
    state = volume.get('space_state', 'Unavailable')
    if state in ('ReadOnlyRisk', 'Failed', 'Unavailable'):
        return {'allowed': False, 'reason': state}
    reserve = 32 * 1024**2 if purpose in ('user_state', 'database') else 96 * 1024**2
    free = volume.get('available_bytes')
    allowed = isinstance(free, int) and free - requested_bytes >= reserve
    if purpose in ('cache', 'diagnostic', 'update') and state != 'Normal':
        allowed = False
    return {'allowed': allowed, 'reason': 'space_reserved' if not allowed else 'budget_available',
            'reserve_bytes': reserve, 'requested_bytes': requested_bytes}


def disposable_cleanup(ctx, max_files=64, max_bytes=8 * 1024**2, now=None):
    """Never walk arbitrary trees or delete media, settings, databases or stages."""
    now = time.time() if now is None else now
    result = {'files': 0, 'bytes': 0, 'errors': []}
    rootfd = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for parts, pattern, keep in (
            (('reborn', 'cache'), r'[0-9]+-[0-9]+--?[0-9]+\.rgba', 0),
            (('reborn', 'diagnostics'), r'reborn-diagnostic-[0-9]+-[0-9]+\.tar\.gz', 2),
            (('cache', 'platform'), r'cache-[a-zA-Z0-9]+\.tmp', 0),
        ):
            fd = os.dup(rootfd)
            try:
                for part in parts:
                    nextfd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                    os.close(fd)
                    fd = nextfd
                if os.fstat(fd).st_dev != os.fstat(rootfd).st_dev:
                    raise OSError(errno.EXDEV, 'cleanup_submount_refused')
                # A bounded scan, not an unbounded in-memory directory listing.
                entries = []
                with os.scandir(fd) as iterator:
                    for n, entry in enumerate(iterator):
                        if n >= 4096:
                            break
                        if not re.fullmatch(pattern, entry.name):
                            continue
                        meta = entry.stat(follow_symlinks=False)
                        if stat.S_ISREG(meta.st_mode) and meta.st_nlink == 1:
                            entries.append((meta.st_mtime, entry.name, meta.st_size, meta.st_ino))
                entries.sort(reverse=True)
                for mtime, name, size, inode in reversed(entries[keep:]):
                    if now - mtime < 86400 or result['files'] >= max_files or result['bytes'] + size > max_bytes:
                        continue
                    current = os.stat(name, dir_fd=fd, follow_symlinks=False)
                    if current.st_ino != inode or current.st_size != size or not stat.S_ISREG(current.st_mode):
                        continue
                    os.unlink(name, dir_fd=fd)
                    result['files'] += 1
                    result['bytes'] += size
            except FileNotFoundError:
                pass
            except OSError as error:
                result['errors'].append({'category': parts[-1], 'errno': error.errno})
            finally:
                os.close(fd)
    finally:
        os.close(rootfd)
    return result


def policy(ctx, cleanup=False):
    volumes = storage(ctx)['volumes']
    data = next(v for v in volumes if v['path'] == '/data')
    result = {'schema': 'org.y2linux.space/v1', 'record': ctx.record('space-policy'),
              'state': data['space_state'], 'volume': data,
              'admission': {p: admission(data, purpose=p) for p in
                            ('cache', 'diagnostic', 'update', 'user_state', 'database', 'transfer')},
              'cleanup': None}
    if cleanup and data['state'] == 'Ready' and data['space_state'] in ('LowSpace', 'CriticalSpace'):
        result['cleanup'] = disposable_cleanup(ctx)
    directory = ctx.path('/run/y2')
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    atomic_json(directory / 'storage.json', result)
    return result
