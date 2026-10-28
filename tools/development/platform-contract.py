#!/usr/bin/env python3
"""Keep platform/contract.json's argument table equal to the Python CLI's parser (PLATFORM-RUST-PORT-PLAN.md §5).

The inventory entries in platform/contract.json (files, locks, timeouts, subprocesses, F-numbers per subcommand)
are reviewed by hand; the argument table is generated from tools/platform/y2_platform/cli.py so it cannot drift.
  platform-contract.py --check   exit 1 when the table differs from the parser
  platform-contract.py --write   rewrite the table
"""
import argparse
import json
from pathlib import Path
import sys

REPOSITORY = Path(__file__).resolve().parents[2]
CONTRACT = REPOSITORY / 'platform/contract.json'


def parser_table():
    """Every subcommand's arguments, read from the parser cli.main() builds (no handler runs)."""
    sys.path.insert(0, str(REPOSITORY / 'tools/platform'))
    from y2_platform import cli

    class Built(Exception):
        pass

    def capture(parser, *_, **__):
        raise Built(parser)

    original = argparse.ArgumentParser.parse_args
    argparse.ArgumentParser.parse_args = capture
    try:
        cli.main()
    except Built as built:
        parser = built.args[0]
    finally:
        argparse.ArgumentParser.parse_args = original
    table = {}
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            for name, sub in action.choices.items():
                table[name] = [describe(a) for a in sub._actions if not isinstance(a, argparse._HelpAction)]
    return table


def describe(action):
    kind = ('flag' if isinstance(action, argparse._StoreTrueAction) else
            'append' if isinstance(action, argparse._AppendAction) else
            getattr(action.type, '__name__', 'str') if action.type else 'str')
    return {'name': '/'.join(action.option_strings) or action.dest, 'positional': not action.option_strings,
            'type': kind, 'choices': list(action.choices) if action.choices else None,
            'default': action.default, 'required': action.required, 'nargs': action.nargs, 'help': action.help}


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = options.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    args = options.parse_args()
    contract = json.loads(CONTRACT.read_text())
    table = parser_table()
    if args.check:
        if contract.get('arguments') != table:
            changed = sorted(set(table) ^ set(contract.get('arguments', {})) |
                             {k for k in table if contract.get('arguments', {}).get(k) not in (None, table[k])})
            print('platform/contract.json arguments differ from cli.py for: %s; run %s --write and review the '
                  'inventory entries of those subcommands' % (', '.join(changed), Path(__file__).name))
            return 1
        return 0
    contract['arguments'] = table
    CONTRACT.write_text(json.dumps(contract, indent=1, ensure_ascii=False) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
