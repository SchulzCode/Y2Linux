"""Exercise production relay on host and ARM with partial/faulting I/O."""
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]


class Relay(unittest.TestCase):
    def test_bounded_protocol(self):
        with tempfile.TemporaryDirectory() as d:
            for arm in (False,True):
                with self.subTest(arm=arm):
                    binary=Path(d)/('arm' if arm else 'host')
                    flags=['clang','-Os','-Wall','-Wextra','-Werror']
                    if arm:
                        flags+=['--target=arm-linux-gnueabi','-mcpu=cortex-a7','-marm',
                            '-mfloat-abi=soft','-ffreestanding','-fno-builtin','-fno-stack-protector',
                            '-nostdlib','-static','-fuse-ld=lld','-Wl,-e,_start','-DARM']
                    subprocess.run(flags+[str(ROOT/'tests/fixtures/relay.c'),'-o',str(binary)],check=True)
                    subprocess.run((['qemu-arm','-cpu','cortex-a7'] if arm else [])+[str(binary)],check=True,timeout=5)
