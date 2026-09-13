"""Execute the production MSDC request/command/completion code against fake MMIO.

The card/DMA responses are injected, not a claim of physical bus qualification.
The production code itself chooses SBC, wire arguments, transfer type and errors.
"""
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path
from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]


def function(source, name):
    start = source.rfind('\nstatic ', 0, source.index(name + '(')) + 1
    # Some functions have forward declarations. Select the actual definition.
    pattern = r'^static [^;{}]*\b' + name + r'\([^;{}]*\)\n\{'
    match = re.search(pattern, source, re.M)
    if match is None:
        raise ValueError(name)
    start = match.start()
    opening = source.index('{', match.start())
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end] + '\n'


class Transport(unittest.TestCase):
    def test_real_request_sequence_wire_arguments_and_faults(self):
        spec = next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/mmc/host/mtk-sd.c')
        source = prepare_overlay(ROOT, spec).read_text()
        names = ('msdc_cmd_prepare_raw_cmd', 'msdc_start_command', 'msdc_cmd_next',
                 'msdc_ops_request', 'msdc_cmd_done', 'msdc_data_xfer_next',
                 'msdc_data_xfer_done')
        bodies = '\n'.join(function(source, name) for name in names)
        constants = '\n'.join(line for line in source.splitlines()
                              if line.startswith('#define ') and not line.endswith('\\'))
        header = (ROOT/'.cache/sources/linux-6.18/include/linux/mmc/mmc.h').read_text()
        constants += '\n' + '\n'.join(line for line in header.splitlines()
                                       if line.startswith('#define ') and not line.endswith('\\'))
        harness = (ROOT/'tests/fixtures/production/mmc-transport-harness.h').read_text()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            code = '#include <stdint.h>\n#define BIT(n) (1U << (n))\n' + constants + '\n' + harness
            code += '\n' + bodies + '\n' + (ROOT/'tests/fixtures/production/mmc-transport-main.c').read_text()
            (path/'test.c').write_text(code)
            subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-Wno-unused-parameter',
                            '-Wno-unused-variable', '-I'+str(ROOT/'kernel/platform'),
                            str(path/'test.c'), '-o', str(path/'test')], check=True)
            subprocess.run([str(path/'test'), str(ROOT/'tests/fixtures/production/MBR')], check=True)
