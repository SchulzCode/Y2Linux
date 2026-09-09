"""Run production PID1 control flow on ARM with a bounded syscall fixture."""
from pathlib import Path
import subprocess
import tempfile
import unittest
PROJECT=Path(__file__).resolve().parents[1]
class Pid1Control(unittest.TestCase):
    def test_runtime_without_uart(self):
        with tempfile.TemporaryDirectory() as d:
            for case in range(26):
                with self.subTest(case=case):
                    binary=Path(d)/('pid1-'+str(case))
                    subprocess.run(['clang','--target=arm-linux-gnueabi','-mcpu=cortex-a7',
                        '-marm','-mfloat-abi=soft','-Os','-ffreestanding','-fno-builtin',
                        '-fno-stack-protector','-fno-unwind-tables','-fno-asynchronous-unwind-tables',
                        '-nostdlib','-static','-fuse-ld=lld','-Wl,-e,_start',
                        '-Wall','-Wextra','-Werror','-DY2_SYSCALL_TEST','-DTEST_CASE='+str(case),
                        '-I'+str(PROJECT/'kernel/diagnostic'),
                        str(PROJECT/'initramfs/start.S'),str(PROJECT/'initramfs/init.c'),
                        str(PROJECT/'tests/fixtures/pid1_syscalls.c'),'-o',str(binary)],check=True)
                    subprocess.run(['qemu-arm','-cpu','cortex-a7',str(binary)],check=True,timeout=5)
