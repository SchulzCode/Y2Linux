#!/usr/bin/env python3
"""Can an older build still read what this one writes on /data (PLATFORM-RUST-IDIOMATIC-PLAN.md §1.3, §2.3)?

Every golden case's /data records (files it added or changed below /data) are compared with the same case at
`--against REV` (default: the last test build's release commit, `git log -1 --grep '^release:'`): each field the
old record had must still be there with the same JSON type (a number stays a number; null is compatible with any
type, as the readers treat a missing value); new fields are fine. A difference is printed as
`case: file: path: old-type -> new-type` and the exit status is 1.

Reviewed exceptions are listed in record-compat-allow.txt next to this script (one printed line each, as
`case: file: path: old-type -> new-type`, with a comment why): a fixture that wrote a field of another type
than its writer writes, kept as read by the old build and repaired by the new one.

usage: record-compat.py [--against REV]
"""
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GOLDENS = 'tests/platform_goldens'
args = sys.argv[1:]
if '--against' in args:
    against = args[args.index('--against') + 1]
else:
    against = subprocess.run(['git', 'log', '-1', '--format=%H', '--grep', '^release:'], cwd=REPO,
                             capture_output=True, text=True).stdout.strip()


def kind(value):
    if value is None:
        return 'null'
    if isinstance(value, bool):
        return 'bool'
    if isinstance(value, (int, float)):
        return 'number'
    if isinstance(value, str):
        return 'text'
    return 'list' if isinstance(value, list) else 'object'


def records(golden):
    found = {}
    for change in ('added', 'changed'):
        for path, entry in (golden or {}).get('tree', {}).get(change, {}).items():
            if path.startswith('/data/') and isinstance(entry, dict) and 'content' in entry:
                found[path] = entry['content']
    return found


def lost(old, new, path=''):
    """Fields of `old` that `new` dropped or retyped."""
    if isinstance(old, dict):
        if not isinstance(new, dict):
            if new is not None:
                yield path or '.', 'object', kind(new)
            return
        for key, value in old.items():
            where = f'{path}.{key}' if path else key
            if key not in new:
                yield where, kind(value), '(absent)'
            else:
                yield from lost(value, new[key], where)
    elif isinstance(old, list) and isinstance(new, list):
        for a, b in zip(old, new):
            yield from lost(a, b, path + '[]')
    elif not hidden(old) and not hidden(new) and kind(old) != kind(new):
        yield path or '.', kind(old), kind(new)


def hidden(value):
    """No value to compare: null (the readers treat it as missing) or the golden suite's `<volatile>`, which
    stands for a host reading (a clock, the fixture copy's stat()) and not for anything the writer wrote."""
    return value is None or value == '<volatile>'


def allowed():
    path = Path(__file__).with_name('record-compat-allow.txt')
    if not path.exists():
        return set()
    return {line.strip() for line in path.read_text().splitlines() if line.strip() and not line.startswith('#')}


def main():
    failures = 0
    allow = allowed()
    for file in sorted((REPO / GOLDENS).glob('*/*.json')):
        relative = file.relative_to(REPO).as_posix()
        shown = subprocess.run(['git', 'show', f'{against}:{relative}'], cwd=REPO, capture_output=True)
        if shown.returncode != 0:
            continue
        old, new = records(json.loads(shown.stdout)), records(json.loads(file.read_text()))
        for path, content in old.items():
            if path not in new:
                continue  # the case no longer writes it: its own golden review says why
            for where, a, b in lost(content, new[path]):
                line = f'{file.parent.name}/{file.stem}: {path}: {where}: {a} -> {b}'
                if line in allow:
                    continue
                print(line)
                failures += 1
    print(f'{failures} /data record fields lost or retyped against {against[:12]}', file=sys.stderr)
    sys.exit(1 if failures else 0)


main()
