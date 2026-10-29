"""The Rust platform's argument parsing (platform/crates/y2-platform/src/argparse.rs) against its golden.

The parser is data, src/argparse.json (main.py's argparse parser as the Python platform built it; edited by hand
since, kept consistent by tools/development/platform-argparse.py --check). A generated corpus of argument lists
(valid, permuted, abbreviated, ambiguous, invalid types and choices, missing and extra arguments, `--`, `=`, help)
runs through the Rust binary (`Y2_PLATFORM_RUST`, with `Y2_ARGPARSE_ECHO=1` below a fixture root: it prints the
parse instead of running anything). Exit code, stdout, stderr and the parsed namespace must be the golden's,
tests/platform_goldens/argparse.jsonl.gz: one JSON line per list, first recorded from main.py's parser on the
player's Python 3.15. The canonical argument list the Rust side hands its commands must parse back to the same
namespace. After a deliberate change of argparse.json (the corpus changes with it) or of the parser,
`Y2_RECORD_GOLDENS=1` rewrites the golden from the Rust binary; review the outcomes that changed.
"""
from concurrent.futures import ThreadPoolExecutor
import gzip
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import GOLDENS, RECORD_ENV, REPOSITORY, SKIP, recording, rust_binary  # noqa: E402

CHECKER = REPOSITORY / 'tools/development/platform-argparse.py'
SPEC = json.loads((REPOSITORY / 'platform/crates/y2-platform/src/argparse.json').read_text())
COMMANDS = dict(SPEC['commands'])
GOLDEN = GOLDENS / 'argparse.jsonl.gz'

STRINGS = ['x', 'value', 'a b', '', '-', '--', '-x', '-5', '-1.5', '-.5', '-1_0', '-5x', '--x=y', '=', "it's",
           'q"uote', 'both\'"', 'ü', 'naïve', '​', ' ', '\t', 'tab\tnew\nline', 'back\\slash', '\x7f',
           '\U0001f600', '\U000e0001', 'https://example.org/manifest.json', '/media/sd/system.img', '日本']
INTS = ['0', '5', '-3', ' 7 ', '0_1', '+4', '007', '١٢', '１２', ' 9', '-0', '10',
        '99999999999999999999999', '1_000', '१२', '\t8\n', ' 3']
BAD_INTS = ['x', '1.5', '1e3', '_1', '1_', '1__0', '', ' ', '0x10', '+', '\x1c5', '−5', '1 2', '+-1', 'inf',
            '1' * 4301]
FLOATS = ['0', '0.1', '1e-1', ' 2.5 ', 'inf', '-Infinity', 'nan', '+nAn', '1_000.5', '.5', '5.', '1e400', '-1e400',
          '١.٥', '-0', '10', '0.05', '1E+01', '1e1_0', '3']
BAD_FLOATS = ['x', '1e', '.', 'e5', '1_.5', '0x1', 'infi', '1.2.3', '', '1_e5', 'nan_', '\x1c1']
HELP = ['-h', '--help', '--he', '--h', '-hx', '-h=x', '--help=', '-hh', '--hel=1']
UNKNOWN = ['--nope', '-q', '--', '-x', 'extra', '--js0n', '-=x', '--=x', '-', '-1', '-1e5']
# Arguments that are not UTF-8: Python's sys.argv holds a lone surrogate per such byte (os.fsdecode), which
# subprocess gives the Rust binary back as the byte. Its stderr writes them as \udcXX (backslashreplace).
NON_UTF8 = [['\udcff'], ['sleep', '\udcff'], ['status', 'cpu\udcff'], ['status', '\udced\udca0\udc80'],
            ['time-set', '\udcff'], ['time-set', '1\udcff'], ['boot-stage', 'a\udcffb'], ['stat\udcffus'],
            ['heal\udcffth'], ['--\udcff'], ['-\udcff', 'time'], ['time', '\udcff'], ['time', '--\udcff'],
            ['network-check', '--peer', '\udce2\udc82'], ['network-check', '--pe\udcff', 'x'],
            ['network-check', '--peer=\udcff'], ['network-check', '--peer', 'x', '--seconds', '\udcfe'],
            ['status', '--pid', '\udcff'], ['status', '--interval', '1\udcff'], ['status', '--pi\udcff', '1'],
            ['transfer', 'begin', '--name', '\udcff\udcfe', '--bytes', '1', '--sha256', 'x'],
            ['transfer', '\udcff'], ['collect', '--workload', 'w\udcff'], ['media', 'format', '--confirm-cid', '\udcff'],
            ['firmware', 'install', '/media/\udcff'], ['report', 'save', '--to', '\udcff'],
            ['reset', 'purge', '--id', '\udcff', '--confirm', '\udcfe'], ['codec-settings', '--sbc-quality', 'xq\udcff']]


def golden():
    """The recorded outcomes: [{argv, exit, stdout, stderr, namespace}] (namespace None: no parse)."""
    return [json.loads(line) for line in gzip.decompress(GOLDEN.read_bytes()).decode().splitlines()]


def write_golden(outcomes):
    text = ''.join(json.dumps(o, sort_keys=True) + '\n' for o in outcomes)
    GOLDEN.write_bytes(gzip.compress(text.encode(), compresslevel=9, mtime=0))


def rust(binary, root, argv):
    environment = {'PATH': '/usr/bin:/bin', 'Y2_ROOT': root, 'Y2_ARGPARSE_ECHO': '1'}
    result = subprocess.run([str(binary), *argv], capture_output=True, env=environment, stdin=subprocess.DEVNULL,
                            timeout=30)
    return {'exit': result.returncode, 'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()}


def echoed(got):
    """The namespace and canonical list a successful Rust parse prints (None, None otherwise)."""
    try:
        echo = json.loads(got['stdout']) if got['exit'] == 0 else None
    except ValueError:
        echo = None
    return (echo['namespace'], echo['canonical']) if isinstance(echo, dict) and 'namespace' in echo else (None, None)


def run_all(argvs):
    with tempfile.TemporaryDirectory(prefix='y2-argparse-') as root, ThreadPoolExecutor(16) as pool:
        return list(pool.map(lambda argv: rust(rust_binary(), root, argv), argvs))


def typo(rng, word):
    if not word:
        return 'x'
    i = rng.randrange(len(word))
    kind = rng.randrange(5)
    if kind == 0:
        return word[:i] + word[i + 1:]
    if kind == 1:
        return word[:i] + rng.choice('aeiouxz-') + word[i:]
    if kind == 2 and len(word) > 1:
        j = min(i + 1, len(word) - 1)
        return word[:i] + word[j] + word[i] + word[j + 1:] if j != i else word + 'q'
    if kind == 3:
        return word.upper()
    return rng.choice(STRINGS)


def good_value(rng, action):
    if action['choices']:
        choice = rng.choice(action['choices'])
        if isinstance(choice, dict):  # an int choice: any spelling int() reads
            return rng.choice([choice['int'], ' %s' % choice['int'], '0' + choice['int'],
                               '{:_}'.format(int(choice['int']))])
        return choice
    if action['type'] == 'int':
        return rng.choice(INTS)
    if action['type'] == 'float':
        return rng.choice(FLOATS)
    return rng.choice(STRINGS[:6] + STRINGS[14:] if rng.random() < 0.8 else STRINGS)


def bad_value(rng, action):
    if action['choices']:
        choice = rng.choice(action['choices'])
        if isinstance(choice, dict):
            return rng.choice(['5', '999', 'x', '1e3', ' 1001'])
        return typo(rng, choice)
    if action['type'] == 'int':
        return rng.choice(BAD_INTS)
    if action['type'] == 'float':
        return rng.choice(BAD_FLOATS)
    return rng.choice(STRINGS)


def spelled(rng, parser, option):
    """The option string, sometimes abbreviated (a unique or an ambiguous prefix)."""
    if option.startswith('--') and rng.random() < 0.4:
        return option[:rng.randrange(3, len(option) + 1)]
    return option


def command_case(rng, name):
    parser = COMMANDS[name]
    actions = [a for a in parser['actions'] if a['kind'] != 'help']
    positionals = [a for a in actions if not a['options']]
    optionals = [a for a in actions if a['options']]
    invalid = rng.random() < 0.35
    pieces = []
    for action in positionals:
        if action['nargs'] == '?' and rng.random() < 0.5:
            continue
        value = bad_value(rng, action) if invalid and rng.random() < 0.3 else good_value(rng, action)
        pieces.append(('positional', [value]))
    for action in optionals:
        if not action['required'] and rng.random() < 0.5:
            continue
        for _ in range(rng.choice([1, 1, 1, 2, 3]) if action['kind'] != 'store_true' else rng.choice([1, 1, 2])):
            option = spelled(rng, parser, action['options'][0])
            if action['kind'] == 'store_true':
                if invalid and rng.random() < 0.15:
                    pieces.append(('option', [option + '=' + rng.choice(['1', '', 'x'])]))
                else:
                    pieces.append(('option', [option]))
                continue
            value = bad_value(rng, action) if invalid and rng.random() < 0.3 else good_value(rng, action)
            if rng.random() < 0.3:
                pieces.append(('option', [option + '=' + value]))
            elif invalid and rng.random() < 0.1:
                pieces.append(('option', [option]))  # its value missing
            else:
                pieces.append(('option', [option, value]))
    # Optionals anywhere; positionals keep their order.
    ordered = [p for p in pieces if p[0] == 'positional']
    for piece in [p for p in pieces if p[0] == 'option']:
        ordered.insert(rng.randrange(len(ordered) + 1), piece)
    argv = [token for _, tokens in ordered for token in tokens]
    if invalid:
        for _ in range(rng.choice([1, 1, 2])):
            mutation = rng.randrange(6)
            if mutation == 0 and argv:
                del argv[rng.randrange(len(argv))]
            elif mutation == 1:
                argv.insert(rng.randrange(len(argv) + 1), rng.choice(UNKNOWN))
            elif mutation == 2:
                argv.insert(rng.randrange(len(argv) + 1), rng.choice(HELP))
            elif mutation == 3 and argv:
                i = rng.randrange(len(argv))
                argv.insert(i, argv[i])
            elif mutation == 4:
                argv.insert(rng.randrange(len(argv) + 1), '--')
            else:
                argv.append(rng.choice(STRINGS))
    elif rng.random() < 0.1 and positionals:
        argv.insert(len(argv) - 1 if argv and rng.random() < 0.5 else 0, '--')
    return [name, *argv]


def top_cases(rng):
    names = list(COMMANDS)
    cases = [[], ['-h'], ['--help'], ['--he'], ['-hx'], ['-h=x'], ['--help=x'], ['--h=x', 'time'], ['-=x', 'time'],
             ['nope'], ['-x', 'time'], ['-x', 'time', '-y'], ['--', 'time'], ['--', '--', 'time'], ['time', '--'],
             ['time', '--', 'x'], ['-h', 'nope'], ['nope', '-h'], ['', 'time'], ['-'], ['--'], ['-5'], ['status', '--',
             '--', 'cpu'], ['status', '--', 'cpu', '--pss'], ['sleep', '--', 'status'], ['boot-stage', '--', '-x'],
             ['boot-stage', '-x'], ['boot-stage', '-1'], ['rtc-alarm', '-5'], ['rtc-alarm', '--', '-5'],
             ['time-set', '-1_0'], ['time-set', '-.5'], ['network-check', '--peer'], ['network-check', '--peer', '-h'],
             ['network-check', '--peer=-h'], ['firmware', 'install', '--', '--x'], ['firmware', '--', 'install', 'p'],
             ['media', 'write', '--se', '5'], ['internal-media', 'setup', '--confirm'], ['codec-settings', '--sbc'],
             ['codec-settings', '--aac-q=high', '--aac-q', 'standard'], ['status', '-hx', '--pid', 'x'],
             ['status', '--pid', 'x', '-h'], ['status', '-p', '1'], ['status', '--pid', '1', '--pid=2', '--pi', '3'],
             ['collect', '--pid', '-1'], ['collect', '--seconds', '-1'], ['status', '--interval', '-1'],
             ['status', '--interval', '-inf'], ['bench-library', '--tracks', '1_000'], ['transfer', 'begin', '--bytes',
             '-0'], ['report', 'save', '--t', 'sd'], ['report', 'save', '--to', 'sd', '--top', 'wifi'],
             ['update', 'check', '--url=', '--package', ''], ['update', 'check', '--url', '--package'],
             ['status', 'cpu', 'memory'], ['status', 'CPU'], ['status', 'cpuu'], ['sleep', 'stat'], ['stauts'],
             ['helth'], ['time', 'x', 'y'], ['shutdown'], ['shutdown', 'reboot', 'now'],
             # nargs='?' after a required positional, options between them (argparse's partial matches).
             ['firmware', 'install', '--x', 'p'], ['firmware', '--x', 'install', 'p'], ['firmware', 'install', '-h', 'p'],
             ['firmware', 'install', 'p', 'q'], ['firmware', 'install', '--', 'p', 'q'], ['firmware', '--', '--', 'p'],
             ['status', '--pss', 'cpu', '--json'], ['status', 'cpu', '--pss', 'memory'], ['status', '--', '--pss'],
             ['firmware', 'install', '-', '-'], ['media', '--json', 'status', '--seconds', '5', '--release'],
             ['status', '-hh'], ['status', '-h-'], ['status', '--json=', '--pss'], ['time-set', '--', '--', '5']]
    for _ in range(200):
        name = rng.choice(names)
        cases.append([typo(rng, name)] + ([rng.choice(STRINGS)] if rng.random() < 0.3 else []))
        cases.append([rng.choice(UNKNOWN + HELP), name])
    return cases


def corpus(seed=20261028, per_command=140):
    """About 6 000 distinct argument lists (fewer for the subcommands with few arguments to vary)."""
    rng = random.Random(seed)
    cases = top_cases(rng) + [[name, flag] for name in COMMANDS for flag in HELP] + NON_UTF8
    for name in COMMANDS:
        found = set()
        for _ in range(per_command * 5):
            case = command_case(rng, name)
            found.add(json.dumps(case))
            cases.append(case)
            if len(found) >= per_command:
                break
    unique = []
    seen = set()
    for case in cases:
        key = json.dumps(case)
        if key not in seen:
            seen.add(key)
            unique.append(case)
    return unique


class PlatformArgparse(unittest.TestCase):
    def test_embedded_parser_is_consistent(self):
        check = subprocess.run([sys.executable, str(CHECKER), '--check'], capture_output=True, text=True)
        self.assertEqual(check.returncode, 0, check.stdout + check.stderr)

    @unittest.skipIf(recording(), 'recording the golden')
    def test_golden_is_the_corpus(self):
        # The corpus follows argparse.json: a changed parser is re-recorded (RECORD_ENV), never run partly.
        recorded = [o['argv'] for o in golden()]
        cases = corpus()
        self.assertEqual(len(recorded), len(cases), 'the corpus changed: record the golden with %s=1' % RECORD_ENV)
        self.assertEqual([a for a, b in zip(recorded, cases) if a != b][:5], [])

    def test_golden_covers_every_kind_of_outcome(self):
        outcomes = golden()
        self.assertGreater(len(outcomes), 5000)
        exits = [o['exit'] for o in outcomes]
        self.assertGreater(exits.count(0), 1000)
        self.assertGreater(exits.count(2), 1000)
        errors = '\n'.join(o['stderr'] for o in outcomes)
        for text in ['ambiguous option', 'ignored explicit argument', 'expected one argument', 'invalid int value',
                     'invalid float value', 'invalid choice', 'maybe you meant', 'unrecognized arguments',
                     'the following arguments are required']:
            self.assertIn(text, errors)
        self.assertTrue(any(o['exit'] == 0 and o['stdout'].startswith('usage:') for o in outcomes))

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_rust_parses_like_the_golden(self):
        if recording():
            cases = corpus()
            outcomes = []
            for argv, got in zip(cases, run_all(cases)):
                namespace, _ = echoed(got)
                outcomes.append({'argv': argv, 'exit': got['exit'], 'namespace': namespace,
                                 'stdout': '' if namespace is not None else got['stdout'], 'stderr': got['stderr']})
            write_golden(outcomes)
            return
        expected = golden()
        actual = run_all([want['argv'] for want in expected])
        failures, canonicals = [], []
        for want, got in zip(expected, actual):
            namespace, canonical = echoed(got)
            if want['namespace'] is not None:
                same = (got['exit'], namespace, got['stderr']) == (0, want['namespace'], want['stderr'])
                if same:
                    canonicals.append((want['argv'], namespace, canonical))
            else:
                same = namespace is None and all(got[k] == want[k] for k in ('exit', 'stdout', 'stderr'))
            if not same:
                failures.append('%r\n  golden: %r\n  rust:   %r' % (want['argv'], want, got))
        self.assertEqual(failures[:20], [], '%d of %d differ' % (len(failures), len(expected)))
        # The canonical form: subcommand, positionals, then the options in definition order, full names; it
        # parses back to the same namespace (unless a value itself starts with '-', which argparse would take
        # for an option: commands read the canonical list by position, never through argparse).
        back = [(argv, namespace, canonical) for argv, namespace, canonical in canonicals
                if not any(token.startswith('-') and token not in self.options(canonical[0])
                           for token in canonical[1:])]
        self.assertGreater(len(back), 1000)
        again = run_all([canonical for _, _, canonical in back])
        for (argv, namespace, canonical), got in zip(back, again):
            with self.subTest(argv=argv):
                self.assertEqual(canonical[0], namespace['command'])
                self.assertEqual(echoed(got)[0], namespace, canonical)
                order = [self.options(canonical[0]).index(t) for t in canonical[1:] if t in self.options(canonical[0])]
                self.assertEqual(order, sorted(order), canonical)

    @staticmethod
    def options(name):
        return [o for a in COMMANDS[name]['actions'] if a['kind'] != 'help' for o in a['options']]


if __name__ == '__main__':
    unittest.main()
