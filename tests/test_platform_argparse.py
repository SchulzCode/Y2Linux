"""The platform's command line (platform/crates/y2-platform/src/cli.rs) against the argument corpus.

tests/platform_goldens/argparse.jsonl.gz holds 5 729 argument lists (valid, permuted, abbreviated, ambiguous, odd
numbers, invalid types and choices, missing and extra arguments, `--`, `=`, help, arguments that are not UTF-8),
one JSON line each with two outcomes:
  python  what main.py's argparse parser did on the player's Python 3.15 (exit code, stdout, stderr) and, for a
          parsed list, the canonical form commands::route got from the Rust port of that parser (which gave
          Python's namespace for every parsed list); recorded once, never re-recorded
  rust    what today's parser does (exit code, stdout, stderr, canonical form); `Y2_RECORD_GOLDENS=1` re-records it
The binary runs with `Y2_ARGPARSE_ECHO=1` below a fixture root: it prints the canonical form instead of running it.

The argparse emulation was replaced by a small table-driven parser (owner decision 2026-10-29): option
abbreviations (and ambiguous prefixes), short-option clusters (`-hx`), Python's int()/float() spellings (spaces,
underscores, non-ASCII digits, `inf`/`nan`) and values beyond 64 bits, and arguments that are not UTF-8 are
refused now. `reasons` names which of these a list uses (from platform/contract.json's argument table). A list
that uses none must reach the same outcome as under Python: the same exit code, the same canonical form, the same
help (with `y2-platform` for `main.py`, rewrapped) or an error. A list that uses one either still has Python's
outcome or is refused (exit 2); it shows help instead only where it holds an exact -h/--help that Python's refusal
of the dropped spelling came before.
"""
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import gzip
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import GOLDENS, REPOSITORY, SKIP, recording, rust_binary  # noqa: E402

GOLDEN = GOLDENS / 'argparse.jsonl.gz'
ARGUMENTS = json.loads((REPOSITORY / 'platform/contract.json').read_text())['arguments']
HELP = ['-h', '--help']
PLAIN_INT = re.compile(r'[+-]?[0-9]+\Z')
PLAIN_FLOAT = re.compile(r'[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?\Z')
ERROR = re.compile(r'y2-platform( [a-z-]+)?: error: .+\Z')
# What the decision dropped, as `reasons` names it.
REASONS = ['not UTF-8', 'abbreviation', 'ambiguous prefix', 'short-option cluster', 'int spelling',
           'int beyond 64 bits', 'float spelling', 'float out of range']


def golden():
    return [json.loads(line) for line in gzip.decompress(GOLDEN.read_bytes()).decode().splitlines()]


def write_golden(lines):
    text = ''.join(json.dumps(line, sort_keys=True) + '\n' for line in lines)
    GOLDEN.write_bytes(gzip.compress(text.encode(), compresslevel=9, mtime=0))


def rust(binary, root, argv):
    environment = {'PATH': '/usr/bin:/bin', 'Y2_ROOT': root, 'Y2_ARGPARSE_ECHO': '1'}
    result = subprocess.run([str(binary), *argv], capture_output=True, env=environment, stdin=subprocess.DEVNULL,
                            timeout=30)
    stdout = result.stdout.decode()
    canonical = None
    if result.returncode == 0 and stdout.startswith('{'):
        canonical, stdout = json.loads(stdout)['canonical'], ''
    return {'exit': result.returncode, 'canonical': canonical, 'stdout': stdout, 'stderr': result.stderr.decode()}


def run_all(argvs):
    with tempfile.TemporaryDirectory(prefix='y2-argparse-') as root, ThreadPoolExecutor(16) as pool:
        return list(pool.map(lambda argv: rust(rust_binary(), root, argv), argvs))


def looks_like_option(token, names=()):
    """argparse's test: one of `names` (exact or a prefix, `=value` attached or not), else anything with a leading
    `-` but a negative number (Unicode digits included) or text with a space."""
    if not token.startswith('-') or len(token) == 1:
        return False
    name = token.partition('=')[0]
    if name in names or name.startswith('--') and any(n.startswith(name) for n in names if n.startswith('--')):
        return True
    return not re.match(r'-\.?\d', token) and ' ' not in token


def number_reason(kind, text):
    """Why Python took this int/float value and the parser does not (None: both take or both refuse it)."""
    if kind in ('int', 'append'):  # the append options (status/collect --pid) take ints
        try:
            value = int(text)
        except ValueError:
            return None
        if not PLAIN_INT.match(text):
            return 'int spelling'
        return None if -2 ** 63 <= value < 2 ** 63 else 'int beyond 64 bits'
    if kind == 'float':
        try:
            value = float(text)
        except ValueError:
            return None
        if not PLAIN_FLOAT.match(text):
            return 'float spelling'
        return None if math.isfinite(value) else 'float out of range'
    return None


def spelling(name, names):
    """(reason, the option meant) for an option name argparse resolved and the parser does not."""
    if name in names:
        return None, name
    if name.startswith('--'):
        matches = [n for n in names if n.startswith('--') and n.startswith(name)]
        if len(matches) == 1:
            return 'abbreviation', matches[0]
        if matches:
            return 'ambiguous prefix', None
    elif name.startswith('-h'):
        return 'short-option cluster', '-h'
    return None, None


def reasons(argv):
    """Which features the decision dropped this argument list uses."""
    if any('\udc80' <= c <= '\udcff' for token in argv for c in token):
        return {'not UTF-8'}
    found = set()
    at, ended = 0, False
    while at < len(argv):  # before the subcommand
        token = argv[at]
        if token == '--' and not ended:
            ended = True
        elif not ended and looks_like_option(token, HELP):
            found.add(spelling(token.partition('=')[0], HELP)[0])
        else:
            break
        at += 1
    if at < len(argv) and argv[at] in ARGUMENTS:
        arguments = ARGUMENTS[argv[at]]
        options = {a['name']: a for a in arguments if not a['positional']}
        positionals = [a for a in arguments if a['positional']]
        names = list(options) + HELP
        at, ended, slot = at + 1, False, 0
        while at < len(argv):
            token = argv[at]
            at += 1
            if token == '--' and not ended:
                ended = True
                continue
            if ended or not looks_like_option(token, names):
                if slot < len(positionals):
                    found.add(number_reason(positionals[slot]['type'], token))
                    slot += 1
                continue
            name, sep, value = token.partition('=')
            reason, meant = spelling(name, names)
            found.add(reason)
            argument = options.get(meant)
            if argument is None or argument['type'] == 'flag':
                continue
            if not sep:
                if at >= len(argv) or looks_like_option(argv[at], names):
                    continue
                value = argv[at]
                at += 1
            found.add(number_reason(argument['type'], value))
    found.discard(None)
    return found


def words(text):
    """Help text as words: the same content however it is wrapped (a hyphenated word may break at a line end)."""
    return re.sub(r'-\n\s+', '-', text).split()


def same_outcome(python, now):
    if python['exit'] != now['exit']:
        return False
    if python['canonical'] is not None:
        return now['canonical'] == python['canonical']
    if python['exit'] == 0:
        return now['stdout'].startswith('usage: y2-platform') and \
            words(now['stdout']) == words(python['stdout'].replace('main.py', 'y2-platform'))
    return now['stdout'] == '' and now['canonical'] is None and refused(now)


def refused(now):
    lines = now['stderr'].splitlines()
    return now['exit'] == 2 and now['stdout'] == '' and len(lines) >= 2 and \
        lines[0].startswith('usage: y2-platform') and bool(ERROR.match(lines[-1]))


class PlatformArgparse(unittest.TestCase):
    def test_golden_covers_every_kind_of_list(self):
        lines = golden()
        self.assertEqual(len(lines), 5729)
        python = [line['python'] for line in lines]
        self.assertGreater(sum(p['canonical'] is not None for p in python), 1000)
        self.assertGreater(sum(p['exit'] == 2 for p in python), 1000)
        self.assertTrue(any(p['exit'] == 0 and p['stdout'].startswith('usage:') for p in python))
        found = Counter(reason for line in lines for reason in reasons(line['argv']))
        for reason in REASONS:
            self.assertGreater(found[reason], 0, reason)

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_rust_parses_the_corpus_as_decided(self):
        lines = golden()
        actual = run_all([line['argv'] for line in lines])
        if recording():
            write_golden([dict(line, rust=now) for line, now in zip(lines, actual)])
        unexplained, changed_badly, unrecorded, counts = [], [], [], Counter()
        for line, now in zip(lines, actual):
            python, why = line['python'], reasons(line['argv'])
            was = 'parsed' if python['canonical'] is not None else 'help' if python['exit'] == 0 else 'refused'
            # A list counts under the first dropped feature it uses, in the order of REASONS.
            first = next((r for r in REASONS if r in why), None)
            if same_outcome(python, now):
                counts['%-7s -> same%s' % (was, ' (uses %s)' % first if first else '')] += 1
            elif not why:
                unexplained.append('%r\n  python: %r\n  rust:   %r' % (line['argv'], python, now))
            elif refused(now) or (now['exit'] == 0 and now['stdout'].startswith('usage: y2-platform')
                                  and any(t in HELP for t in line['argv'])):
                counts['%-7s -> %s: %s' % (was, 'refused' if now['exit'] else 'help', first)] += 1
            else:
                changed_badly.append('%r (%s)\n  python: %r\n  rust:   %r' % (line['argv'], why, python, now))
            if line.get('rust') != now and not recording():
                unrecorded.append('%r\n  recorded: %r\n  rust:     %r' % (line['argv'], line.get('rust'), now))
        print('\nargument corpus, %d lists:\n%s' % (len(lines), '\n'.join(
            '  %5d  %s' % (n, k) for k, n in sorted(counts.items()))), file=sys.stderr)
        self.assertEqual(unexplained[:10], [], '%d lists without a dropped feature changed' % len(unexplained))
        self.assertEqual(changed_badly[:10], [], '%d changed lists are not refused' % len(changed_badly))
        self.assertEqual(unrecorded[:10], [], '%d outcomes differ from the golden (re-record with '
                         'Y2_RECORD_GOLDENS=1 and review)' % len(unrecorded))

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_the_canonical_form_parses_to_itself(self):
        # Commands read the canonical list by position; it is also a command line the parser takes as it is
        # (unless a value itself starts with '-' and would read as an option).
        forms = [line['rust']['canonical'] for line in golden() if line.get('rust', {}).get('canonical')]
        forms = [form for form in forms if not any(looks_like_option(t) and t not in self.options(form[0])
                                                   for t in form[1:])]
        self.assertGreater(len(forms), 500)
        for form, now in zip(forms, run_all(forms)):
            self.assertEqual(now['canonical'], form)
            order = [self.options(form[0]).index(t) for t in form[1:] if t in self.options(form[0])]
            self.assertEqual(order, sorted(order), form)

    @staticmethod
    def options(name):
        return [a['name'] for a in ARGUMENTS[name] if not a['positional']]


if __name__ == '__main__':
    unittest.main()
