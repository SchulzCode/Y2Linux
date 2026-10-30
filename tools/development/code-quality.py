#!/usr/bin/env python3
"""Does the Rust code of Y2Linux's platform and Y2Reborn stay as clean as it is (docs/CODING-STANDARD.md)?

Measures the production code (tests excluded: `*tests.rs`, `tests/`, `*_tests/`, inline `#[cfg(test)] mod …
{}` and files of `#[cfg(test)] mod …;` declarations) and compares it with code-quality-baseline.json next to
this script. The check is a ratchet: nothing may get worse, everything may get better.

- functions over 100 lines: none new, none longer;
- files over 1000 lines: none new, none longer;
- `unsafe` (blocks, functions, impls) per crate: no more, and none in a crate that has none;
- `.unwrap()` / `.expect(` per crate, `panic!` / `todo!` / `unimplemented!` / `unreachable!` per crate,
  `#[allow(…)]` per crate: no more;
- raw JSON field lookups (`["field"]`) per file: no more (read records in their y2p-contract types);
- code lines rustfmt left over 100 columns per file: no more (break them up so rustfmt formats them);
- `TODO` / `FIXME` / `XXX` / `HACK`: none (open work goes into the planning documents).

A regression is printed as `metric: where: baseline -> now` and the exit status is 1. After an improvement (or a
reviewed exception, explained in the commit), `--update` writes the current figures as the new baseline; the
baseline's diff is part of the commit and its review.

usage: code-quality.py [--reborn DIR] [--update]
"""
import json
import os
import re
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = Path(__file__).with_name('code-quality-baseline.json')
FUNCTION_LINES = 100
FILE_LINES = 1000
LINE_COLUMNS = 100
PER_CRATE = ('unsafe', 'unwrap', 'panic', 'allow')
PER_FILE = ('json_lookups', 'long_lines')


def blank(source):
    """The source with comments, strings and character literals blanked out (line breaks kept)."""
    out, i, n = [], 0, len(source)
    while i < n:
        if source.startswith('//', i):
            j = source.find('\n', i)
            j = n if j < 0 else j
        elif source.startswith('/*', i):
            j = source.find('*/', i + 2)
            j = n if j < 0 else j + 2
        elif source[i] == 'r' and re.match(r'r#*"', source[i:i + 8]) and not (i and (source[i - 1].isalnum()
                                                                                   or source[i - 1] == '_')):
            hashes = re.match(r'r(#*)"', source[i:]).group(1)
            j = source.find('"' + hashes, i + len(hashes) + 2)
            j = n if j < 0 else j + len(hashes) + 1
        elif source[i] == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == '\\' else 1
            j += 1
        elif source[i] == "'" and re.match(r"'(\\.[^']*|[^'\\])'", source[i:i + 12]):
            j = i + len(re.match(r"'(\\.[^']*|[^'\\])'", source[i:]).group(0))
        else:
            out.append(source[i])
            i += 1
            continue
        out.append(re.sub(r'[^\n]', ' ', source[i:j]))
        i = j
    return ''.join(out)


def closing(text, start):
    """The index just after the brace that closes the one at `start`."""
    depth = 0
    for j in range(start, len(text)):
        if text[j] == '{':
            depth += 1
        elif text[j] == '}':
            depth -= 1
            if depth == 0:
                return j + 1
    return len(text)


def test_spans(text):
    """The spans of inline `#[cfg(test)] mod … { … }` blocks."""
    spans = []
    for match in re.finditer(r'#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{', text):
        brace = text.index('{', match.start())
        spans.append((match.start(), closing(text, brace)))
    return spans


def test_module_files(path, source):
    """The files of the `#[cfg(test)] mod name;` declarations in `path`."""
    directory = path.parent if path.name in ('lib.rs', 'main.rs', 'mod.rs') else path.with_suffix('')
    files = []
    pattern = r'#\[cfg\(test\)\]\s*(?:#\[path\s*=\s*"([^"]+)"\]\s*)?(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;'
    for match in re.finditer(pattern, source):
        given, name = match.groups()
        if given:
            files.append((path.parent / given).resolve())
        else:
            files += [(directory / f'{name}.rs').resolve(), (directory / name / 'mod.rs').resolve()]
    return files


def sources(roots):
    """(crate, path) of every production source file below `roots`."""
    found, tests = [], set()
    for root, crate_of in roots:
        for directory, _, names in os.walk(root):
            if '/target' in directory or '/vendor' in directory or '/tests' in directory or '_tests' in directory:
                continue
            for name in names:
                path = Path(directory, name)
                if name.endswith('.rs') and not name.endswith('tests.rs'):
                    found.append((crate_of(path), path))
                    tests.update(test_module_files(path, path.read_text(errors='replace')))
    return [(crate, path) for crate, path in found if path.resolve() not in tests]


def functions(text):
    """(name, lines) of every function with a body in `text` (comments and strings blanked)."""
    result = []
    for match in re.finditer(r'\bfn\s+(\w+)', text):
        depth, j = 0, match.end()
        while j < len(text):
            ch = text[j]
            if ch in '(<[':
                depth += 1
            elif ch in ')>]':
                depth -= 1
            elif ch == ';' and depth <= 0:
                j = -1
                break
            elif ch == '{' and depth <= 0:
                break
            j += 1
        if 0 <= j < len(text):
            result.append((match.group(1), text[match.start():closing(text, j)].count('\n') + 1))
    return result


def measure(roots, base):
    figures = {'long_functions': {}, 'long_files': {}, 'markers': {}}
    for key in PER_CRATE + PER_FILE:
        figures[key] = Counter()
    for crate, path in sources(roots):
        source = path.read_text(errors='replace')
        text = blank(source)
        spans = test_spans(text)
        where = str(path.relative_to(base))
        keep = lambda s: ''.join(c if not any(a <= i < b for a, b in spans) else (c if c == '\n' else ' ')
                                 for i, c in enumerate(s))
        code, raw = (keep(text), keep(source)) if spans else (text, source)
        lines = len(code.rstrip('\n').split('\n')) - sum(code[a:b].count('\n') for a, b in spans)
        if lines > FILE_LINES:
            figures['long_files'][where] = lines
        seen = Counter()
        for name, length in functions(code):
            seen[name] += 1
            if length > FUNCTION_LINES:
                key = f'{where}::{name}' + (f'#{seen[name]}' if seen[name] > 1 else '')
                figures['long_functions'][key] = length
        figures['unsafe'][crate] += len(re.findall(r'\bunsafe\s*(\{|fn\b|impl\b)', code))
        figures['unwrap'][crate] += len(re.findall(r'\.unwrap\(\)|\.expect\(', code))
        figures['panic'][crate] += len(re.findall(r'\b(panic|todo|unimplemented|unreachable)!', code))
        figures['allow'][crate] += len(re.findall(r'#!?\[allow\(', code))
        figures['json_lookups'][where] += len(re.findall(r'\["[A-Za-z_0-9]+"\]', raw))
        figures['long_lines'][where] += sum(1 for line in raw.split('\n')
                                            if len(line) > LINE_COLUMNS and not line.strip().startswith('//'))
        markers = len(re.findall(r'\b(TODO|FIXME|XXX|HACK)\b', raw))
        if markers:
            figures['markers'][where] = markers
    for key in PER_CRATE + PER_FILE:
        figures[key] = {k: v for k, v in sorted(figures[key].items()) if v}
    return figures


def regressions(baseline, now):
    found = []
    for metric, values in now.items():
        before = baseline.get(metric, {})
        for where, value in values.items():
            old = before.get(where, 0)
            if metric == 'markers' or value > old:
                found.append(f'{metric}: {where}: {old} -> {value}')
    return found


def main():
    args = sys.argv[1:]
    reborn = Path(args[args.index('--reborn') + 1] if '--reborn' in args else REPO.parent / 'Y2Reborn').resolve()
    base = REPO.parent
    crates = REPO / 'platform' / 'crates'
    roots = [(crates, lambda p: 'platform/' + p.relative_to(crates).parts[0])]
    if reborn.is_dir():
        for part in ('crates', 'app'):
            top = reborn / part
            roots.append((top, lambda p, top=top: 'reborn/' + p.relative_to(top).parts[0]))
    now = measure(roots, base)
    # The contract crate is Y2Linux's; Reborn's copy is identical (release.py refuses otherwise).
    for metric in now:
        now[metric] = {k: v for k, v in now[metric].items() if 'Y2Reborn/crates/y2p-contract' not in k
                       and k != 'reborn/y2p-contract'}
    if '--update' in args:
        BASELINE.write_text(json.dumps(now, indent=1, sort_keys=True) + '\n')
        print(f'baseline written: {sum(len(v) for v in now.values())} entries')
        return 0
    baseline = json.loads(BASELINE.read_text())
    found = regressions(baseline, now)
    for line in found:
        print(line)
    better = [f'{m}: {w}' for m, values in baseline.items() for w, v in values.items() if now.get(m, {}).get(w, 0) < v]
    if better and not found:
        print(f'{len(better)} figures improved: record them with --update')
    return 1 if found else 0


if __name__ == '__main__':
    sys.exit(main())
