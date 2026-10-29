"""Golden harness for the Rust platform (/usr/sbin/y2-platform, platform/).

One subcommand runs as the Rust host build (`Y2_PLATFORM_RUST`) on a fresh copy of a fixture root. Its result
is normalised and compared with the case's golden file, tests/platform_goldens/<group>/<case>.json: exit code,
stdout, the subprocesses asked for, and the tree difference against the root as the command found it (paths
added, changed or removed, with type, mode, contents and symlink targets). Clock readings are inputs, not
behaviour, and are normalised by key name or pattern; the root and repository paths are replaced.

The goldens were first recorded from the Python platform the Rust one was ported from (1:1, accepted
differences in platform/CONTRACT.md). A deliberate behaviour change re-records the goldens it touches:
`Y2_RECORD_GOLDENS=1` writes each golden that differs (or is missing) from the Rust run instead of failing,
and removes goldens no case names; review the git diff. Without it a missing golden fails.

The Rust host build honours below a fixture root:
- `Y2_ROOT=<dir>`: every device path is that directory + the absolute path; no host file outside it is read
  or written.
- `Y2_FIXTURE_COMMANDS=<file>`: JSON list of {"argv": [...], "ok": bool, "reason": str|null, "output":
  str|null}; a subprocess is never started, its result is the entry with the same argv, otherwise
  {"ok": false, "reason": "fixture", "output": null}.
- `Y2_FIXTURE_CALLS=<file>`: one JSON line {"argv": [...], "timeout": seconds} per subprocess asked for.
- `Y2_DAEMON_TICKS=<n>` (a case's `env`): a daemon returns after n iterations of its loop.
- `Y2_FIXTURE_TOKEN=<hex32>` (a case's `env`): the update staging token instead of a random one.
"""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

REPOSITORY = Path(__file__).resolve().parents[1]
GOLDENS = Path(__file__).resolve().parent / 'platform_goldens'
RUST_ENV = 'Y2_PLATFORM_RUST'
RECORD_ENV = 'Y2_RECORD_GOLDENS'
SKIP = RUST_ENV + ' (host build of the Rust y2-platform: cd platform && cargo build --offline --locked) not given'
# Clock readings and records of them; equal behaviour gives different values.
VOLATILE_KEYS = frozenset({
    'wall_timestamp', 'monotonic_ns', 'monotonic_s', 'stage_monotonic_ns', 'boottime_ns', 'elapsed_ms',
    'duration_ms', 'collection_duration_ns', 'timestamp', 'time', 'at', 'updated_at', 'checked_at', 'now', 'uptime_seconds',
    # statvfs() of the fixture copy measures the host's disk.
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


def rust_binary():
    value = os.environ.get(RUST_ENV)
    return Path(value) if value and Path(value).is_file() and os.access(value, os.X_OK) else None


def recording():
    return os.environ.get(RECORD_ENV) == '1'


def rust_side(binary=None):
    """A side: runs the Rust binary on `root` with the fixture contract above."""
    binary = binary or rust_binary()

    def run(root, argv, commands_file, calls_file, timeout, env, stdin=None):
        """`stdin`: a file the run inherits as its standard input (otherwise /dev/null)."""
        env = base_environment({**env, 'Y2_ROOT': str(root), 'Y2_FIXTURE_COMMANDS': str(commands_file),
                                'Y2_FIXTURE_CALLS': str(calls_file)})
        with open(stdin or os.devnull, 'rb') as given:
            return subprocess.run([str(binary), *argv], stdin=given, capture_output=True, timeout=timeout,
                                  env=env)
    return run


def normalise_json(value, volatile):
    if isinstance(value, dict):
        return {k: '<volatile>' if k in volatile or (k in VOLATILE_NUMBERS and isinstance(v, (int, float)))
                else normalise_json(v, volatile) for k, v in value.items()}
    if isinstance(value, list):
        return [normalise_json(v, volatile) for v in value]
    return value


def replace_paths(text, root):
    """The fixture copy's and the checkout's paths, and today's UTC date (the reports' daily counters)."""
    today = datetime.now(timezone.utc).date().isoformat()
    return (text.replace(str(root), '<root>').replace(str(REPOSITORY), '<repository>')
            .replace('"%s"' % today, '"<today>"'))


def normalise_text(text, root, volatile, patterns):
    text = replace_paths(text, root)
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
                tree[relative] = {'type': 'symlink', 'target': replace_paths(os.readlink(path), root)}
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


def tree_difference(before, after):
    """What changed between two snapshots: added and changed paths with their entries, removed paths."""
    return {'added': {p: e for p, e in after.items() if p not in before},
            'changed': {p: e for p, e in after.items() if p in before and before[p] != e},
            'removed': sorted(p for p in before if p not in after)}


def touched(result):
    """The paths a result added or changed, with their entries (for assertions on a golden)."""
    return {**result['tree']['added'], **result['tree']['changed']}


def write_files(root, files):
    """A case's `files` (path -> text, a function returning it, or None to remove it) below `root`."""
    for name, text in (files or {}).items():
        path = root / name.lstrip('/')
        text = text() if callable(text) else text  # made at run time (a clock reading)
        if text is None:
            path.unlink(missing_ok=True)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)


def run_case(fixture, argv, commands=(), timeout=60, volatile=(), patterns=(), env=None, files=None, wrap=None):
    """Run `argv` on a fresh copy of `fixture` (changed by `files`) and return the normalised result. `wrap`
    (side -> side) prepares the root or the run around the platform (peers, a first command, renames after);
    the tree difference is taken against the root as the platform's first run found it."""
    volatile = VOLATILE_KEYS | set(volatile)
    patterns = [(TRAIL_CLOCK, '<clock> '), *patterns]
    with tempfile.TemporaryDirectory(prefix='y2-golden-') as scratch:
        scratch = Path(scratch)
        root = scratch / 'root'
        shutil.copytree(fixture, root, symlinks=True)
        write_files(root, files)
        commands_file = scratch / 'commands.json'
        commands_file.write_text(json.dumps(list(commands)))
        calls_file = scratch / 'calls.jsonl'
        calls_file.touch()
        platform = rust_side()
        before = []
        # Records stamped with the clock are made again right before the platform runs: their age is then
        # milliseconds however busy the host is (the stamps are normalised, so the snapshot is unchanged).
        stamped = {name: text for name, text in (files or {}).items() if callable(text)}

        def first_found(root, *arguments, **options):
            if not before:
                before.append(snapshot(root, volatile, patterns))
                write_files(root, stamped)
            return platform(root, *arguments, **options)
        result = (wrap(first_found) if wrap else first_found)(root, argv, commands_file, calls_file, timeout,
                                                              env or {})
        # A timeout is recorded as given (`timeout=3` or the default 1.0): compare it as a number.
        calls = [normalise_json({**call, 'timeout': float(call['timeout'])}, volatile) for call in
                 (json.loads(replace_paths(line, root)) for line in calls_file.read_text().splitlines() if line)]
        after = snapshot(root, volatile, patterns)
        return {
            'argv': [a.replace(str(REPOSITORY), '<repository>') for a in argv],
            'exit': result.returncode,
            'stdout': normalise_text(result.stdout.decode('utf-8', errors='replace'), root, volatile, patterns),
            'calls': calls,
            'tree': tree_difference(before[0] if before else after, after),
        }


def differences(left, right, path=''):
    """Human-readable list of where two results differ (empty when equal)."""
    if type(left) is not type(right):
        return ['%s: %r != %r' % (path or '/', left, right)]
    if isinstance(left, dict):
        found = []
        for key in sorted(set(left) | set(right), key=str):
            if key not in left or key not in right:
                found.append('%s/%s: only in %s' % (path, key, 'golden' if key in left else 'run'))
            else:
                found.extend(differences(left[key], right[key], '%s/%s' % (path, key)))
        return found
    if isinstance(left, list) and len(left) == len(right):
        found = []
        for index, (a, b) in enumerate(zip(left, right)):
            found.extend(differences(a, b, '%s[%d]' % (path, index)))
        return found
    return [] if left == right else ['%s: %r != %r' % (path or '/', left, right)]


def golden_path(group, name):
    return GOLDENS / group / (name + '.json')


def encode(result):
    # ASCII escapes: arguments and outputs may hold lone surrogates (bytes that are not UTF-8).
    return json.dumps(result, indent=1, sort_keys=True) + '\n'


def read_golden(group, name):
    path = golden_path(group, name)
    return json.loads(path.read_text()) if path.exists() else None


def check(testcase, group, name, result):
    """Compare `result` with its golden, or (Y2_RECORD_GOLDENS=1) write the golden when it differs."""
    golden = read_golden(group, name)
    if recording():
        if golden != json.loads(encode(result)):
            path = golden_path(group, name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(encode(result))
            print('recorded %s' % path.relative_to(REPOSITORY))
        return
    testcase.assertIsNotNone(golden, 'no golden %s: record it with %s=1 and review it'
                             % (golden_path(group, name).relative_to(REPOSITORY), RECORD_ENV))
    testcase.assertEqual(differences(golden, json.loads(encode(result))), [])


def run_all(jobs, workers=None):
    """Run `jobs` ((key, function) pairs) in threads (each a subprocess on its own root); {key: result}.
    An exception is returned in place of the result."""
    def one(job):
        try:
            return job[1]()
        except Exception as error:  # reported by the case's subTest
            return error
    with ThreadPoolExecutor(workers or min(16, (os.cpu_count() or 2) * 2)) as pool:
        return dict(zip([key for key, _ in jobs], pool.map(one, jobs)))


def check_all(testcase, jobs, workers=None):
    """Run `jobs` (((group, name), function returning a result)) in parallel, check each against its golden."""
    for (group, name), result in run_all(jobs, workers).items():
        with testcase.subTest(name):
            if isinstance(result, Exception):
                raise result
            check(testcase, group, name, result)


def prune(names):
    """Goldens below GOLDENS that no case names ({group: set of names}); removed when recording."""
    found = []
    for path in sorted(GOLDENS.glob('*/*.json')):
        if path.stem not in names.get(path.parent.name, ()):
            found.append(str(path.relative_to(REPOSITORY)))
            if recording():
                path.unlink()
    return found
