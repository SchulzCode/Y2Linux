#!/usr/bin/env python3
"""Keep platform/contract.json's argument table equal to the platform's parser (PLATFORM-RUST-PORT-PLAN.md §5).

The inventory entries in platform/contract.json (files, locks, timeouts, subprocesses, F-numbers per subcommand)
are reviewed by hand; the argument table is the parser's own table (platform/crates/y2-platform/src/cli.rs
`COMMANDS`, with each argument's help), which the binary prints as `y2-platform __arguments`, so it cannot drift.
`cargo test` compares the same (cli.rs `contract_json_holds_this_table`).
  platform-contract.py --check   exit 1 when the table differs from the binary's
  platform-contract.py --write   rewrite the table from the binary, then review the inventory entries of the
                                 subcommands that changed
The binary is $Y2_PLATFORM_RUST, else platform/target/debug/y2-platform (cd platform && cargo build --offline
--locked). Without one (the release container has no cargo) --check says so and passes: cargo test checks it.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

REPOSITORY = Path(__file__).resolve().parents[2]
CONTRACT = REPOSITORY / 'platform/contract.json'
DEFAULT_BINARY = REPOSITORY / 'platform/target/debug/y2-platform'
KEYS = ['name', 'positional', 'type', 'choices', 'default', 'required', 'nargs', 'help']


def binary():
    path = Path(os.environ.get('Y2_PLATFORM_RUST') or DEFAULT_BINARY)
    return path if path.is_file() else None


def parser_table(path):
    """The binary's table, each argument's keys in contract.json's order."""
    output = subprocess.run([str(path), '__arguments'], capture_output=True, check=True, timeout=30,
                            env={'PATH': '/usr/bin:/bin'}).stdout
    table = json.loads(output)
    return {name: [{key: argument[key] for key in KEYS} for argument in arguments]
            for name, arguments in table.items()}


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = options.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    args = options.parse_args()
    path = binary()
    if path is None:
        print('no y2-platform host build (Y2_PLATFORM_RUST or %s): not compared' % DEFAULT_BINARY.relative_to(REPOSITORY))
        return 0 if args.check else 1
    contract = json.loads(CONTRACT.read_text())
    current = contract.get('arguments', {})
    table = parser_table(path)
    if args.check:
        changed = sorted(set(table) ^ set(current) | {k for k in table if current.get(k) not in (None, table[k])})
        if changed:
            print('platform/contract.json arguments differ from the parser for: %s; run %s --write and review the '
                  'inventory entries of those subcommands' % (', '.join(changed), Path(__file__).name))
            return 1
        return 0
    contract['arguments'] = table
    CONTRACT.write_text(json.dumps(contract, indent=1, ensure_ascii=False) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
