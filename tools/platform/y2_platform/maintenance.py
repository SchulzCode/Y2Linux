"""Explicit owner reset and private state export; never format Y2DATA.

Reset moves only reviewed user paths into a private recovery directory. Purge is
a separate confirmed, bounded operation. SSH/entropy/update/factory state stays.
"""
# SPDX-License-Identifier: GPL-2.0-only
import contextlib
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import re
import sqlite3
import stat
import tarfile
import time
import uuid
from .boot import private_directory
from .common import atomic_json, json_read
from .transfer import data_volume, open_directory
from .update import free, status as update_status

SCOPES = ('settings', 'network', 'bluetooth-bonds', 'library', 'caches', 'full-user')
SETTINGS = 'reborn/state/session.json'


def identity(meta):
    return {'device': meta.st_dev, 'inode': meta.st_ino, 'mode': stat.S_IFMT(meta.st_mode),
            'size': meta.st_size, 'mtime_ns': meta.st_mtime_ns}


def parent_fd(data, relative):
    parts = relative.split('/')
    if any(not x or x in ('.', '..') for x in parts):
        raise ValueError('invalid_maintenance_path')
    fd = os.dup(data)
    try:
        for part in parts[:-1]:
            nextfd = open_directory(fd, part)
            os.close(fd)
            fd = nextfd
        return fd, parts[-1]
    except BaseException:
        os.close(fd)
        raise


def inspect(data, relative):
    try:
        parent, name = parent_fd(data, relative)
    except FileNotFoundError:
        return None
    try:
        meta = os.stat(name, dir_fd=parent, follow_symlinks=False)
        if not (stat.S_ISREG(meta.st_mode) or stat.S_ISDIR(meta.st_mode)) or meta.st_dev != os.fstat(data).st_dev:
            raise ValueError('unsafe_maintenance_path:' + relative)
        return identity(meta)
    except FileNotFoundError:
        return None
    finally:
        os.close(parent)


def services(scope):
    value = ['S05reborn', 'S42y2-readiness']
    if scope in ('network', 'bluetooth-bonds', 'full-user'):
        value += ['S41y2-connectivity', 'S40bluetoothd']
    return value


def busy(ctx, scope):
    needles = ['reborn', 'reborn-supervise', 'service-daemon', 'readiness-supervise']
    if scope in ('network', 'bluetooth-bonds', 'full-user'):
        needles += ['connectivity', 'wpa_supplicant', 'udhcpc', 'bluealsad', 'bluetoothd', 'y2-bt-reconnect']
    found = []
    # Actual command lines, not stale PID files. Include supervision owners.
    entries = list(ctx.path('/proc').glob('[0-9]*/cmdline'))
    if len(entries) > 4096:
        raise ValueError('process_inventory_limit')
    for entry in entries:
        if entry.parent.name == str(os.getpid()) and ctx.root == Path('/'):
            continue
        try:
            with entry.open('rb') as stream:
                raw = stream.read(4096)
        except FileNotFoundError:
            continue
        tokens = raw.decode(errors='replace').split('\0')
        if any(Path(token).name in needles for token in tokens if token):
            found.append(int(entry.parent.name))
    return found


def child_paths(ctx, relative):
    directory = ctx.path('/data/' + relative)
    if not directory.exists():
        return []
    if not stat.S_ISDIR(directory.lstat().st_mode):
        raise ValueError('unsafe_maintenance_directory')
    import itertools
    entries = sorted(itertools.islice(directory.iterdir(), 257))
    if len(entries) > 256:
        raise ValueError('maintenance_directory_limit')
    return [relative + '/' + p.name for p in entries]


def paths(ctx, scope):
    if scope == 'settings':
        return [SETTINGS]
    if scope == 'network':
        return ['network']
    if scope == 'bluetooth-bonds':
        return ['bluetooth/preferred-audio', *child_paths(ctx, 'bluetooth/bluez')]
    if scope == 'library':
        return ['reborn/library.db', 'reborn/library.db-wal', 'reborn/library.db-shm', 'reborn/library.db-journal']
    if scope == 'caches':
        return ['reborn/cache', 'cache']
    # Known user-owned state only. Unknown/factory/calibration/private platform
    # directories are deliberately excluded, even in this explicit full reset.
    return ['reborn', 'music', 'network', 'settings', 'apps', 'y2player', 'cache', 'logs',
            'bluetooth/enabled', 'bluetooth/preferred-audio', *child_paths(ctx, 'bluetooth/bluez'),
            *child_paths(ctx, 'bluetooth/bluealsa')]


def plan(ctx, scope):
    if scope not in SCOPES:
        raise ValueError('unknown_reset_scope')
    volume = data_volume(ctx)
    if update_status(ctx)['state'] not in ('Idle', 'Acknowledged', 'RolledBack', 'Failed'):
        raise ValueError('update_in_progress')
    pending = ctx.path('/data/system/platform/maintenance-pending')
    if pending.exists():
        raise ValueError('resume_existing_reset_before_planning_another')
    live = busy(ctx, scope)
    requirements = {'stop_services': services(scope), 'restart': 'owner reboot after completion',
                    'preserved': ['SSH identity', 'entropy', 'update/rollback', 'factory/calibration', 'unknown data directories']}
    if live:
        return {'state': 'NeedsQuiescence', 'scope': scope, 'pids': live, **requirements}
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        targets = [{'path': p, 'identity': meta} for p in paths(ctx, scope) if (meta := inspect(data, p))]
    finally:
        os.close(data)
    value = {'schema': 1, 'id': uuid.uuid4().hex, 'state': 'Planned', 'scope': scope,
             'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'), 'data_generation': volume['generation'],
             'data_uuid': volume.get('uuid'),
             'expires_monotonic_s': time.monotonic() + 300, 'targets': targets, **requirements}
    value['confirmation'] = hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    root = private_directory(ctx.path('/data/system/platform'))
    atomic_json(root/'reset-plan.json', value, durable=True)
    return value


def durable_flag(path):
    fd = os.open(path, os.O_CREAT | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(b'Owner maintenance pending; keep mutable consumers stopped\n')
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def regular_bytes(data, relative, maximum):
    parent, name = parent_fd(data, relative)
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    finally:
        os.close(parent)
    with os.fdopen(fd, 'rb') as stream:
        meta = os.fstat(stream.fileno())
        if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1 or meta.st_size > maximum:
            raise ValueError('export_file_bounds:' + relative)
        value = stream.read(maximum + 1)
        if len(value) > maximum:
            raise ValueError('export_file_grew')
        return value


def execute(ctx, confirmation, erase_music=False, resume=False):
    root = private_directory(ctx.path('/data/system/platform'))
    lock = os.open(root/'.maintenance.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    data = quarantine = None
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        value = ctx.json('/data/system/platform/reset-plan.json', {})
        if not confirmation or value.get('confirmation') != confirmation or not re.fullmatch('[0-9a-f]{32}', value.get('id', '')):
            raise ValueError('exact_reset_confirmation_required')
        signed = {key: item for key, item in value.items() if key != 'confirmation'}
        if hashlib.sha256(json.dumps(signed, sort_keys=True, separators=(',', ':')).encode()).hexdigest() != confirmation:
            raise ValueError('reset_plan_changed')
        if value.get('scope') not in SCOPES or (not resume and value.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id')):
            raise ValueError('stale_reset_plan')
        if value['scope'] == 'full-user' and not erase_music:
            raise ValueError('full_reset_requires_erase_user_music_confirmation')
        if not resume and time.monotonic() > value['expires_monotonic_s']:
            raise ValueError('reset_plan_expired')
        if busy(ctx, value['scope']):
            raise ValueError('reset_consumers_still_running')
        volume = data_volume(ctx)
        if (not resume and volume['generation'] != value['data_generation'] or
                resume and (not value.get('data_uuid') or volume.get('uuid') != value['data_uuid'])):
            raise ValueError('reset_data_generation_changed')
        if update_status(ctx)['state'] not in ('Idle', 'Acknowledged', 'RolledBack', 'Failed'):
            raise ValueError('update_in_progress')
        data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        finished = ctx.json('/data/system/platform/maintenance-state.json', {})
        if finished.get('id') == value['id'] and finished.get('state') == 'Complete':
            (root/'maintenance-pending').unlink(missing_ok=True)
            return finished
        if not resume:
            for target in value['targets']:
                if inspect(data, target['path']) != target['identity']:
                    raise ValueError('reset_target_changed_before_apply')
        location = private_directory(ctx.path('/data/reset-quarantine')) / value['id']
        location.mkdir(mode=0o700, exist_ok=resume)
        quarantine = os.open(location, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        if os.fstat(quarantine).st_dev != os.fstat(data).st_dev:
            raise ValueError('reset_quarantine_submount')
        progress = {'schema': 1, 'id': value['id'], 'scope': value['scope'], 'state': 'Applying', 'completed': []}
        durable_flag(root/'maintenance-pending')
        atomic_json(location/'record.json', progress, durable=True)
        for index, target in enumerate(value['targets']):
            relative, expected = target['path'], target['identity']
            saved = f'{index:03d}'
            source = inspect(data, relative)
            try:
                old = identity(os.stat(saved, dir_fd=quarantine, follow_symlinks=False))
            except FileNotFoundError:
                old = None
            if value['scope'] == 'settings':
                if source != expected and not (resume and old and old['inode'] == expected['inode']):
                    # Settings backup is a copy, handled by completed hash below.
                    record = json_read(location/'settings-result.json', {})
                    if resume and record.get('sha256') == hashlib.sha256(regular_bytes(data, relative, 8*1024**2)).hexdigest():
                        progress['completed'].append(relative)
                        continue
                    raise ValueError('reset_settings_changed')
                raw = regular_bytes(data, relative, 8*1024**2)
                model = json.loads(raw)
                wrapped = 'schema' in model
                if wrapped and model.get('schema') != 2:
                    raise ValueError('unsupported_session_schema')
                body = model.get('model') if wrapped else model
                if not isinstance(body, dict) or 'settings' not in body:
                    raise ValueError('invalid_saved_session')
                answer = ctx.command(['/usr/bin/reborn', '--default-settings'], timeout=2)
                defaults = json.loads(answer['output']) if answer['ok'] else {}
                if defaults.get('session_schema') != 2 or not isinstance(defaults.get('settings'), dict):
                    raise ValueError('application_defaults_unavailable')
                if not old:
                    free(quarantine, len(raw)*2)
                    fd = os.open(saved, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=quarantine)
                    with os.fdopen(fd, 'wb') as backup:
                        backup.write(raw); backup.flush(); os.fsync(backup.fileno())
                    os.fsync(quarantine)
                body['settings'] = defaults['settings']
                # atomic_json's exact canonical encoding, including its newline.
                result = json.dumps(model, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()+b'\n'
                atomic_json(location/'settings-result.json', {'sha256': hashlib.sha256(result).hexdigest()}, durable=True)
                atomic_json(ctx.path('/data/'+relative), model, durable=True)
            else:
                if old and source is None and old['device'] == expected['device'] and old['inode'] == expected['inode']:
                    pass  # Durable rename completed before an interrupted journal update.
                elif old or source != expected:
                    raise ValueError('reset_target_changed:' + relative)
                else:
                    parent, name = parent_fd(data, relative)
                    try:
                        os.rename(name, saved, src_dir_fd=parent, dst_dir_fd=quarantine)
                        os.fsync(parent); os.fsync(quarantine)
                    finally:
                        os.close(parent)
            progress['completed'].append(relative)
            atomic_json(location/'record.json', progress, durable=True)
        if data_volume(ctx)['generation'] != volume['generation']:
            raise ValueError('reset_data_generation_changed')
        progress.update(state='Complete', preserved=value['preserved'],
                        quarantine='/data/reset-quarantine/'+value['id'], purge_confirmation=confirmation)
        atomic_json(location/'record.json', progress, durable=True)
        atomic_json(root/'maintenance-state.json', progress, durable=True)
        (root/'maintenance-pending').unlink()
        directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)
        return progress
    finally:
        for fd in (data, quarantine, lock):
            if fd is not None: os.close(fd)


def purge(ctx, token, confirmation, limit=256):
    if not re.fullmatch('[0-9a-f]{32}', token):
        raise ValueError('invalid_reset_id')
    data_volume(ctx)
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    parent = top = None
    try:
        parent = open_directory(data, 'reset-quarantine')
        top = open_directory(parent, token)
        record = json.loads(regular_bytes(top, 'record.json', 16384))
        if record.get('id') != token or record.get('state') != 'Complete' or record.get('purge_confirmation') != confirmation:
            raise ValueError('completed_reset_confirmation_required')
    except BaseException:
        if top is not None: os.close(top)
        raise
    finally:
        if parent is not None: os.close(parent)
        os.close(data)
    removed = 0
    deadline = time.monotonic()+8
    try:
        fcntl.flock(top, fcntl.LOCK_EX | fcntl.LOCK_NB)
        device = os.fstat(top).st_dev
        def walk(fd, depth=0):
            nonlocal removed
            if depth > 16: raise ValueError('purge_depth_limit')
            with os.scandir(fd) as entries:
                for entry in entries:
                    if depth == 0 and not re.fullmatch('[0-9]{3}', entry.name): continue
                    if removed >= limit or time.monotonic() >= deadline: return False
                    meta = entry.stat(follow_symlinks=False)
                    if meta.st_dev != device: raise ValueError('purge_submount_refused')
                    if stat.S_ISDIR(meta.st_mode):
                        child = open_directory(fd, entry.name)
                        try: complete=walk(child,depth+1)
                        finally: os.close(child)
                        if not complete: return False
                        os.rmdir(entry.name, dir_fd=fd)
                    else:
                        # Symlinks inside user content are unlinked, never followed.
                        os.unlink(entry.name, dir_fd=fd)
                    removed += 1
            os.fsync(fd)
            return True
        complete = walk(top)
        return {'state':'Complete' if complete else 'MoreRemaining','removed':removed,'id':token}
    finally:
        os.close(top)


def export(ctx, include_database=False, include_network=False):
    volume = data_volume(ctx)
    directory = private_directory(ctx.path('/data/exports'))
    token = uuid.uuid4().hex
    scratch = directory/('.export-'+token)
    scratch.mkdir(mode=0o700)
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    files, absent = [], []
    try:
        sources = [SETTINGS, 'bluetooth/preferred-audio']
        if include_network: sources += ['network/wpa_supplicant.conf', 'network/enabled']
        for relative in sources:
            try: raw = regular_bytes(data,relative,8*1024**2)
            except FileNotFoundError:
                absent.append(relative); continue
            free(data,len(raw))
            name=relative.replace('/','--')
            (scratch/name).write_bytes(raw); os.chmod(scratch/name,0o600)
            files.append((name,relative))
        if include_database:
            relative='reborn/library.db'; before=inspect(data,relative)
            if before is None: absent.append(relative)
            else:
                database=ctx.path('/data/'+relative)
                target=scratch/'library.db'
                deadline=time.monotonic()+30
                with contextlib.closing(sqlite3.connect(database.as_uri()+'?mode=ro',uri=True,timeout=.5)) as source, contextlib.closing(sqlite3.connect(target)) as dest:
                    def progress(status, remaining, total):
                        if time.monotonic()>deadline or total*4096>128*1024**2:
                            raise ValueError('database_export_limit')
                        free(data,1024**2)
                    source.backup(dest,pages=128,progress=progress,sleep=.05)
                    if dest.execute('PRAGMA quick_check').fetchone()!=('ok',): raise ValueError('database_backup_integrity')
                after=inspect(data,relative)
                if not after or (after['device'],after['inode']) != (before['device'],before['inode']):
                    raise ValueError('database_source_replaced')
                os.chmod(target,0o600); files.append(('library.db',relative))
        manifest={'schema':1,'record':ctx.record('owner-state-export'),
                  'contains_network_credentials':include_network,'excludes_bonds_and_private_keys':True,
                  'consistency':'SQLite online backup; each session file atomic; no cross-file transaction',
                  'files':[],'absent':absent}
        for name,relative in files:
            with (scratch/name).open('rb') as stream:
                digest=hashlib.file_digest(stream,'sha256').hexdigest()
            manifest['files'].append({'name':name,'original':relative,'sha256':digest,'bytes':(scratch/name).stat().st_size})
        atomic_json(scratch/'manifest.json',manifest,durable=True)
        output=directory/('.state-'+token+'.partial')
        fd=os.open(output,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
        with os.fdopen(fd,'wb') as stream:
            with tarfile.open(fileobj=stream,mode='w:gz') as archive:
                for name in ['manifest.json']+[n for n,_ in files]:
                    free(data, (scratch/name).stat().st_size)
                    archive.add(scratch/name,arcname=name,recursive=False)
            stream.flush();os.fsync(stream.fileno())
        if data_volume(ctx)['generation'] != volume['generation']: raise ValueError('export_data_generation_changed')
        complete=directory/('state-'+token+'.tar.gz')
        output.rename(complete); output=complete
        parent=os.open(directory,os.O_RDONLY|os.O_DIRECTORY)
        try:os.fsync(parent)
        finally:os.close(parent)
        with output.open('rb') as stream: digest=hashlib.file_digest(stream,'sha256').hexdigest()
        return {'state':'Complete','path':'/data/exports/'+output.name,'sha256':digest,'bytes':output.stat().st_size,
                'contains_network_credentials':include_network}
    finally:
        os.close(data)
        for entry in scratch.iterdir():
            if entry.is_file() and not entry.is_symlink(): entry.unlink()
        scratch.rmdir()
