"""Run the actual multiple-device input reader on host and ARM EABI."""
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
class Evdev(unittest.TestCase):
    def test_multiple_late_and_faulting_devices(self):
        with tempfile.TemporaryDirectory() as tmp:
            for arm in (False,True):
                p=Path(tmp)/str(arm)
                flags=['clang','-Os','-Wall','-Wextra','-Werror','-Wno-unused-function']
                if arm: flags+=['--target=arm-linux-gnueabi','-mcpu=cortex-a7','-marm','-mfloat-abi=soft','-ffreestanding','-fno-builtin','-fno-stack-protector','-nostdlib','-static','-fuse-ld=lld','-Wl,-e,_start','-DARM']
                subprocess.run(flags+[str(ROOT/'tests/fixtures/evdev-baseline.c'),'-o',str(p)],check=True)
                subprocess.run((['qemu-arm','-cpu','cortex-a7'] if arm else [])+[str(p)],check=True,timeout=5)
