#!/usr/bin/env python3
"""Export the golden cases Reborn depends on into Y2Reborn's test data, where reborn-platform's contract test
parses each with Reborn's own parsers and compares the typed result with the one recorded before
(PLATFORM-RUST-IDIOMATIC-PLAN.md §2.3): a platform change that keeps Reborn's view passes, one that changes what
Reborn reads fails there.

Picked: every case whose command line is one Reborn runs (crates/reborn-platform/src/client.rs `call`) and every
file a case wrote that Reborn reads (by path), as the golden suite normalised them.

usage: reborn-contract.py [<Y2Reborn checkout>]   (default: ../Y2Reborn next to this repository)
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GOLDENS = REPO / 'tests' / 'platform_goldens'
REBORN = Path(sys.argv[1]) if len(sys.argv) > 1 else REPO.parent / 'Y2Reborn'
OUT = REBORN / 'crates' / 'reborn-platform' / 'tests' / 'platform-contract.json'

# The first words of each command Reborn runs.
COMMANDS = [
    ['status'], ['capabilities'], ['health'], ['sleep', 'request'], ['report'], ['update', 'check'],
    ['update', 'apply'], ['update', 'cancel'], ['update', 'rollback'], ['update', 'stage'], ['media', 'repair'],
    ['internal-media', 'setup'], ['export-state'], ['export-diagnostics'], ['codec-settings'], ['time-set'],
    ['bench-storage'], ['bench-library'],
]
# The platform's files Reborn reads.
FILES = [
    '/run/y2/usb-transfer.json', '/run/y2/media-mount.json', '/run/y2/network.json', '/run/y2/power.json',
    '/run/y2/time-status.json', '/run/y2/internal-media.json', '/run/y2/coexistence.json',
    '/run/y2/codec-runtime.json', '/run/y2/application-ready.json', '/run/y2/update-progress.json',
    '/run/y2/report-progress.json', '/run/y2/shutdown.json', '/run/y2/bt-codec-uncertain.json',
    '/data/bluetooth/codec-history.json', '/data/bluetooth/codec-policy.json',
]


def wanted(argv):
    return any(argv[:len(c)] == c for c in COMMANDS)


def tree_files(tree):
    """The files Reborn reads that the case wrote (added or changed): {path: content}."""
    found = {}
    for kind in ('added', 'changed'):
        for path, entry in (tree or {}).get(kind, {}).items():
            if path in FILES and isinstance(entry, dict) and 'content' in entry:
                found[path] = entry['content']
    return found


def main():
    cases = []
    for path in sorted(GOLDENS.glob('*/*.json')):
        golden = json.loads(path.read_text())
        argv = [str(a) for a in golden.get('argv', [])]
        files = tree_files(golden.get('tree'))
        if not wanted(argv) and not files:
            continue
        cases.append({'name': f'{path.parent.name}/{path.stem}', 'argv': argv, 'exit': golden.get('exit'),
                      'stdout': golden.get('stdout') if wanted(argv) else None, 'files': files})
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(cases, indent=1, sort_keys=True) + '\n')
    print(f'{OUT}: {len(cases)} cases')


main()
