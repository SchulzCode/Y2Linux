#!/usr/bin/env python3
"""Keep the Rust dispatcher's argument parser data consistent (PLATFORM-RUST-PORT-PLAN.md §3, §5a).

/usr/sbin/y2-platform parses its arguments itself (platform/crates/y2-platform/src/argparse.rs) the way the Python
platform's `main.py` did with its argparse parser. The parser is data, made once from that parser and edited by hand
since (the Python platform is gone):
  src/argparse.json        per parser (main.py and each subcommand): its actions (kind, dest, option strings,
                           nargs, type, choices, default, required, the name errors use), the option strings in
                           argparse's lookup order, prog, `main.py … --help` and the usage line errors print; one
                           parser per line (argparse.rs reads only the lines of main.py and of the subcommand called)
  src/argparse/unicode.rs  the Unicode tables of Python 3.15's int(), float(), repr() and re `\\d` the parser uses
An edit of argparse.json changes the argument golden (tests/test_platform_argparse.py; re-record it with
Y2_RECORD_GOLDENS=1 and review) and platform/contract.json's table (tools/development/platform-contract.py).
  platform-argparse.py --check   exit 1 when argparse.json is not in its line layout or not consistent (each
                                 parser's options index its actions, names, progs, usage and help agree,
                                 subcommands are main.py's choices) or unicode.rs's tables are not sorted ranges
  platform-argparse.py --write   rewrite argparse.json in its line layout (after a hand edit)
"""
import argparse
import json
from pathlib import Path
import re

REPOSITORY = Path(__file__).resolve().parents[2]
SOURCE = REPOSITORY / 'platform/crates/y2-platform/src'
SPEC = SOURCE / 'argparse.json'
UNICODE = SOURCE / 'argparse/unicode.rs'
KINDS = {'help', 'store', 'store_true', 'append', 'parsers'}
TYPES = {None, 'int', 'float'}
NARGS = {None, '?', '*', '+', '...', 'A...'}
KEYS = {'prog', 'usage', 'help', 'suggest_on_error', 'options', 'actions'}
ACTION_KEYS = {'kind', 'dest', 'options', 'nargs', 'type', 'choices', 'default', 'const', 'required', 'name'}


def layout(spec):
    """argparse.json's text: main.py's parser on the first line, then one subcommand per line."""
    def compact(value):
        return json.dumps(value, ensure_ascii=False, separators=(',', ':'))
    return '{"top":%s,\n"commands":[\n%s\n]}\n' % (compact(spec['top']), ',\n'.join(
        compact(command) for command in spec['commands']))


def parser_problems(name, parser, prog):
    found = []
    if set(parser) != KEYS:
        return ['%s: keys %s' % (name, sorted(parser))]
    if parser['prog'] != prog:
        found.append('%s: prog %r, not %r' % (name, parser['prog'], prog))
    if not parser['usage'].startswith('usage: ' + prog) or not parser['help'].startswith(parser['usage']):
        found.append('%s: usage/help do not start with "usage: %s"' % (name, prog))
    dests = [a['dest'] for a in parser['actions'] if a.get('dest') is not None]
    if len(dests) != len(set(dests)):
        found.append('%s: shared dest' % name)
    for index, action in enumerate(parser['actions']):
        where = '%s action %d' % (name, index)
        if set(action) != ACTION_KEYS:
            found.append('%s: keys %s' % (where, sorted(action)))
            continue
        if action['kind'] not in KINDS or action['type'] not in TYPES:
            found.append('%s: kind %r type %r not implemented' % (where, action['kind'], action['type']))
        if action['nargs'] not in NARGS and not isinstance(action['nargs'], int):
            found.append('%s: nargs %r' % (where, action['nargs']))
        if action['choices'] is not None and not all(
                isinstance(c, str) or (isinstance(c, dict) and set(c) == {'int'} and re.fullmatch(r'-?\d+', c['int']))
                for c in action['choices']):
            found.append('%s: choices %r' % (where, action['choices']))
        expected = '/'.join(action['options']) if action['options'] else action['dest']
        if action['name'] != expected:
            found.append('%s: name %r, not %r' % (where, action['name'], expected))
        for option in action['options']:
            if not option.startswith('-'):
                found.append('%s: option %r' % (where, option))
            if action['kind'] != 'help' and option not in parser['help']:
                found.append('%s: %s not in the help text' % (where, option))
    indexed = sorted((o, i) for o, i in parser['options'])
    defined = sorted((o, i) for i, a in enumerate(parser['actions']) for o in a.get('options', ()))
    if indexed != defined or len({o for o, _ in parser['options']}) != len(parser['options']):
        found.append('%s: options do not index the actions\' option strings once each' % name)
    return found


def problems(text):
    try:
        spec = json.loads(text)
    except ValueError as error:
        return ['argparse.json: %s' % error]
    if set(spec) != {'top', 'commands'}:
        return ['argparse.json: keys %s' % sorted(spec)]
    found = [] if text == layout(spec) else ['argparse.json: not in its line layout (run --write)']
    names = [name for name, _ in spec['commands']]
    found += parser_problems('main.py', spec['top'], 'main.py')
    subparsers = [a for a in spec['top']['actions'] if a.get('kind') == 'parsers']
    if len(subparsers) != 1 or subparsers[0]['choices'] != names:
        found.append('main.py: its subcommand choices are not the commands, in order')
    if len(set(names)) != len(names):
        found.append('argparse.json: a subcommand twice')
    for name, parser in spec['commands']:
        found += parser_problems(name, parser, 'main.py ' + name)
        if name not in spec['top']['usage']:
            found.append('main.py: %s not in its usage' % name)
        if any(a.get('kind') == 'parsers' for a in parser['actions']):
            found.append('%s: argparse.rs implements one level of subcommands' % name)
    return found


def unicode_problems(source):
    found = []
    for table in ('NONPRINTABLE', 'SPACE', 'DECIMAL_ZEROS'):
        match = re.search(r'pub const %s: &\[[^\]]+\] = &\[(.*?)\n\];' % table, source, re.S)
        if not match:
            found.append('unicode.rs: no table %s' % table)
            continue
        if table == 'NONPRINTABLE':
            ranges = [(int(a, 16), int(b, 16)) for a, b in re.findall(r'\(0x([0-9a-f]+), 0x([0-9a-f]+)\)', match[1])]
            if not ranges or any(a > b for a, b in ranges) or any(
                    b + 1 >= c for (_, b), (c, _) in zip(ranges, ranges[1:])):
                found.append('unicode.rs: NONPRINTABLE is not sorted, disjoint, merged ranges')
        else:
            points = [int(p, 16) for p in re.findall(r'0x([0-9a-f]+)', match[1])]
            if not points or points != sorted(set(points)):
                found.append('unicode.rs: %s is not sorted' % table)
            if table == 'DECIMAL_ZEROS' and (points[0] != 0x30 or any(b - a < 10 for a, b in zip(points, points[1:]))):
                found.append('unicode.rs: DECIMAL_ZEROS are not runs of ten digits from 0x30')
    return found


def main():
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = options.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    args = options.parse_args()
    text = SPEC.read_text()
    if args.write:
        SPEC.write_text(layout(json.loads(text)))
        return 0
    found = problems(text) + unicode_problems(UNICODE.read_text())
    for problem in found:
        print(problem)
    return 1 if found else 0


if __name__ == '__main__':
    raise SystemExit(main())
