#!/usr/bin/env python3
"""Keep platform/contract.json's argument table equal to the platform's parser (PLATFORM-RUST-PORT-PLAN.md §5).

The inventory entries in platform/contract.json (files, locks, timeouts, subprocesses, F-numbers per subcommand)
are reviewed by hand; the argument table follows the parser the Rust dispatcher embeds,
platform/crates/y2-platform/src/argparse.json, so it cannot drift. argparse.json has no per-argument help: an
argument's `help` must appear in its subcommand's help text instead.
  platform-contract.py --check   exit 1 when the table differs from argparse.json
  platform-contract.py --write   rewrite the table (an argument's help is kept by name; a new one's is null)
"""
import argparse
import json
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
CONTRACT = REPOSITORY / 'platform/contract.json'
SPEC = REPOSITORY / 'platform/crates/y2-platform/src/argparse.json'


def value(typed):
    """argparse.json's tagged value as the table writes it (int and float untagged)."""
    if isinstance(typed, dict) and set(typed) == {'int'}:
        return int(typed['int'])
    if isinstance(typed, dict) and set(typed) == {'float'}:
        return float(typed['float'])
    if isinstance(typed, list):
        return [value(v) for v in typed]
    return typed


def describe(action, help_text):
    kind = {'store_true': 'flag', 'append': 'append'}.get(action['kind'], action['type'] or 'str')
    return {'name': '/'.join(action['options']) or action['dest'], 'positional': not action['options'],
            'type': kind, 'choices': [value(c) for c in action['choices']] if action['choices'] else None,
            'default': value(action['default']), 'required': action['required'], 'nargs': action['nargs'],
            'help': help_text}


def parser_table(helps):
    """Every subcommand's arguments from argparse.json; `helps` gives each argument's help ({(command, name)})."""
    spec = json.loads(SPEC.read_text())
    return {name: [describe(a, helps.get((name, '/'.join(a['options']) or a['dest'])))
                   for a in parser['actions'] if a['kind'] != 'help'] for name, parser in spec['commands']}, \
        dict(spec['commands'])


def unshown_helps(table, parsers):
    """Arguments whose help is not in their subcommand's help text (argparse wraps it: spaces compared loosely)."""
    found = []
    for name, arguments in table.items():
        shown = ' '.join(parsers[name]['help'].split())
        for argument in arguments:
            if argument['help'] is not None and ' '.join(argument['help'].split()) not in shown:
                found.append('%s %s' % (name, argument['name']))
    return found


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = options.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    args = options.parse_args()
    contract = json.loads(CONTRACT.read_text())
    current = contract.get('arguments', {})
    helps = {(name, a['name']): a['help'] for name, arguments in current.items() for a in arguments}
    table, parsers = parser_table(helps)
    if args.check:
        changed = sorted(set(table) ^ set(current) | {k for k in table if current.get(k) not in (None, table[k])})
        if changed:
            print('platform/contract.json arguments differ from argparse.json for: %s; run %s --write and review the '
                  'inventory entries of those subcommands' % (', '.join(changed), Path(__file__).name))
            return 1
        unshown = unshown_helps(table, parsers)
        if unshown:
            print('platform/contract.json help not in the subcommand\'s help text (argparse.json): %s'
                  % ', '.join(unshown))
            return 1
        return 0
    contract['arguments'] = table
    CONTRACT.write_text(json.dumps(contract, indent=1, ensure_ascii=False) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
