"""One bounded durable boot journal; an unclean boot is not a reset diagnosis."""
# SPDX-License-Identifier: GPL-2.0-only
import os
from pathlib import Path
import stat
import time
from .common import atomic_json


def private_directory(path):
    path = Path(path)
    # No symlink traversal in mutable platform-owned state.
    for parent in list(reversed(path.parents)) + [path]:
        if parent.exists() or parent.is_symlink():
            mode = parent.lstat().st_mode
            if not stat.S_ISDIR(mode):
                raise ValueError('state_directory_not_directory')
        else:
            parent.mkdir(mode=0o700)
    os.chmod(path, 0o700)
    return path


def transition(previous, boot_id, stage, identity, now_ns):
    if not boot_id:
        raise ValueError('boot_id_unavailable')
    if not isinstance(previous, dict) or previous.get('schema') != 1:
        previous = {}
    same = previous.get('boot_id') == boot_id
    count = previous.get('boot_count', 0)
    if not isinstance(count, int) or count < 0:
        count = 0
    unclean = previous.get('consecutive_unclean', 0)
    if not isinstance(unclean, int) or unclean < 0:
        unclean = 0
    if not same:
        unclean = min(1000, unclean + 1) if previous and not previous.get('orderly_shutdown') else 0
    history = previous.get('history', []) if isinstance(previous.get('history'), list) else []
    if previous and not same:
        history = (history + [{k: previous.get(k) for k in
                              ('boot_id', 'last_stage', 'orderly_shutdown', 'identity')}])[-8:]
    return {'schema': 1, 'boot_id': boot_id, 'boot_count': count + (not same),
            'previous_boot_id': previous.get('previous_boot_id') if same else previous.get('boot_id'),
            'last_stage': stage, 'stage_monotonic_ns': now_ns,
            'orderly_shutdown': stage == 'shutdown_complete',
            'consecutive_unclean': unclean, 'recovery_recommended': unclean >= 3,
            'reset_cause': None, 'reset_cause_reason': 'not_observed',
            'history': history, 'identity': identity}


def mark(ctx, stage):
    if stage not in ('platform_start', 'services_started', 'application_ready', 'shutdown_pending', 'shutdown_complete'):
        raise ValueError('unknown_boot_stage')
    if ctx.read('/data/.y2data-schema') != '1':
        raise ValueError('data_schema_unavailable')
    from .observe import mountinfo
    mounted = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/data']
    if len(mounted) != 1 or mounted[0]['source'] != ctx.read('/run/y2-data-device'):
        raise ValueError('verified_data_mount_unavailable')
    path = private_directory(ctx.path('/data/system/platform')) / 'boot.json'
    import fcntl
    fd = os.open(path.parent / '.boot.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        value = transition(ctx.json('/data/system/platform/boot.json'),
                           ctx.read('/proc/sys/kernel/random/boot_id'), stage,
                           ctx.json('/etc/y2linux/versions.json'), time.monotonic_ns())
        atomic_json(path, value, durable=True)
        runtime = ctx.path('/run/y2/boot-stages.jsonl')
        runtime.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        import json
        fd_stage = os.open(runtime, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            if os.fstat(fd_stage).st_size < 8192:
                os.write(fd_stage, (json.dumps({'stage': stage, 'monotonic_s': time.monotonic()}) + '\n').encode())
        finally:
            os.close(fd_stage)
        return value
    finally:
        os.close(fd)


def capture_previous(ctx):
    """Snapshot existing bounded syslog before this boot's logging starts.

    No new logger/poll loop, retained-RAM allocation or guessed reset register.
    A panic may stop userspace before logging; absence of a marker proves nothing.
    """
    import hashlib
    import json
    if ctx.read('/data/.y2data-schema') != '1':
        raise ValueError('data_schema_unavailable')
    from .observe import mountinfo
    mounts = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/data']
    if len(mounts) != 1 or mounts[0]['source'] != ctx.read('/run/y2-data-device'):
        raise ValueError('verified_data_mount_unavailable')
    root = private_directory(ctx.path('/data/system/platform'))
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if not boot:
        raise ValueError('boot_id_unavailable')
    previous = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if previous.get('captured_on_boot_id') == boot:
        return previous
    journal = ctx.json('/data/system/platform/boot.json', {})
    if journal.get('boot_id') == boot:
        raise ValueError('previous_boot_capture_missed_early_boundary')
    result = {'schema': 1, 'captured_on_boot_id': boot, 'previous_boot_id': journal.get('boot_id'),
              'previous_last_stage': journal.get('last_stage'), 'previous_orderly_shutdown': journal.get('orderly_shutdown'),
              'reset_cause': None, 'reset_cause_reason': 'no_qualified_retained_register_or_pstore_backend',
              'kernel_panic_retention_guaranteed': False, 'logs_are_private': True,
              'record': ctx.record('previous-boot-evidence'), 'logs': []}
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    logs = None
    try:
        try:
            logs = os.open('logs', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=data)
        except FileNotFoundError:
            logs = None
        if logs is not None:
            if os.fstat(logs).st_dev != os.fstat(data).st_dev:
                raise ValueError('log_submount_refused')
            for name in ('system.log.0', 'system.log'):
                try:
                    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=logs)
                except FileNotFoundError:
                    continue
                with os.fdopen(fd, 'rb') as stream:
                    info = os.fstat(stream.fileno())
                    if not stat.S_ISREG(info.st_mode):
                        raise ValueError('invalid_log_file')
                    stream.seek(max(0, info.st_size - 65536))
                    raw = stream.read(65536)
                lines = [line[:500] for line in raw.decode(errors='replace').splitlines()[-32:]]
                result['logs'].append({'name': name, 'tail_sha256': hashlib.sha256(raw).hexdigest(),
                                       'source_bytes_at_capture': info.st_size, 'tail_lines': lines,
                                       'bounded_tail_not_complete_log': True})
        while len(json.dumps(result).encode()) > 96000:
            largest = max(result['logs'], key=lambda item: len(item['tail_lines']))
            largest['tail_lines'].pop(0)
        atomic_json(root/'previous-boot-evidence.json', result, durable=True)
        return result
    finally:
        if logs is not None: os.close(logs)
        os.close(data)


def evidence_status(ctx):
    value = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if not value:
        return {'state': 'Unavailable', 'reason': 'no_previous_boot_record'}
    return {k: value.get(k) for k in ('captured_on_boot_id', 'previous_boot_id',
            'previous_last_stage', 'previous_orderly_shutdown', 'reset_cause',
            'reset_cause_reason', 'kernel_panic_retention_guaranteed')} | {
            'private_record': '/data/system/platform/previous-boot-evidence.json',
            'log_tails': [{k: log.get(k) for k in ('name','tail_sha256','source_bytes_at_capture')}
                          for log in value.get('logs',[])]}
