#!/usr/bin/env python3
"""What a golden re-record changed in meaning (PLATFORM-RUST-IDIOMATIC-PLAN.md §2.2): every golden in the working
tree compared with the committed one (or `--against REV`), as parsed JSON, one line per changed value
(`case: path: old -> new`). Formatting cannot show up here (the goldens hold parsed values); a list of the cases
that changed, then their differences.

usage: golden-diff.py [--against REV] [--summary]
"""
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GOLDENS = 'tests/platform_goldens'
args = sys.argv[1:]
against = args[args.index('--against') + 1] if '--against' in args else 'HEAD'
summary = '--summary' in args


def committed(path):
    shown = subprocess.run(['git', 'show', f'{against}:{path}'], cwd=REPO, capture_output=True)
    return json.loads(shown.stdout) if shown.returncode == 0 else None


def short(value):
    text = json.dumps(value, sort_keys=True)
    return text if len(text) <= 160 else text[:157] + '...'


def differences(old, new, path=''):
    if isinstance(old, dict) and isinstance(new, dict):
        for key in sorted(set(old) | set(new)):
            where = f'{path}.{key}' if path else key
            if key not in old:
                yield where, '(absent)', short(new[key])
            elif key not in new:
                yield where, short(old[key]), '(absent)'
            else:
                yield from differences(old[key], new[key], where)
    elif isinstance(old, list) and isinstance(new, list) and len(old) == len(new):
        for index, (a, b) in enumerate(zip(old, new)):
            yield from differences(a, b, f'{path}[{index}]')
    elif old != new or type(old) is not type(new):
        yield path, short(old), short(new)


def main():
    listed = subprocess.run(['git', 'ls-files', '--others', '--modified', '--exclude-standard', '--', GOLDENS],
                            cwd=REPO, capture_output=True, text=True).stdout.split()
    tracked = subprocess.run(['git', 'diff', '--name-only', against, '--', GOLDENS], cwd=REPO,
                             capture_output=True, text=True).stdout.split()
    changed = sorted(set(listed) | set(tracked))
    total = 0
    for path in changed:
        if not path.endswith('.json'):
            continue
        name = path[len(GOLDENS) + 1:-5]
        file = REPO / path
        new = json.loads(file.read_text()) if file.exists() else None
        old = committed(path)
        if old is None or new is None:
            print(f'{name}: {"added" if old is None else "removed"}')
            total += 1
            continue
        found = list(differences(old, new))
        if not found:
            continue
        total += 1
        if summary:
            print(f'{name}: {len(found)} values')
            continue
        for where, a, b in found:
            print(f'{name}: {where}: {a} -> {b}')
    print(f'{total} goldens differ from {against}', file=sys.stderr)


main()
