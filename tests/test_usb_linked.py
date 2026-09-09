"""Reject disabled/aliased USB relay syscalls in the actual linked ARM kernel."""
import os
from pathlib import Path
import struct
import unittest
from tools.validation.artifacts import Elf
from tools.validation.sleep_syscall import slot_offset
from tools.validation.usb import check
ROOT=os.environ.get('Y2_ARTIFACT_TEST_ROOT')


@unittest.skipUnless(ROOT,'requires completed build')
class LinkedUsb(unittest.TestCase):
    def test_disabled_relay_syscalls(self):
        for number in (14,19,54):
            with self.subTest(number=number):
                kernel=Elf(Path(ROOT)/'kernel/vmlinux');check(kernel)
                data=bytearray(kernel.data)
                struct.pack_into('<I',data,slot_offset(kernel,number),kernel.sym('sys_ni_syscall'))
                kernel.data=bytes(data)
                with self.assertRaisesRegex(ValueError,'USB relay syscall'): check(kernel)
