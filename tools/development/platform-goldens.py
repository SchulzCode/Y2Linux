#!/usr/bin/env python3
"""The y2p-contract golden files: the platform's records the contract types must read and write back.

platform/crates/y2p-contract/tests/golden/<name>.json are what y2-platform prints or writes for the cases below
(tests/test_platform_differential.py's fixture, root path replaced by `/`); the contract types must read and write
them back unchanged (`cargo test` in platform/, crates/y2p-contract/tests/golden.rs). They are first recorded
from the Python platform, like the platform goldens of tests/platform_goldens/base, which they must agree with.
  platform-goldens.py           rewrite them from the Rust host build (Y2_PLATFORM_RUST, or platform/target/debug)
  platform-goldens.py --check   exit 1 when one differs from its platform golden (clock readings aside); needs
                                no binary
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

REPOSITORY = Path(__file__).resolve().parents[2]
GOLDEN = REPOSITORY / 'platform/crates/y2p-contract/tests/golden'
sys.path.insert(0, str(REPOSITORY / 'tests'))
from platform_golden import VOLATILE_KEYS, normalise_json, read_golden, rust_side, touched  # noqa: E402

# golden name: (platform golden in tests/platform_goldens/base, argv, file the record is read from instead of stdout)
CASES = {
    'capabilities': ('capabilities', ['capabilities'], None),
    'health': ('health', ['health'], None),
    'status': ('status', ['status'], None),
    'status-wifi': ('status-wifi', ['status', 'wifi'], None),
    'boot-record': ('boot-stage', ['boot-stage', 'platform_start'], None),
    'boot-record-file': ('boot-stage', ['boot-stage', 'platform_start'], '/data/system/platform/boot.json'),
    'boot-stage-line': ('boot-stage', ['boot-stage', 'platform_start'], '/run/y2/boot-stages.jsonl'),
    'error': ('boot-stage-unknown', ['boot-stage', 'no_such_stage'], None),
}


def comparable(value):
    """A record with the clock readings and the root path normalised (`<root>` in a platform golden)."""
    return normalise_json(json.loads(json.dumps(value).replace('<root>', '')), VOLATILE_KEYS)


def recorded(case, source):
    golden = read_golden('base', case)
    if golden is None:
        return None
    return touched(golden)[source]['content'] if source else golden['stdout']


def generate():
    from test_platform_differential import player_root
    binary = os.environ.get('Y2_PLATFORM_RUST') or str(REPOSITORY / 'platform/target/debug/y2-platform')
    side = rust_side(Path(binary))
    with tempfile.TemporaryDirectory(prefix='y2-golden-') as scratch:
        scratch = Path(scratch)
        fixture = player_root(scratch / 'fixture')
        (scratch / 'commands.json').write_text('[]')
        goldens = {}
        for name, (_, argv, source) in CASES.items():
            root = scratch / name
            shutil.copytree(fixture, root, symlinks=True)
            result = side(root, argv, scratch / 'commands.json', scratch / 'calls', 60, {})
            text = (root / source.lstrip('/')).read_text() if source else result.stdout.decode()
            goldens[name] = json.loads(text.replace(str(root), ''))
        return goldens


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    options.add_argument('--check', action='store_true')
    args = options.parse_args()
    if args.check:
        stale = [name for name, (case, _, source) in CASES.items()
                 if not (GOLDEN / f'{name}.json').exists() or recorded(case, source) is None or
                 comparable(json.loads((GOLDEN / f'{name}.json').read_text())) != comparable(recorded(case, source))]
        if stale:
            print('y2p-contract goldens differ from tests/platform_goldens/base: %s; run %s and review'
                  % (', '.join(stale), Path(__file__).name))
            return 1
        return 0
    GOLDEN.mkdir(parents=True, exist_ok=True)
    for name, value in generate().items():
        (GOLDEN / f'{name}.json').write_text(json.dumps(value, indent=1, sort_keys=True) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
