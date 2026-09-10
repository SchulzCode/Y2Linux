"""Execute actual MT6582 PHY callbacks with traced MMIO and CCF ownership."""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]

class PhyHandoff(unittest.TestCase):
    def test_live_cold_and_failure_ownership(self):
        source = (PROJECT/'kernel/platform/phy-mt6582.c').read_text()
        source = re.sub(r'^#include .*\n', '', source, flags=re.M)
        shim = (PROJECT/'tests/fixtures/baseline-phy.c').read_text()
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)
            (p/'test.c').write_text(shim.replace('/* DRIVER */', source))
            subprocess.run(['clang','-std=gnu11','-O2','-Wall','-Werror',str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
