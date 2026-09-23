"""Bounded observation and durable record primitives; no device policy here."""
# SPDX-License-Identifier: GPL-2.0-only
import datetime
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import time


def read(path, limit=131072):
    try:
        with open(path, 'r', encoding='utf-8', errors='replace') as stream:
            value = stream.read(limit + 1)
        return value.strip() if len(value) <= limit else None
    except OSError:
        return None


def number(value):
    try:
        return int(value)
    except (ValueError, TypeError):
        return None


def json_read(path, default=None):
    try:
        return json.loads(read(path) or '')
    except (ValueError, TypeError):
        return default


def atomic_json(path, value, durable=False, mode=0o600):
    """Publish one complete record. Caller owns/trusts the parent directory."""
    path = Path(path)
    fd, name = tempfile.mkstemp(prefix='.' + path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            os.fchmod(stream.fileno(), mode)
            json.dump(value, stream, sort_keys=True, separators=(',', ':'), allow_nan=False)
            stream.write('\n')
            stream.flush()
            if durable:
                os.fsync(stream.fileno())
        os.replace(name, path)
        if durable:
            directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
    finally:
        try:
            os.unlink(name)
        except FileNotFoundError:
            pass


def command(argv, timeout=1.0, limit=65536):
    """No shell, bounded output/deadline; kill the process group on overrun."""
    try:
        process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, start_new_session=True,
                                   env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'})
    except OSError:
        return {'ok': False, 'reason': 'command_unavailable', 'output': None}
    output = bytearray()
    failure = None
    deadline = time.monotonic() + timeout
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                failure = 'command_timeout'
                break
            events = selector.select(remaining)
            if not events:
                failure = 'command_timeout'
                break
            chunk = os.read(process.stdout.fileno(), 4096)
            if not chunk:
                break
            output.extend(chunk)
            if len(output) > limit:
                failure = 'command_output_limit'
                break
    if not failure:
        try:
            process.wait(timeout=max(0.001, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            failure = 'command_timeout'
    if failure:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=0.05)
        except subprocess.TimeoutExpired:
            pass  # A kernel D-state cannot be given a userspace deadline.
    process.stdout.close()
    ok = failure is None and process.returncode == 0
    return {'ok': ok, 'reason': failure or (None if ok else 'command_failed'),
            'output': output.decode('utf-8', errors='replace').strip() if ok else None}


class Context:
    def __init__(self, root='/', runner=None):
        self.root = Path(root)
        # Fixture roots cannot accidentally query/mutate the host's services.
        self.runner = runner or (command if self.root == Path('/') else
                                 lambda *a, **kw: {'ok': False, 'reason': 'fixture', 'output': None})

    def path(self, name):
        return self.root / str(name).lstrip('/')

    def read(self, name, limit=131072):
        return read(self.path(name), limit)

    def integer(self, name):
        return number(self.read(name))

    def json(self, name, default=None):
        value = json_read(self.path(name), default)
        return default if default is not None and not isinstance(value, type(default)) else value

    def glob(self, pattern):
        return sorted(self.root.glob(pattern.lstrip('/')))[:256]

    def command(self, argv, **kwargs):
        return self.runner(argv, **kwargs)

    def record(self, workload='observation', parameters=None):
        versions = self.json('/etc/y2linux/versions.json', {})
        return {
            'wall_timestamp': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'monotonic_ns': time.monotonic_ns(),
            'boot_id': self.read('/proc/sys/kernel/random/boot_id'),
            'y2linux_commit': versions.get('build_git_commit'),
            'reborn_commit': versions.get('reborn_source_commit'),
            'kernel': self.read('/proc/sys/kernel/osrelease'),
            'rootfs_release': versions.get('rootfs_version'),
            'build_id': self.read('/etc/y2linux/build-id'),
            'workload': workload, 'parameters': parameters or {}, 'units': {},
            'result': None, 'failure': None, 'evidence_level': 'IMPLEMENTED',
        }


def key_values(text, separator=':'):
    result = {}
    for line in (text or '').splitlines():
        if separator in line:
            key, value = line.split(separator, 1)
            result[key.strip()] = value.strip()
    return result


def counters(text):
    return {k: number(v.split()[0]) for k, v in key_values(text, ' ').items() if v}
