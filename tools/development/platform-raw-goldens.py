#!/usr/bin/env python3
"""The golden suite's cases with key order and JSON spacing kept (stdout and file contents), written into
an empty directory. tests/platform_golden.py compares parsed JSON (sorted), so it cannot see a change of key
order or separators; run this with a baseline binary and with the changed one and diff the two directories
(`diff -r`): a refactor that keeps the output shows no difference.

usage: platform-raw-goldens.py <y2-platform binary> <empty output directory>
"""
import json
import os
import re
import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(REPO), str(REPO / 'tests')]
binary, out = sys.argv[1], Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
os.environ['Y2_PLATFORM_RUST'] = binary
os.environ['Y2_RECORD_GOLDENS'] = '1'

import platform_golden as pg  # noqa: E402

pg.GOLDENS = out

# The spacing of a JSON text: every structural character outside strings, with the space after it.
STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


def shape(line):
    return re.sub(r'[^{}\[\],: ]', '', STRING.sub('', line))


def normalise_text(text, root, volatile, patterns):
    text = pg.replace_paths(text, root)
    for pattern, replacement in patterns:
        text = re.sub(pattern, replacement, text)
    lines = []
    for line in text.splitlines():
        try:
            lines.append({'json': pg.normalise_json(json.loads(line), volatile), 'shape': shape(line)})
        except ValueError:
            lines.append(line)
    if len(lines) > 1:
        return lines
    try:
        return {'json': pg.normalise_json(json.loads(text), volatile), 'shape': shape(text)}
    except ValueError:
        return lines


pg.normalise_text = normalise_text
pg.encode = lambda result: json.dumps(result, indent=1) + '\n'
original_check = pg.check


def check(testcase, group, name, result):
    path = pg.golden_path(group, name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(pg.encode(result))


pg.check = check

modules = ['tests.test_platform_differential', 'tests.test_platform_devices', 'tests.test_platform_observation',
           'tests.test_platform_reports', 'tests.test_platform_suspend', 'tests.test_platform_power_rpc']
suite = unittest.defaultTestLoader.loadTestsFromNames(modules)
for module in modules:
    mod = sys.modules[module]
    if hasattr(mod, 'check'):
        mod.check = check
    if hasattr(mod, 'normalise_text'):
        mod.normalise_text = normalise_text
unittest.TextTestRunner(verbosity=0).run(suite)
