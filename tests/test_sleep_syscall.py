"""Check real linked syscall 162; a userspace syscall mock cannot catch ENOSYS."""
import os
from pathlib import Path
import struct
import tempfile
import unittest
from tools.validation.artifacts import Elf
from tools.validation.sleep_syscall import check, slot_offset

ROOT = os.environ.get('Y2_ARTIFACT_TEST_ROOT')


@unittest.skipUnless(ROOT, 'requires built kernel via Y2_ARTIFACT_TEST_ROOT')
class SleepSyscall(unittest.TestCase):
    def setUp(self):
        self.kernel = Elf(Path(ROOT) / 'kernel/vmlinux')
        self.config = Path(ROOT) / 'kernel/.config'

    def test_actual_kernel_syscall_162(self):
        self.assertEqual(check(self.kernel, self.config)['target'], 'sys_nanosleep_time32')

    def test_reject_missing_time32(self):
        with tempfile.TemporaryDirectory() as d:
            config = Path(d) / '.config'
            config.write_text(self.config.read_text().replace(
                'CONFIG_COMPAT_32BIT_TIME=y', '# CONFIG_COMPAT_32BIT_TIME is not set'))
            with self.assertRaisesRegex(ValueError, 'requires CONFIG_COMPAT_32BIT_TIME'):
                check(self.kernel, config)

    def test_reject_actual_table_redirected_to_enosys(self):
        # Keep a valid config and the real function: table bytes must still be checked.
        data = bytearray(self.kernel.data)
        struct.pack_into('<I', data, slot_offset(self.kernel), self.kernel.sym('sys_ni_syscall'))
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'vmlinux'
            path.write_bytes(data)
            with self.assertRaisesRegex(ValueError, 'resolves to sys_ni_syscall'):
                check(Elf(path), self.config)
