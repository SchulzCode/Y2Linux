"""Tester export with an allowlisted projection, separate from private backup.

No raw log, database, credential file, calibration or user filename is exported.
Log excerpts are represented by fixed event classes and numeric errno only: free
text cannot be reliably scrubbed by a password regular expression.
"""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import hashlib
import io
import json
import math
import os
import re
import stat
import tarfile
import time
import uuid
from .boot import private_directory
from .observe import snapshot
from .health import check
from .transfer import data_volume
from .space import admission
from .diagnostic_fields import FIELDS, KERNEL_STATUS_KEYS, KERNEL_FIELDS

MAX_BYTES = 2 * 1024**2
# Values, not a permissive pattern: arbitrary strings are private by default.
PUBLIC_WORDS = frozenset(('Ready', 'Complete', 'Failed', 'Idle', 'Normal', 'Active',
    'Charging', 'Discharging', 'Full', 'Unknown', 'Unavailable', 'Stopped', 'Playing',
    'Paused', 'OK', 'FAILED', 'DEGRADED', 'UNAVAILABLE', 'arch_sys_counter', 'connected', 'disconnected', 'enabled', 'disabled', 'true', 'false',
    'SBC', 'AAC', 'aptX', 'aptX-HD', 'LDAC', 'S16_LE', 'S24_LE', 'S32_LE',
    'HS200', 'DDR52', 'HS52', 'SDR104', 'SDR50', 'DDR50', 'HS', 'WFI', 'SLIDLE',
    'DORMANT', 'schedutil', 'performance', 'powersave', 'ext4', 'vfat', 'exfat'))
PUBLIC_WORDS |= frozenset(('idle requested refused sleeping restoring restored restore_failed rtc power multiple unknown '
    'high standard mobile xq xq+ Auto SBC-XQ inventra_dma pio hs200 ddr52 mmc-hs sdr104 sdr50 ddr50 sd-hs ready '
    'NONE SUSPEND_REQUEST FILESYSTEM_SYNCED DEVICES_SUSPENDED SECONDARIES_OFF CIRQ_CLONED '
    'WAKE_MASK_PROGRAMMED RTC_ARMED PCM_INSTALLED CPU_CONTEXT_SAVING BEFORE_SPM_ENTRY '
    'AFTER_SPM_RETURN CPU_CONTEXT_RESTORED CIRQ_REPLAYED TIMER_RESTORED SECONDARIES_ON '
    'DEVICES_RESUMING RADIOS_RESTORING REBORN_READY COMPLETE UART_REQUEST UART_ACK '
    'NORMAL_PCM_RESTORED ABORTED HELPER_REQUEST TASKS_FROZEN PLATFORM_BEGIN DPM_PREPARE_BEGIN '
    'DPM_PREPARED LATE_SUSPENDED NOIRQ_SUSPENDED SECONDARIES_DISABLING SYSCORE_SUSPENDED '
    'PLATFORM_ENTER TEST_RETURN SELFTEST_A SELFTEST_B BACKSTOP_STARTED EXIT DEVICES_RESUMED '
    'CONSOLE_RESUMED PLATFORM_ENDED TASKS_THAWED FILESYSTEMS_THAWED POST_SUSPEND_NOTIFIED '
    'CONSOLE_RESTORED').split())
PRIVATE_KEYS = re.compile(r'(password|passphrase|secret|credential|ssid|bssid|address|mac$|cid$|serial|calibration|nvram|bond|link.?key|identity|filename|track|title|artist|album|path|cmdline)', re.I)
IDENTITY_KEYS = frozenset(('build_git_commit', 'reborn_source_commit', 'y2linux_commit', 'reborn_commit', 'sha256'))
VERSION_KEYS = frozenset(('kernel_version', 'kernel', 'rootfs_version', 'rootfs_release', 'release_version', 'reborn_version', 'build_id'))
EVENTS = ('timeout', 'crc', 'underrun', 'xrun', 'overflow', 'abort', 'resume',
          'suspend', 'reconnect', 'disconnect', 'failed', 'error', 'panic', 'warning')
COMPONENTS = ('msdc', 'mmc', 'musb', 'usb', 'spm', 'cirq', 'gpt', 'afe', 'alsa',
              'cs431', 'bluetooth', 'bluealsa', 'wifi', 'wlan', 'reborn', 'battery',
              'charger', 'ext4', 'drm', 'lima', 'watchdog')


def project(value, key='', depth=0):
    """Keep counters and known public enums; discard unknown strings and keys."""
    if depth > 16:
        return '[depth-limit]'
    if PRIVATE_KEYS.search(key):
        return '[redacted]'
    if value is None or type(value) in (bool, int):
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, dict):
        return {k: project(v, k, depth+1) for k, v in list(value.items())[:512]
                if isinstance(k, str) and k in FIELDS}
    if isinstance(value, (list, tuple)):
        return [project(v, key, depth+1) for v in value[:256]]
    if isinstance(value, str):
        if key in KERNEL_STATUS_KEYS or key in ('11200000.usb', '11230000.mmc', '11240000.mmc'):
            return {field: project(raw, field, depth+1)
                    for field, raw in re.findall(r'(\w+)=(\S+)', value) if field in KERNEL_FIELDS}
        if key in IDENTITY_KEYS and re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', value):
            return value
        if key in ('boot_id', 'previous_boot_id') and re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', value):
            return value
        if key in VERSION_KEYS and re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.+_-]{0,99}', value):
            return value
        if len(value) < 32 and re.fullmatch(r'-?(?:[0-9]+(?:\.[0-9]+)?|0x[0-9a-fA-F]+)', value):
            return value
        if value in PUBLIC_WORDS:
            return value
        return '[redacted]'
    return None


def events(raw):
    """Fixed-vocabulary excerpts; no source text is ever copied into the bundle."""
    result = []
    for number, line in enumerate((raw or '').splitlines()[-256:]):
        lower = line.lower()
        tags = [tag for tag in EVENTS if re.search(r'\b'+tag+r'\w*\b', lower)]
        components = [part for part in COMPONENTS if part in lower]
        if tags and components:
            result.append({'tail_line': number, 'components': components, 'events': tags,
                           'errno': [int(x) for x in re.findall(r'\b(?:errno|error|ret)\s*[=:]\s*(-?\d{1,4})\b', lower)[:4]]})
    return result


def log_tail(ctx, relative):
    """Walk reviewed data paths with no symlinks; never read arbitrary files."""
    fd = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parts = relative.split('/')
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        child = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        with os.fdopen(child, 'rb') as stream:
            meta = os.fstat(stream.fileno())
            if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1:
                raise ValueError('diagnostic_log_type')
            stream.seek(max(0, meta.st_size-65536))
            return events(stream.read(65536).decode(errors='replace'))
    except FileNotFoundError:
        return []
    finally:
        os.close(fd)


def collect(ctx):
    result = {'schema': 'org.y2linux.tester-diagnostics/v1', 'redacted': True,
              'privacy': 'Counters and known enums only; logs are event classes, never raw text.',
              'versions': project(ctx.json('/etc/y2linux/versions.json', {})),
              'boot_id': project(ctx.read('/proc/sys/kernel/random/boot_id'), 'boot_id'),
              'platform': project(snapshot(ctx)), 'health': project(check(ctx)),
              'previous_boot': project(ctx.json('/data/system/platform/previous-boot-evidence.json', {})),
              'suspend': project(ctx.json('/data/system/platform/suspend-last.json', {})),
              'logs': {}}
    for name, path in (('system', 'logs/system.log'), ('reborn', 'reborn/logs/reborn.log')):
        try:
            result['logs'][name] = log_tail(ctx, path)
        except (OSError, ValueError):
            result['logs'][name] = {'state': 'Failed'}
    kernel = ctx.command(['dmesg'], timeout=2, limit=1024**2)
    result['logs']['kernel'] = events(kernel.get('output')) if kernel.get('ok') else {'state': 'Unavailable'}
    return result


def export(ctx):
    volume = data_volume(ctx)
    if not admission(volume, MAX_BYTES, 'diagnostic')['allowed']:
        raise ValueError('diagnostic_space_reserved')
    directory = private_directory(ctx.path('/data/exports'))
    lock = os.open(directory/'.diagnostics.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    partial = None
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if sum(1 for _ in directory.glob('y2-diagnostics-*.tar.gz')) >= 8:
            raise ValueError('diagnostic_export_limit_retrieve_existing')
        raw = (json.dumps(collect(ctx), sort_keys=True, allow_nan=False)+'\n').encode()
        if len(raw) > MAX_BYTES:
            raise ValueError('diagnostic_size_limit')
        name = 'y2-diagnostics-'+str(int(time.time()))+'-'+uuid.uuid4().hex+'.tar.gz'
        partial = directory/('.'+name+'.partial')
        fd = os.open(partial, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as stream:
            with tarfile.open(fileobj=stream, mode='w:gz') as archive:
                entry = tarfile.TarInfo('diagnostics.json')
                entry.size, entry.mode = len(raw), 0o600
                archive.addfile(entry, io.BytesIO(raw))
            stream.flush()
            os.fsync(stream.fileno())
        if data_volume(ctx)['generation'] != volume['generation']:
            raise ValueError('diagnostic_data_generation_changed')
        complete = directory/name
        partial.rename(complete)
        parent = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
        with complete.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        return {'state': 'Complete', 'path': '/data/exports/'+name, 'sha256': digest,
                'bytes': complete.stat().st_size, 'redacted': True}
    finally:
        if partial is not None:
            partial.unlink(missing_ok=True)
        os.close(lock)
