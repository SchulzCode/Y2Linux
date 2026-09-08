"""Execute the actual emitted stop block under ARM userspace with mock MMIO.

This tests instruction semantics/register preservation, not watchdog hardware.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from tools.validation.artifacts import Elf

ROOT = os.environ.get('Y2_ARTIFACT_TEST_ROOT')

@unittest.skipUnless(ROOT, 'requires emitted build artifacts')
class WatchdogInstructions(unittest.TestCase):
    def test_actual_arm_stop_preserves_handoff(self):
        root=Path(ROOT)
        elf=Elf(root/'kernel/arch/arm/boot/compressed/vmlinux')
        begin=elf.sym('y2_wdt_stop_begin');end=elf.sym('y2_wdt_continue')
        # Include the real branch and linker padding; execution must skip it.
        block=(root/'kernel/arch/arm/boot/zImage').read_bytes()[begin:end]
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);(path/'stop.bin').write_bytes(block)
            for mode in [0, 0x5c, 0x5d, 0x7f]:
                source=f'''
.syntax unified
.arch armv7-a
.arm
.global _start
_start:
    movw r0,#0x7000
    movt r0,#0x1000
    mov r1,#4096
    mov r2,#3
    mov r3,#0x32
    mvn r4,#0
    mov r5,#0
    mov r7,#192
    svc #0
    movw r2,#0x7000
    movt r2,#0x1000
    cmp r0,r2
    bne failed
    mov r1,#{mode}
    str r1,[r2]
    mov r7,#77
    mov r8,#88
    mov r9,#99
    .incbin "{path}/stop.bin"
    ldr r1,[r2]
    mov r3,#{mode & ~1}
    orr r3,r3,#0x22000000
    cmp r1,r3
    bne failed
    cmp r7,#77
    bne failed
    cmp r8,#88
    bne failed
    cmp r9,#99
    bne failed
    mov r0,#0
    b done
failed:
    mov r0,#1
done:
    mov r7,#1
    svc #0
'''
                (path/'test.S').write_text(source)
                subprocess.run(['clang','--target=arm-linux-gnueabi','-nostdlib','-static',
                    '-fuse-ld=lld','-Wl,-e,_start',str(path/'test.S'),'-o',str(path/'test')],check=True)
                with self.subTest(mode=hex(mode)):
                    subprocess.run(['qemu-arm','-cpu','cortex-a7',str(path/'test')],check=True,timeout=5)
