#!/usr/bin/env python3
"""Write the Python platform's output on the differential fixture as y2p-contract golden files.

platform/crates/y2p-contract/tests/golden/<name>.json are what the Python platform prints or writes for
the cases below (tests/test_platform_differential.py's fixture, root path replaced by `/`); the Rust
contract types must read and write them back unchanged (tests/golden.rs).
  platform-goldens.py           rewrite them
  platform-goldens.py --check   exit 1 when they no longer match the Python platform (clock readings aside)
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPOSITORY = Path(__file__).resolve().parents[2]
GOLDEN = REPOSITORY / 'platform/crates/y2p-contract/tests/golden'
sys.path.insert(0, str(REPOSITORY / 'tests'))
from platform_differential import LAUNCHER, VOLATILE_KEYS, normalise_json  # noqa: E402
from test_platform_differential import player_root  # noqa: E402

# golden name: (argv, file the record is read from instead of stdout, or None)
CASES = {
    'capabilities': (['capabilities'], None),
    'health': (['health'], None),
    'status': (['status'], None),
    'status-wifi': (['status', 'wifi'], None),
    'boot-record': (['boot-stage', 'platform_start'], None),
    'boot-record-file': (['boot-stage', 'platform_start'], 'data/system/platform/boot.json'),
    'boot-stage-line': (['boot-stage', 'platform_start'], 'run/y2/boot-stages.jsonl'),
    'error': (['boot-stage', 'no_such_stage'], None),
}


def generate():
    with tempfile.TemporaryDirectory(prefix='y2-golden-') as scratch:
        scratch = Path(scratch)
        fixture = player_root(scratch / 'fixture')
        (scratch / 'commands.json').write_text('[]')
        goldens = {}
        for name, (argv, source) in CASES.items():
            root = scratch / name
            shutil.copytree(fixture, root, symlinks=True)
            result = subprocess.run([sys.executable, '-I', str(LAUNCHER), '--root', str(root), '--commands',
                                     str(scratch / 'commands.json'), '--calls', str(scratch / 'calls'), '--', *argv],
                                    capture_output=True, text=True, check=False)
            text = (root / source).read_text() if source else result.stdout
            goldens[name] = json.loads(text.replace(str(root), ''))
        return goldens


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    options.add_argument('--check', action='store_true')
    args = options.parse_args()
    goldens = generate()
    if args.check:
        stale = [name for name, value in goldens.items()
                 if not (GOLDEN / f'{name}.json').exists() or
                 normalise_json(json.loads((GOLDEN / f'{name}.json').read_text()), VOLATILE_KEYS) !=
                 normalise_json(value, VOLATILE_KEYS)]
        if stale:
            print('y2p-contract goldens differ from the Python platform: %s; run %s and review'
                  % (', '.join(stale), Path(__file__).name))
            return 1
        return 0
    GOLDEN.mkdir(parents=True, exist_ok=True)
    for name, value in goldens.items():
        (GOLDEN / f'{name}.json').write_text(json.dumps(value, indent=1, sort_keys=True) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
