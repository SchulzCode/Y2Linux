"""Differential harness for the platform port (docs/planning/PLATFORM-RUST-PORT-PLAN.md §3.5).

One `y2-platform` subcommand runs twice, each time on its own copy of the same fixture root: once as the Python
platform (tests/platform_differential_launcher.py, `Context` rooted at the copy) and once as another implementation,
normally a host build of the Rust binary. The two runs are compared on exit code, stdout, the subprocesses asked
for and the resulting tree (paths, file types, modes, contents, symlink targets). Values that are clock readings
are inputs, not behaviour, and are normalised by key name or pattern.

A host build of the Rust binary is given by `Y2_PLATFORM_RUST` (path) and must honour:
- `Y2_ROOT=<dir>`: every device path is that directory + the absolute path (as `ctx.path`); no host file outside it
  is read or written.
- `Y2_FIXTURE_COMMANDS=<file>`: JSON list of {"argv": [...], "ok": bool, "reason": str|null, "output": str|null};
  a subprocess is never started, its result is the entry with the same argv, otherwise
  {"ok": false, "reason": "fixture", "output": null} (what a Python fixture Context answers).
- `Y2_FIXTURE_CALLS=<file>`: append one JSON line {"argv": [...], "timeout": seconds} per subprocess asked for.
- `Y2_DAEMON_TICKS=<n>` (a case's `env`): a daemon returns after n iterations of its loop (fixture roots only); the
  case names `Y2_LAUNCHER_PATCH=<module>:<function>` (tests/platform_cases) to stop the Python daemon the same way.
- `Y2_PLATFORM_PYTHON`, `Y2_PLATFORM_MAIN`: the interpreter and entry its fallback runs for a subcommand it does
  not implement (honoured only with `Y2_ROOT`); the harness names the launcher, which takes the fixture from the
  same variables.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

REPOSITORY = Path(__file__).resolve().parents[1]
LAUNCHER = Path(__file__).resolve().parent / 'platform_differential_launcher.py'
RUST_ENV = 'Y2_PLATFORM_RUST'
# Clock readings and records of them; equal behaviour gives different values.
VOLATILE_KEYS = frozenset({
    'wall_timestamp', 'monotonic_ns', 'monotonic_s', 'stage_monotonic_ns', 'boottime_ns', 'elapsed_ms',
    'duration_ms', 'collection_duration_ns', 'timestamp', 'time', 'at', 'updated_at', 'checked_at', 'now', 'uptime_seconds',
    # statvfs() of the fixture copy measures the host's disk, which changes between the two runs.
    'available_bytes', 'free_bytes', 'available_inodes', 'total_bytes', 'total_inodes',
    'boottime_s', 'link_counts_since_s',
})
# Clock readings only when they are numbers: `epoch` is also the Wi-Fi association's id (a string).
VOLATILE_NUMBERS = frozenset({'epoch'})
# A leading clock reading on a trail line ("12.34 platform …").
TRAIL_CLOCK = re.compile(r'(?m)^\d+\.\d+ ')


def base_environment(extra):
    # What the platform's callers pass (udhcpc/ntpd hook variables) and nothing from the host.
    return {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', **extra}


def python_side(root, argv, commands_file, calls_file, timeout, env):
    command = [sys.executable, '-I', str(LAUNCHER), '--root', str(root), '--commands', str(commands_file),
               '--calls', str(calls_file), '--', *argv]
    return subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout,
                          env=base_environment(env))


def rust_side(binary):
    """A side that runs an implementation honouring the Y2_ROOT/Y2_FIXTURE_* contract above."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        # Subcommands not ported yet go to the Python platform (the dispatcher's fallback) on the same root.
        env = base_environment({**env, 'Y2_ROOT': str(root), 'Y2_FIXTURE_COMMANDS': str(commands_file),
                                'Y2_FIXTURE_CALLS': str(calls_file), 'Y2_PLATFORM_PYTHON': sys.executable,
                                'Y2_PLATFORM_MAIN': str(LAUNCHER),
                                'Y2_FIXTURE_FALLBACK': str(root.parent / 'fallback')})
        return subprocess.run([str(binary), *argv], stdin=subprocess.DEVNULL, capture_output=True,
                              timeout=timeout, env=env)
    return run


def rust_binary():
    value = os.environ.get(RUST_ENV)
    return Path(value) if value and Path(value).is_file() and os.access(value, os.X_OK) else None


def normalise_json(value, volatile):
    if isinstance(value, dict):
        return {k: '<volatile>' if k in volatile or (k in VOLATILE_NUMBERS and isinstance(v, (int, float)))
                else normalise_json(v, volatile) for k, v in value.items()}
    if isinstance(value, list):
        return [normalise_json(v, volatile) for v in value]
    return value


def normalise_text(text, root, volatile, patterns):
    text = text.replace(str(root), '<root>')
    for pattern, replacement in patterns:
        text = re.sub(pattern, replacement, text)
    lines = []
    for line in text.splitlines():
        try:
            lines.append(json.dumps(normalise_json(json.loads(line), volatile), sort_keys=True))
        except ValueError:
            lines.append(line)
    if len(lines) > 1:
        return lines
    try:
        # One JSON document (also when pretty-printed over several lines).
        return normalise_json(json.loads(text), volatile)
    except ValueError:
        return lines


def snapshot(root, volatile, patterns):
    """Every path under `root` with its type, mode and normalised content."""
    tree = {}
    for directory, names, files in os.walk(root):
        for name in sorted(names + files):
            path = Path(directory) / name
            relative = '/' + str(path.relative_to(root))
            info = path.lstat()
            mode = oct(info.st_mode & 0o7777)
            if path.is_symlink():
                target = os.readlink(path).replace(str(root), '<root>')
                tree[relative] = {'type': 'symlink', 'target': target}
            elif path.is_dir():
                tree[relative] = {'type': 'dir', 'mode': mode}
            elif path.is_file():
                data = path.read_bytes()
                try:
                    content = normalise_text(data.decode('utf-8'), root, volatile, patterns)
                except UnicodeDecodeError:
                    content = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                tree[relative] = {'type': 'file', 'mode': mode, 'content': content}
            else:
                tree[relative] = {'type': 'other', 'mode': mode}
    return tree


def run_case(side, fixture, argv, commands=(), timeout=60, volatile=(), patterns=(), env=None, files=None,
             ported=None):
    """Run `argv` with `side` on a fresh copy of `fixture` (changed by `files`: path -> text, a function
    returning it, or None to remove it); return everything compared, and under `fallback` whether a Rust side handed the command to
    the Python platform (never compared)."""
    volatile = VOLATILE_KEYS | set(volatile)
    patterns = [(TRAIL_CLOCK, '<clock> '), *patterns]
    with tempfile.TemporaryDirectory(prefix='y2-diff-') as scratch:
        scratch = Path(scratch)
        root = scratch / 'root'
        shutil.copytree(fixture, root, symlinks=True)
        for name, text in (files or {}).items():
            path = root / name.lstrip('/')
            text = text() if callable(text) else text  # made at run time (a clock reading)
            if text is None:
                path.unlink(missing_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
        commands_file = scratch / 'commands.json'
        commands_file.write_text(json.dumps(list(commands)))
        calls_file = scratch / 'calls.jsonl'
        calls_file.touch()
        result = side(root, argv, commands_file, calls_file, timeout, env or {})
        # A timeout is recorded as given (`timeout=3` or the default 1.0): compare it as a number.
        calls = [normalise_json({**call, 'timeout': float(call['timeout'])}, volatile) for call in
                 (json.loads(line.replace(str(root), '<root>')) for line in calls_file.read_text().splitlines() if line)]
        return {
            'exit': result.returncode,
            'stdout': normalise_text(result.stdout.decode('utf-8', errors='replace'), root, volatile, patterns),
            'calls': calls,
            'tree': snapshot(root, volatile, patterns),
            'fallback': (scratch / 'fallback').exists(),
        }


def differences(left, right, path=''):
    """Human-readable list of where two run_case results differ (empty when equal)."""
    if type(left) is not type(right):
        return ['%s: %r != %r' % (path or '/', left, right)]
    if isinstance(left, dict):
        found = []
        for key in sorted(set(left) | set(right), key=str):
            if key not in left or key not in right:
                found.append('%s/%s: only in %s' % (path, key, 'left' if key in left else 'right'))
            else:
                found.extend(differences(left[key], right[key], '%s/%s' % (path, key)))
        return found
    if isinstance(left, list) and len(left) == len(right):
        found = []
        for index, (a, b) in enumerate(zip(left, right)):
            found.extend(differences(a, b, '%s[%d]' % (path, index)))
        return found
    return [] if left == right else ['%s: %r != %r' % (path or '/', left, right)]


def compare(fixture, argv, left=python_side, right=None, **case):
    """Differences between two implementations of `argv` on `fixture`. With `ported` True (False), the
    right side must have run the command itself (handed it to Python)."""
    right = right or python_side
    a, b = run_case(left, fixture, argv, **case), run_case(right, fixture, argv, **case)
    found = differences({k: v for k, v in a.items() if k != 'fallback'}, {k: v for k, v in b.items() if k != 'fallback'})
    if right is not python_side and case.get('ported') is not None and b['fallback'] == case['ported']:
        found.append('ran in Python' if case['ported'] else 'ran in Rust, expected the Python fallback')
    return found


def host_access(fixture, argv, env=None, timeout=60, files=None):
    """What the Python side of `argv` reads, writes, lists or starts outside the fixture root."""
    with tempfile.TemporaryDirectory(prefix='y2-diff-') as scratch:
        scratch = Path(scratch)
        root = scratch / 'root'
        shutil.copytree(fixture, root, symlinks=True)
        for name, text in (files or {}).items():
            path = root / name.lstrip('/')
            text = text() if callable(text) else text  # made at run time (a clock reading)
            if text is None:
                path.unlink(missing_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
        (scratch / 'commands.json').write_text('[]')
        log = scratch / 'host-access.jsonl'
        command = [sys.executable, '-I', str(LAUNCHER), '--root', str(root), '--commands',
                   str(scratch / 'commands.json'), '--calls', str(scratch / 'calls.jsonl'),
                   '--host-access', str(log), '--', *argv]
        subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout,
                       env=base_environment(env or {}))
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
