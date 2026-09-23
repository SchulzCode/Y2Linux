"""Scratch-only storage measurements, descriptor-pinned across media removal."""
# SPDX-License-Identifier: GPL-2.0-only
import errno
import math
import os
from pathlib import Path
import random
import stat
import time
import uuid
from .common import read
from .observe import mountinfo, storage


class Scratch:
    """Never resolves workload names against a replaceable mountpoint path."""
    def __init__(self, ctx, volume, budget_bytes, guard=None):
        if volume not in ('/data', '/media/sd'):
            raise ValueError('scratch_volume_not_allowed')
        self.ctx, self.volume = ctx, volume
        self.guard = guard
        self.volume_fd = self.base_fd = self.fd = None
        self.name = 'run-' + uuid.uuid4().hex
        self.files = set()
        self.initial = self.identity()
        if guard is None:
            item = next(v for v in storage(ctx)['volumes'] if v['path'] == volume)
            if item['state'] != 'Ready' or item['space_state'] != 'Normal':
                raise ValueError('healthy_identity_verified_volume_required')
            self.initial_uuid = item['uuid']
        else:
            self.initial_uuid = 'test-fixture'
        try:
            self.volume_fd = os.open(ctx.path(volume), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            vfs = os.fstatvfs(self.volume_fd)
            if vfs.f_bavail * vfs.f_frsize < budget_bytes + max(96 * 1024**2, vfs.f_blocks * vfs.f_frsize // 10):
                raise ValueError('insufficient_scratch_reserve')
            self.ensure()
            try:
                os.mkdir('.y2-bench', 0o700, dir_fd=self.volume_fd)
            except FileExistsError:
                pass
            self.base_fd = os.open('.y2-bench', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                   dir_fd=self.volume_fd)
            meta = os.fstat(self.base_fd)
            if meta.st_uid != os.geteuid() or meta.st_mode & 0o077:
                raise ValueError('unsafe_scratch_permissions')
            os.mkdir(self.name, 0o700, dir_fd=self.base_fd)
            self.fd = os.open(self.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=self.base_fd)
            self.ensure()
        except BaseException:
            self.close()
            raise

    def identity(self):
        if self.guard:
            return self.guard()
        found = [m for m in mountinfo(self.ctx.read('/proc/self/mountinfo')) if m['path'] == self.volume]
        if len(found) != 1:
            raise OSError(errno.ENODEV, 'source_disappeared')
        m = found[0]
        if not self.ctx.path('/sys/dev/block/' + m['device_id']).exists() or 'ro' in m['options']:
            raise OSError(errno.ENODEV, 'source_unavailable')
        return (self.ctx.read('/proc/sys/kernel/random/boot_id'), m['mount_id'], m['device_id'])

    def ensure(self):
        if self.identity() != self.initial:
            raise OSError(errno.ENODEV, 'source_generation_changed')
        if self.volume_fd is not None:
            if os.stat(self.ctx.path(self.volume)).st_dev != os.fstat(self.volume_fd).st_dev:
                raise OSError(errno.ENODEV, 'source_device_changed')

    def open(self, name):
        if not name.isalnum():
            raise ValueError('invalid_scratch_name')
        self.ensure()
        fd = os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=self.fd)
        self.files.add(name)
        return fd

    def rename(self, source, target):
        if source not in self.files or not target.isalnum() or target in self.files:
            raise ValueError('unowned_scratch_rename')
        self.ensure()
        os.rename(source, target, src_dir_fd=self.fd, dst_dir_fd=self.fd)
        self.files.remove(source)
        self.files.add(target)

    def unlink(self, name):
        if name not in self.files:
            raise ValueError('unowned_scratch_file')
        self.ensure()
        os.unlink(name, dir_fd=self.fd)
        self.files.remove(name)

    def close(self):
        # Descriptor-relative cleanup can never touch a replacement filesystem.
        if self.fd is not None:
            for name in self.files:
                try:
                    os.unlink(name, dir_fd=self.fd)
                except OSError:
                    pass
            os.close(self.fd)
            self.fd = None
        if self.base_fd is not None:
            try:
                os.rmdir(self.name, dir_fd=self.base_fd)
            except OSError:
                pass
            os.close(self.base_fd)
            self.base_fd = None
        if self.volume_fd is not None:
            os.close(self.volume_fd)
            self.volume_fd = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def distribution(samples_ns, byte_count=0, duration_ns=None):
    if not samples_ns:
        return {'samples': 0, 'p50_ms': None, 'p95_ms': None, 'p99_ms': None, 'max_ms': None,
                'MB_per_second': None, 'operations_per_second': None}
    samples = sorted(samples_ns)
    elapsed = duration_ns if duration_ns is not None else sum(samples)
    def percentile(p):
        return samples[max(0, math.ceil(len(samples) * p) - 1)] / 1e6
    return {'samples': len(samples), 'p50_ms': percentile(.5), 'p95_ms': percentile(.95),
            'p99_ms': percentile(.99), 'max_ms': samples[-1] / 1e6,
            'MB_per_second': byte_count * 1000 / elapsed if elapsed else None,
            'operations_per_second': len(samples) * 1e9 / elapsed if elapsed else None,
            'bytes': byte_count, 'elapsed_ms': elapsed / 1e6}


def write_all(fd, data, offset):
    written = 0
    while written < len(data):
        n = os.pwrite(fd, memoryview(data)[written:], offset + written)
        if n <= 0:
            raise OSError(errno.EIO, 'short_write')
        written += n


def storage_benchmark(ctx, volume='/data', size_mib=16, operations=64, seconds=300, guard=None):
    if not 1 <= size_mib <= 128 or not 1 <= operations <= 1024 or not 1 <= seconds <= 1800:
        raise ValueError('benchmark_bounds_exceeded')
    size = size_mib * 1024**2
    deadline = time.monotonic() + seconds
    result = {'schema': 'org.y2linux.benchmark/v1',
              'record': ctx.record('storage', {'volume': volume, 'size_mib': size_mib,
                                             'operations': operations, 'deadline_seconds': seconds,
                                             'io': 'buffered', 'cold_read': 'file_fadvise_advisory_only',
                                             'seed': 6582}), 'measurements': [],
              'volume': None}
    result['record']['units'] = {'latency': 'ms', 'throughput': 'MB/s (decimal)', 'rate': 'operations/s'}
    rng = random.Random(6582)
    # A repeatable nonzero 1-MiB pattern prepared outside timed I/O.
    block = rng.randbytes(1024**2)

    def ensure(scratch):
        if time.monotonic() >= deadline:
            raise TimeoutError('benchmark_deadline')
        scratch.ensure()

    def measure(name, count, fn, bytes_per_op=0, finalize=None):
        samples = []
        started = time.monotonic_ns()
        for n in range(count):
            ensure(scratch)
            before = time.monotonic_ns()
            fn(n)
            samples.append(time.monotonic_ns() - before)
        if finalize:
            finalize()
        elapsed = time.monotonic_ns() - started
        result['measurements'].append({'operation': name, 'result': 'OK',
                                       **distribution(samples, count * bytes_per_op, elapsed)})

    try:
        with Scratch(ctx, volume, size + 1024**2, guard) as scratch:
            result['volume'] = {'path': volume, 'generation': scratch.initial, 'uuid': scratch.initial_uuid}
            if guard is None:
                result['volume']['observation'] = storage(ctx)
            fd = scratch.open('data')
            try:
                for bs in (4096, 16384, 65536, 1048576):
                    count = size // bs
                    measure(f'sequential_write_{bs}', count,
                            lambda n: write_all(fd, block[:bs], n * bs), bs,
                            finalize=lambda: os.fdatasync(fd))
                    if hasattr(os, 'posix_fadvise'):
                        os.posix_fadvise(fd, 0, size, os.POSIX_FADV_DONTNEED)
                    def verify(n):
                        if os.pread(fd, bs, n * bs) != block[:bs]:
                            raise OSError(errno.EIO, 'readback_mismatch')
                    measure(f'sequential_read_{bs}', count, verify, bs)
                    offsets = [rng.randrange(count) * bs for _ in range(min(count, operations))]
                    measure(f'random_write_{bs}', len(offsets),
                            lambda n: write_all(fd, block[:bs], offsets[n]), bs,
                            finalize=lambda: os.fdatasync(fd))
                    if hasattr(os, 'posix_fadvise'):
                        os.posix_fadvise(fd, 0, size, os.POSIX_FADV_DONTNEED)
                    def verify_random(n):
                        if os.pread(fd, bs, offsets[n]) != block[:bs]:
                            raise OSError(errno.EIO, 'readback_mismatch')
                    measure(f'random_read_{bs}', len(offsets), verify_random, bs)
                for name, sync in [('fsync', os.fsync), ('fdatasync', os.fdatasync)]:
                    def durable(n):
                        write_all(fd, block[:4096], 0)
                        sync(fd)
                    measure(name, min(operations, 64), durable, 4096)
            finally:
                os.close(fd)
            measure('create', operations, lambda n: os.close(scratch.open(f'f{n}')))
            measure('stat', operations, lambda n: os.stat(f'f{n}', dir_fd=scratch.fd, follow_symlinks=False))
            measure('rename', operations, lambda n: scratch.rename(f'f{n}', f'g{n}'))
            measure('delete', operations, lambda n: scratch.unlink(f'g{n}'))
            def rename_sync(n):
                fd = scratch.open('renameA')
                try:
                    write_all(fd, block[:4096], 0)
                    os.fsync(fd)
                finally:
                    os.close(fd)
                scratch.rename('renameA', 'renameB')
                os.fsync(scratch.fd)
                scratch.unlink('renameB')
                os.fsync(scratch.fd)
            try:
                measure('file_fsync_rename_directory_fsync', min(operations, 64), rename_sync, 4096)
            except OSError as error:
                if error.errno not in (errno.EINVAL, errno.ENOTSUP):
                    raise
                result['measurements'].append({'operation': 'file_fsync_rename_directory_fsync',
                                               'result': 'UNAVAILABLE', 'reason': 'filesystem_does_not_support_directory_fsync'})
            ensure(scratch)
        result['record']['result'] = 'OK'
    except (OSError, ValueError, TimeoutError) as error:
        result['record'].update(result='FAILED', failure={'type': type(error).__name__,
                                                         'reason': str(error), 'errno': getattr(error, 'errno', None)})
    return result
