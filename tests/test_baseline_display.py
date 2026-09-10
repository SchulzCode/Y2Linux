"""Test the display child isolation and exact in-RAM module packaging."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from tools.build.initramfs import archive
from tools.validation.formats import cpio
ROOT=Path(__file__).resolve().parents[1]

class DisplayLoader(unittest.TestCase):
    def test_parent_never_blocks_on_module_init(self):
        with tempfile.TemporaryDirectory() as tmp:
            for arm in (False,True):
                p=Path(tmp)/str(arm)
                flags=['clang','-Os','-Wall','-Wextra','-Werror','-Wno-unused-function']
                if arm: flags+=['--target=arm-linux-gnueabi','-mcpu=cortex-a7','-marm','-mfloat-abi=soft','-ffreestanding','-fno-builtin','-fno-stack-protector','-nostdlib','-static','-fuse-ld=lld','-Wl,-e,_start','-DARM']
                subprocess.run(flags+[str(ROOT/'tests/fixtures/display-loader.c'),'-o',str(p)],check=True)
                subprocess.run((['qemu-arm','-cpu','cortex-a7'] if arm else [])+[str(p)],check=True,timeout=5)

    def test_archive_requires_exact_module_and_combined_cap(self):
        image=archive(b'init',b'module')
        self.assertEqual(cpio(image,b'module'),b'init')
        with self.assertRaises(ValueError): cpio(image,b'MODULE')
        with self.assertRaises(ValueError): cpio(image)
        with self.assertRaises(ValueError): archive(bytes(0x100001),bytes(0x100000))
