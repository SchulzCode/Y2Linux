"""Mutate actual build evidence; require the full artifact validator to reject it."""
import contextlib
import gzip
import io
import os
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from tools.validation.artifacts import check
from tools.validation.bootimg import check as check_bootimg

ROOT = os.environ.get('Y2_ARTIFACT_TEST_ROOT')
PROJECT = Path(__file__).resolve().parents[1]

@unittest.skipUnless(ROOT, 'requires a completed build via Y2_ARTIFACT_TEST_ROOT')
class ActualArtifacts(unittest.TestCase):
    def setUp(self):
        self.original = Path(ROOT)

    def verify(self, root):
        with contextlib.redirect_stdout(io.StringIO()): return check(root, PROJECT)

    def test_actual_artifacts(self):
        report, payload = self.verify(self.original)
        check_bootimg((self.original/'BOOTIMG.img').read_bytes(),payload,
                      (self.original/'initramfs.cpio.gz').read_bytes(),report)
        self.assertEqual(report['status'], 'PASS offline D08; hardware launch NOT authorized')

    def test_actual_corruptions(self):
        files = ['kernel/.config','kernel/vmlinux','kernel/arch/arm/boot/Image',
                 'kernel/arch/arm/boot/zImage','kernel/arch/arm/boot/compressed/vmlinux',
                 'y2.dtb','initramfs.cpio.gz','init','zImage-dtb','BOOTIMG.img']
        def flip(data):
            b=bytearray(data); b[len(b)//2]^=1; return bytes(b)
        def replace(old,new):
            def mutate(data):
                self.assertIn(old,data)
                return data.replace(old,new,1)
            return mutate
        cases = [
          ('kernel/.config',replace(b'# CONFIG_ARM_ATAG_DTB_COMPAT is not set',b'CONFIG_ARM_ATAG_DTB_COMPAT=y')),
          ('kernel/.config',replace(b'# CONFIG_SMP is not set',b'CONFIG_SMP=y')),
          ('kernel/.config',replace(b'CONFIG_PHYS_OFFSET=0x80000000',b'CONFIG_PHYS_OFFSET=0x00000000')),
          ('kernel/.config',replace(b'# CONFIG_ARM_VIRT_EXT is not set',b'CONFIG_ARM_VIRT_EXT=y')),
          ('kernel/.config',lambda b:b+b'\nCONFIG_SERIAL_8250_DMA=y\n'),
          ('y2.dtb',replace(struct.pack('>IIII',0x80000000,0x1800000,0x84000000,0x80000),
                           struct.pack('>IIII',0x80000000,0x40000000,0x84000000,0x80000))),
          ('y2.dtb',replace(struct.pack('>I',0x84000000+(self.original/'initramfs.cpio.gz').stat().st_size),struct.pack('>I',0x84000001+(self.original/'initramfs.cpio.gz').stat().st_size))),
          ('y2.dtb',replace(b'serial0\0',b'serial1\0')),
          ('y2.dtb',replace(struct.pack('>II',0x11002000,0x400),struct.pack('>II',0x11005000,0x400))),
          ('y2.dtb',replace(struct.pack('>III',0,51,8),struct.pack('>III',0,54,8))),
          ('y2.dtb',replace(struct.pack('>QQ',0x80000000,0x4000),struct.pack('>QQ',0x80000000,0x3000))),
          ('y2.dtb',lambda b:b[:-1]),
          ('initramfs.cpio.gz',lambda b:b+b),
          ('initramfs.cpio.gz',lambda b:gzip.compress(bytes(0x210001),mtime=0)),
          ('initramfs.cpio.gz',lambda b:b[:-1]),
          ('kernel/arch/arm/boot/Image',flip),
          ('kernel/arch/arm/boot/zImage',flip),
          ('kernel/vmlinux',replace(b'\x7fELF\x01\x01',b'\x7fELF\x01\x02')),
          ('kernel/arch/arm/boot/compressed/vmlinux',replace(b'LC1\0',b'LX1\0')),
          ('zImage-dtb',lambda b:b[:-1]+bytes([b[-1]^1])),
          ('init',flip),
          ('BOOTIMG.img',lambda b:b[:-1]+bytes([b[-1]^1])),
        ]
        for path, mutate in cases:
            with self.subTest(path=path), tempfile.TemporaryDirectory() as directory:
                root=Path(directory)
                for name in files:
                    dest=root/name;dest.parent.mkdir(parents=True,exist_ok=True)
                    if name==path: dest.write_bytes(mutate((self.original/name).read_bytes()))
                    else: dest.symlink_to(self.original/name)
                with self.assertRaises((ValueError,struct.error)):
                    self.verify(root)

if __name__=='__main__': unittest.main()
