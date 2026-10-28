#!/usr/bin/env python3
"""Keep the Rust dispatcher's argument parser equal to the Python CLI's (PLATFORM-RUST-PORT-PLAN.md §3, §5a).

/usr/sbin/y2-platform parses its arguments itself (platform/crates/y2-platform/src/argparse.rs) the way
`python3 -I /usr/lib/y2-platform/main.py` does with the parser tools/platform/y2_platform/cli.py builds. Two files
are this parser as data, both made by the real parser, run with sys.argv[0] = 'main.py', no terminal and no COLUMNS
(as on the player, where the platform's callers read its output through pipes):
  src/argparse.json        per parser (main.py and each subcommand): its actions (kind, dest, option strings,
                           nargs, type, choices, default, required, the name errors use), the option strings in
                           argparse's lookup order, prog, `main.py … --help` and the usage line errors print
  src/argparse/unicode.rs  the Unicode tables of Python's int(), float(), repr() and re `\\d` the parser uses

The reference interpreter is the player's Python 3.15 (Y2_PLAYER_PYTHON, else the build's host python3.15 under
out/build/buildroot/host/bin of this checkout or of the main checkout). Without it the host python3 runs the parser
with the 3.15 argparse default that changes this parser's behaviour (suggest_on_error=True: "maybe you meant"); the
Unicode tables are then left alone (another Unicode version) and not checked.
  platform-argparse.py --check        exit 1 when a file differs from the parser
  platform-argparse.py --write        rewrite them
  platform-argparse.py --reference    (tests) read a JSON list of argument lists on stdin and print, per list, what
                                      main.py's parser does: exit code, stdout, stderr and the parsed namespace
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import sys

REPOSITORY = Path(__file__).resolve().parents[2]
SOURCE = REPOSITORY / 'platform/crates/y2-platform/src'
SPEC = SOURCE / 'argparse.json'
UNICODE = SOURCE / 'argparse/unicode.rs'
PLAYER = (3, 15)
PLAYER_BINARY = 'out/build/buildroot/host/bin/python3.15'


def reference_python():
    """The player's interpreter, if this host has one; otherwise None."""
    named = os.environ.get('Y2_PLAYER_PYTHON')
    if named:
        return named
    roots = [REPOSITORY]
    try:
        common = subprocess.run(['git', '-C', str(REPOSITORY), 'rev-parse', '--git-common-dir'],
                                capture_output=True, text=True, check=True).stdout.strip()
        roots.append((REPOSITORY / common).resolve().parent)  # a worktree's main checkout
    except (OSError, subprocess.CalledProcessError):
        pass
    for root in roots:
        candidate = root / PLAYER_BINARY
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
    return None


def inner(mode, stdin=None):
    """Run this file's `mode` in the reference interpreter (or this one) as the player would: no terminal, no
    COLUMNS, no colour variables, isolated mode."""
    python = reference_python() or sys.executable
    environment = {'PATH': os.environ.get('PATH', '/usr/bin:/bin')}
    result = subprocess.run([python, '-I', str(Path(__file__).resolve()), mode], input=stdin,
                            capture_output=True, text=True, env=environment)
    if result.returncode != 0:
        raise SystemExit('%s %s failed:\n%s' % (python, mode, result.stderr))
    return json.loads(result.stdout)


# ---- runs inside the reference interpreter ----

def built_parser():
    """The parser cli.main() builds (no handler runs), with prog 'main.py' as on the player."""
    sys.argv[0] = 'main.py'
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
    parsers = [parser] + [sub for action in parser._actions if isinstance(action, argparse._SubParsersAction)
                          for sub in action.choices.values()]
    if sys.version_info[:2] < PLAYER:
        for each in parsers:
            each.suggest_on_error = True  # the 3.15 default
    return parser


def typed(value):
    """A parsed value as JSON: int and float tagged (JSON has no inf/nan and no int/float distinction)."""
    if value is argparse.SUPPRESS:
        return {'suppress': True}
    if value is None or isinstance(value, (bool, str)):
        return value
    if isinstance(value, int):
        return {'int': str(value)}
    if isinstance(value, float):
        return {'float': repr(value)}
    if isinstance(value, list):
        return [typed(v) for v in value]
    if isinstance(value, dict):
        return {k: typed(v) for k, v in value.items()}
    raise TypeError('unsupported value %r' % (value,))


KINDS = {argparse._HelpAction: 'help', argparse._StoreAction: 'store', argparse._StoreTrueAction: 'store_true',
         argparse._AppendAction: 'append', argparse._SubParsersAction: 'parsers'}
TYPES = {None: None, int: 'int', float: 'float'}


def plain(text):
    # Names, option strings and choices are read without unescaping (borrowed from the embedded text).
    if json.dumps(text, ensure_ascii=False)[1:-1] != text:
        raise ValueError('%r needs JSON escapes' % text)
    return text


def describe_action(action):
    if type(action) not in KINDS or action.type not in TYPES or getattr(action, 'deprecated', False):
        raise ValueError('argparse.rs does not implement %r' % (action,))
    if action.nargs not in (None, '?', '*', '+', '...', 'A...') and not isinstance(action.nargs, int):
        raise ValueError('nargs %r' % (action.nargs,))
    choices = list(action.choices) if action.choices is not None else None
    if choices is not None and not all(isinstance(c, (str, int)) and not isinstance(c, bool) for c in choices):
        raise ValueError('choices %r' % (choices,))
    return {
        'kind': KINDS[type(action)],
        'dest': None if action.dest is argparse.SUPPRESS else plain(action.dest),
        'options': [plain(o) for o in action.option_strings],
        'nargs': action.nargs,
        'type': TYPES[action.type],
        'choices': [plain(c) if isinstance(c, str) else typed(c) for c in choices] if choices is not None else None,
        'default': typed(action.default),
        'const': typed(action.const),
        'required': action.required,
        'name': argparse._get_action_name(action),
    }


def describe_parser(parser):
    if (parser.prefix_chars != '-' or not parser.allow_abbrev or not parser.exit_on_error or
            parser.fromfile_prefix_chars or parser._mutually_exclusive_groups or parser._defaults or
            parser._has_negative_number_optionals or parser.argument_default is not None):
        raise ValueError('argparse.rs does not implement the settings of %s' % parser.prog)
    dests = [a.dest for a in parser._actions if a.dest is not argparse.SUPPRESS]
    if len(dests) != len(set(dests)):
        raise ValueError('%s: shared dest' % parser.prog)
    index = {id(action): i for i, action in enumerate(parser._actions)}
    return {
        'prog': parser.prog,
        'usage': parser.format_usage(),
        'help': parser.format_help(),
        'suggest_on_error': parser.suggest_on_error,
        'options': [[plain(option), index[id(action)]] for option, action in parser._option_string_actions.items()],
        'actions': [describe_action(a) for a in parser._actions],
    }


def ranges(flags):
    """[[first, last], …] of the code points where flags[c] is true."""
    found, start = [], None
    for c, flag in enumerate(flags + [False]):
        if flag and start is None:
            start = c
        elif not flag and start is not None:
            found.append([start, c - 1])
            start = None
    return found


def extract():
    import unicodedata
    parser = built_parser()
    subparsers = [a for a in parser._actions if isinstance(a, argparse._SubParsersAction)]
    if len(subparsers) != 1 or any(isinstance(a, argparse._SubParsersAction)
                                   for sub in subparsers[0].choices.values() for a in sub._actions):
        raise ValueError('argparse.rs implements one level of subcommands')
    spec = {'top': describe_parser(parser),
            'commands': [[plain(name), describe_parser(sub)] for name, sub in subparsers[0].choices.items()]}
    every = [chr(c) for c in range(0x110000)]
    zeros = [c for c in range(0x110000) if unicodedata.decimal(every[c], None) == 0]
    for zero in zeros:
        assert all(unicodedata.decimal(every[zero + d], None) == d for d in range(10))
    assert sum(1 for ch in every if unicodedata.decimal(ch, None) is not None) == 10 * len(zeros)
    assert all(bool(__import__('re').match(r'\d', ch)) == (unicodedata.decimal(ch, None) is not None) for ch in every)
    unicode = {'version': unicodedata.unidata_version, 'python': list(sys.version_info[:2]),
               'nonprintable': ranges([not ch.isprintable() for ch in every]),
               'space': [c for c in range(0x110000) if every[c].isspace()], 'zeros': zeros}
    print(json.dumps({'spec': spec, 'unicode': unicode}))


def reference():
    """main.py's parse of each argument list on stdin: what it prints and exits with, or the namespace."""
    parser = built_parser()
    outcomes = []
    for argv in json.load(sys.stdin):
        out, err = io.StringIO(), io.StringIO()
        saved = sys.stdout, sys.stderr
        sys.stdout, sys.stderr = out, err
        try:
            namespace = parser.parse_args(argv)
            code = 0
        except SystemExit as stop:
            namespace, code = None, stop.code
        finally:
            sys.stdout, sys.stderr = saved
        outcomes.append({'exit': code, 'stdout': out.getvalue(), 'stderr': err.getvalue(),
                         'namespace': None if namespace is None else typed(vars(namespace))})
    print(json.dumps(outcomes))


# ---- the files ----

def unicode_source(table):
    def rows(items, per):
        return ''.join('\n   ' + ''.join(' %s,' % item for item in items[i:i + per]) for i in range(0, len(items), per))
    return ('// Generated by tools/development/platform-argparse.py --write from Python %d.%d (unicodedata %s);\n'
            '// do not edit. The player\'s Python decides: str.isprintable() (repr), str.isspace() and the\n'
            '// decimal digits int(), float() and re `\\d` accept.\n'
            '// SPDX-License-Identifier: GPL-2.0-only\n\n'
            '/// Code points `repr()` escapes (not str.isprintable()), as inclusive ranges.\n'
            'pub const NONPRINTABLE: &[(u32, u32)] = &[%s\n];\n\n'
            '/// str.isspace(), which int() and float() turn into a space before parsing.\n'
            'pub const SPACE: &[u32] = &[%s\n];\n\n'
            '/// The zero of each run of ten decimal digits (Unicode Nd).\n'
            'pub const DECIMAL_ZEROS: &[u32] = &[%s\n];\n' % (
                *table['python'], table['version'],
                rows(['(0x%x, 0x%x)' % tuple(r) for r in table['nonprintable']], 4),
                rows(['0x%x' % c for c in table['space']], 8), rows(['0x%x' % c for c in table['zeros']], 8)))


def main():
    if sys.argv[1:] == ['--extract']:
        return extract()
    if sys.argv[1:] == ['--inner-reference']:
        return reference()
    options = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = options.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    mode.add_argument('--reference', action='store_true')
    args = options.parse_args()
    if args.reference:
        print(json.dumps(inner('--inner-reference', sys.stdin.read())))
        return 0
    found = inner('--extract')
    def compact(value):
        return json.dumps(value, ensure_ascii=False, separators=(',', ':'))
    # One parser per line: argparse.rs reads only the lines of main.py and of the subcommand called.
    spec = '{"top":%s,\n"commands":[\n%s\n]}\n' % (compact(found['spec']['top']), ',\n'.join(
        compact(command) for command in found['spec']['commands']))
    table = found['unicode']
    player = tuple(table['python']) == PLAYER
    if args.write:
        SPEC.write_text(spec)
        if player:
            UNICODE.parent.mkdir(exist_ok=True)
            UNICODE.write_text(unicode_source(table))
        else:
            print('%s: kept (Python %d.%d is not the player\'s %d.%d)' % (UNICODE.name, *table['python'], *PLAYER),
                  file=sys.stderr)
        return 0
    stale = [path.relative_to(REPOSITORY) for path, text, checked in
             [(SPEC, spec, True), (UNICODE, unicode_source(table), player)]
             if checked and (not path.exists() or path.read_text() != text)]
    if stale:
        print('%s differ from cli.py\'s parser; run %s --write' % (', '.join(map(str, stale)), Path(__file__).name))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
