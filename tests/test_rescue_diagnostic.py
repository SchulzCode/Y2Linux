import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def fixture(stage=9, magic=0x59325532, initial_magic=0x59325031):
    power = struct.pack('<Ii11I', initial_magic, 0, 7, 0, 1, 8, 1, 1,
                        0, 0, 0x23, 0x8000, 0x20)
    live = struct.pack('<Ii7I', magic, -19, stage, 0, 0x20, 0x80, 0, 0, 0) + bytes(52)
    clock = struct.pack('<i5I', 0, 15, 0, 0x10000, 0x05000001, 1)
    phy = struct.pack('<i22I', 0, 0x1fffff, *([0]*21))
    wake = struct.pack('<i22I', -19, 0, 0, 0, *([0]*19))
    return live + power + clock + phy + wake


class USBStatus(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.path = Path(cls.temp.name)
        cls.binary = cls.path/'usb-status'
        subprocess.run([os.environ.get('CC', 'clang'), '-Wall','-Wextra','-Werror',
            str(ROOT/'tools/production/usb-status.c'), '-o',str(cls.binary)], check=True)

    @classmethod
    def tearDownClass(cls): cls.temp.cleanup()

    def run_status(self, raw):
        source = self.path/'snapshot'
        source.write_bytes(raw)
        return subprocess.run([str(self.binary), str(source)], text=True, capture_output=True)

    def test_cached_failure_and_initial_cable_state(self):
        r = self.run_status(fixture())
        self.assertEqual(r.returncode, 0)
        self.assertIn('USB FAILED rc=-19 configured=0', r.stdout)
        self.assertIn('Initial PW=0/7 CHR=0020', r.stdout)
        self.assertIn('WAKE=-19/0', r.stdout)

    def test_untrusted_or_truncated_status_is_rejected(self):
        for raw in (fixture(stage=99), fixture(magic=0), fixture()[:40], fixture()[:88],
                    fixture(initial_magic=0)):
            with self.subTest(size=len(raw)):
                self.assertEqual(self.run_status(raw).returncode, 2)


if __name__ == '__main__': unittest.main()
