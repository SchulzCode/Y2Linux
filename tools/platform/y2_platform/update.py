"""Signed root-only OTA admission and boot health; writes only dedicated staging.

The small native core is the shared authentication/policy authority. The rescue
copy performs offline backup/write/readback; this module never opens a block.
"""
# SPDX-License-Identifier: GPL-2.0-only
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import time
import urllib.parse
import urllib.request
import uuid
from .boot import private_directory, mark
from .common import atomic_json
from .transfer import data_volume

FILES = ('manifest.json', 'manifest.sig', 'rootfs.ext4.gz')
TERMINAL = ('Idle', 'Acknowledged', 'RolledBack', 'Failed')
RESERVE = 96 * 1024**2


def status(ctx):
    value = ctx.json('/data/updates/state.json')
    if value is None and not ctx.path('/data/updates/state.json').exists() and not ctx.path('/data/updates/state.json').is_symlink():
        value = {'schema': 1, 'state': 'Idle'}
    if not isinstance(value, dict) or value.get('schema') != 1:
        return {'schema': 1, 'state': 'Failed', 'failure': 'journal_unreadable'}
    return {**value, 'download': ctx.json('/data/updates/download.json'),
            'bootimg_automatic': False, 'physical_qualification': False}


def core(ctx, action, directory=None, timeout=15):
    argv = ['/usr/sbin/y2-update-core', action]
    if directory:
        argv.append(directory)
    result = ctx.command(argv, timeout=timeout, limit=20000, failure_output=True)
    if not result['ok']:
        try:
            refusal = json.loads(result.get('output') or '{}').get('failure')
        except (ValueError, TypeError, AttributeError):
            refusal = None
        raise ValueError(refusal or ('update_core_' + (result['reason'] or 'refused')))
    value = json.loads(result['output'])
    if not isinstance(value, dict):
        raise ValueError('invalid_update_core_response')
    return value


def free(fd, size):
    info = os.fstatvfs(fd)
    if (info.f_flag & os.ST_RDONLY or info.f_bavail * info.f_frsize < size + RESERVE or
            info.f_files and info.f_favail < 128):
        raise ValueError('update_data_space_reserve')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('update_redirect_refused')


def base_url(url):
    parts = urllib.parse.urlsplit(url)
    if (parts.scheme != 'https' or not parts.hostname or parts.username or parts.password or
            parts.fragment or parts.query or not parts.path.endswith('/manifest.json') or
            any(ord(c) < 33 for c in url)):
        raise ValueError('HTTPS_manifest_URL_required_without_credentials_query_or_redirect')
    return url[:-len('manifest.json')]


def copy_stream(source, destination, maximum, exact=None, seconds=900):
    digest = hashlib.sha256()
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    count = 0
    deadline = time.monotonic() + seconds
    try:
        with os.fdopen(fd, 'wb') as stream:
            while True:
                if time.monotonic() >= deadline:
                    raise ValueError('download_deadline')
                chunk = source.read(min(1024**2, maximum - count + 1))
                if not chunk:
                    break
                count += len(chunk)
                if count > maximum:
                    raise ValueError('download_size_limit')
                free(stream.fileno(), len(chunk))
                stream.write(chunk)
                digest.update(chunk)
            if exact is not None and count != exact:
                raise ValueError('incomplete_download')
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        # Keep interrupted bytes for explicit diagnostics; never queue them.
        raise
    return count, digest.hexdigest()


def remove_package(directory):
    # Only these three disposable files; unexpected contents stop cleanup.
    if not directory.exists():
        return
    if not stat.S_ISDIR(directory.lstat().st_mode):
        raise ValueError('unsafe_staging_directory')
    names = set(os.listdir(directory))
    if names - set(FILES):
        raise ValueError('unknown_staging_contents')
    for name in names:
        file = directory / name
        if not stat.S_ISREG(file.lstat().st_mode) or file.stat().st_nlink != 1:
            raise ValueError('unsafe_staging_file')
    for name in names:
        (directory / name).unlink()
    directory.rmdir()


def stage(ctx, source=None, url=None, check_only=False):
    if bool(source) == bool(url):
        raise ValueError('exactly_one_local_package_or_HTTPS_manifest_required')
    volume = data_volume(ctx)
    root = private_directory(ctx.path('/data/updates'))
    fd = os.open(root / '.stage.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    token = '.incoming-' + uuid.uuid4().hex
    incoming = root / token
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if status(ctx)['state'] not in TERMINAL:
            raise ValueError('update_already_pending')
        # An earlier interrupted download is disposable; bounded, named cleanup.
        old = list(root.glob('.incoming-*'))
        if len(old) > 8:
            raise ValueError('excess_staging_owner_inspection_required')
        for item in old:
            remove_package(item)
        incoming.mkdir(mode=0o700)
        atomic_json(root/'download.json', {'schema': 1, 'state': 'Staging', 'id': token}, durable=True)
        if url:
            from .timekeeping import status as clock
            if not clock(ctx)['tls_ready'] or not clock(ctx)['entropy_ready']:
                raise ValueError('clock_or_entropy_not_ready_for_TLS')
            base = base_url(url)
            opener = urllib.request.build_opener(NoRedirect(), urllib.request.ProxyHandler({}))

            @contextlib.contextmanager
            def get(name):
                with opener.open(base + name, timeout=10) as response:
                    if response.status != 200:
                        raise ValueError('HTTP_status')
                    yield response
        else:
            directory = Path(source)
            if not stat.S_ISDIR(directory.lstat().st_mode):
                raise ValueError('local_package_directory_required')

            @contextlib.contextmanager
            def get(name):
                source_fd = os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(source_fd, 'rb') as stream:
                    if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                        raise ValueError('local_package_regular_files_required')
                    yield stream
        for name, maximum in [('manifest.json', 16384), ('manifest.sig', 64)]:
            with get(name) as stream:
                copy_stream(stream, incoming/name, maximum, exact=64 if name.endswith('.sig') else None, seconds=30)
        manifest = core(ctx, 'check-manifest', '/data/updates/' + token)
        if check_only:
            remove_package(incoming)
            atomic_json(root/'download.json', {'schema': 1, 'state': 'Checked', 'release_version': manifest['release_version']})
            return manifest
        size = manifest['payload']['bytes']
        free(fd, size)
        with get('rootfs.ext4.gz') as stream:
            count, digest = copy_stream(stream, incoming/'rootfs.ext4.gz', size, exact=size)
        if digest != manifest['payload']['sha256']:
            raise ValueError('download_hash_mismatch')
        # Fresh sealed local copy, native decompression/hash/extent validation.
        core(ctx, 'check', '/data/updates/' + token, timeout=300)
        if data_volume(ctx)['generation'] != volume['generation']:
            raise ValueError('update_data_generation_changed')
        remove_package(root/'pending')
        incoming.rename(root/'pending')
        parent = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
        result = core(ctx, 'queue', timeout=300)
        atomic_json(root/'download.json', {'schema': 1, 'state': 'Verified', 'bytes': count,
                    'raw_bytes': manifest['payload']['raw_bytes'], 'sha256': digest,
                    'backup_budget': 'rescue captures actual compressed previous root before any write',
                    'reserve_bytes': RESERVE}, durable=True)
        return result
    except (OSError, ValueError, TimeoutError) as error:
        atomic_json(root/'download.json', {'schema': 1, 'state': 'Failed', 'failure': str(error)}, durable=True)
        raise
    finally:
        os.close(fd)


def apply(ctx, rollback=False):
    if rollback:
        core(ctx, 'rollback')
    elif status(ctx)['state'] != 'Queued':
        raise ValueError('no_verified_queued_update')
    from .power import rpc
    return rpc(ctx, {'action': 'reboot', 'reason': 'update'})


def health_ack(ctx):
    pending = status(ctx)
    if pending['state'] != 'PendingHealth':
        return pending
    runtime = private_directory(ctx.path('/run/y2'))
    progress = ctx.json('/run/y2/update-health.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if progress.get('boot_id') != boot:
        progress = {'boot_id': boot, 'deadline_monotonic_s': time.monotonic() + 180}
        atomic_json(runtime/'update-health.json', progress)
    if time.monotonic() >= progress['deadline_monotonic_s']:
        return core(ctx, 'rollback')  # next owner reboot restores; never an automatic boot loop
    # A successful frame alone is insufficient: app must still answer, core
    # mounts/devices must be usable, and the same boot/process must own readiness.
    ready = ctx.json('/run/y2/application-ready.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if ready.get('boot_id') != boot or ready.get('first_frame') is not True:
        return {'state': 'PendingHealth', 'reason': 'application_first_frame_missing'}
    pid = ready.get('pid')
    if type(pid) is not int or pid <= 0:
        return {'state': 'PendingHealth', 'reason': 'application_pid_missing'}
    from .observe import memory
    current = memory(ctx, [pid], False)['processes'][0]
    if current.get('start_ticks') != ready.get('start_ticks'):
        return {'state': 'PendingHealth', 'reason': 'application_generation_changed'}
    from .health import check
    checks = {item['name']: item for item in check(ctx)['checks']}
    required = ('root', 'data', 'filesystem_errors', 'display', 'render_node', 'audio', 'input')
    if any(checks.get(name, {}).get('state') != 'OK' for name in required):
        return {'state': 'PendingHealth', 'reason': 'core_health_not_OK'}
    answer = ctx.command(['/usr/bin/rebornctl', 'status', '--json'], timeout=2)
    try:
        app = json.loads(answer['output']) if answer['ok'] else {}
    except (ValueError, TypeError):
        app = {}
    if (app.get('graphics', {}).get('available') is not True or
            app.get('library', {}).get('error') is not None or app.get('uptime_seconds', 0) < 10):
        return {'state': 'PendingHealth', 'reason': 'application_not_ready'}
    mark(ctx, 'application_ready')
    return core(ctx, 'ack')
