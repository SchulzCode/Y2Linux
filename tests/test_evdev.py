"""Execute real ARM input discovery/parser against unrelated devices and faults."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Evdev(unittest.TestCase):
    def test_arm_input_discovery_events_and_faults(self):
        with tempfile.TemporaryDirectory() as directory:
            binary=Path(directory)/'evdev'
            subprocess.run(['clang','--target=arm-linux-gnueabi','-mcpu=cortex-a7',
                '-marm','-mfloat-abi=soft','-Os','-ffreestanding','-fno-builtin',
                '-fno-stack-protector','-fno-unwind-tables','-fno-asynchronous-unwind-tables',
                '-nostdlib','-static','-fuse-ld=lld','-Wl,-e,_start',
                '-Wall','-Wextra','-Werror','-Wno-unused-function',
                str(ROOT/'tests/fixtures/evdev.c'),'-o',str(binary)],check=True)
            subprocess.run(['qemu-arm','-cpu','cortex-a7',str(binary)],check=True,timeout=5)
