"""Execute production PID1 control flow with file-backed roots and mock mounts.

Hardware, mounts, fsck and exec targets are replaced at the host boundary. The
actual ordering and production gates are exercised without a privileged mount.
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]


class Handover(unittest.TestCase):
    def exercise(self, failure='', cmdline=''):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)
            for d in ('sbin','bin','dev','proc','sys','run','tmp','newroot/etc/y2linux',
                      'newroot/sbin','newroot/data','newdata','sys/kernel/debug'):
                (p/d).mkdir(parents=True,exist_ok=True)
            (p/'newroot/etc/y2linux/layout-version').write_text('1\n')
            (p/'newroot/etc/y2linux/platform-contract').write_text('y2-platform-v1\n')
            (p/'newdata/.y2data-schema').write_text('2\n' if failure=='schema' else '1\n')
            (p/'newroot/sbin/init').write_text('#!/bin/sh\nexit 0\n');(p/'newroot/sbin/init').chmod(0o755)
            (p/'proc/cmdline').write_text(cmdline)
            (p/'display.ko').write_bytes(b'module fixture')
            script=(ROOT/'initramfs/production/init').read_text()
            # Replace the terminal rescue loop with a finite test exit, keeping
            # every call site and root handover branch as production implements it.
            start=script.index('rescue() {');end=script.index('\ncase ',start)
            script=script[:start]+"rescue() { echo \"RESCUE $*\" >> \"$LEDGER\"; exit 99; }\n"+script[end:]
            script=re.sub(r'(?<![A-Za-z0-9_./])/(?:sbin|bin|dev|proc|sys|run|tmp|newroot|newdata|display\.ko)(?=[/\s"\']|$)',
                          lambda m:str(p)+m.group(),script)
            script=re.sub(r'^export PATH=.*$',f'export PATH={p}/bin:{p}/sbin:/usr/bin:/bin',script,flags=re.M)
            script=script.replace('exec switch_root ',f'exec {p}/bin/switch_root ')
            # The fake mount command records ordering and can inject handover failure.
            wrapper='''#!/bin/sh
name=${0##*/}
echo "$name $*" >> "$LEDGER"
case "$name" in
mount) case "$FAILURE:$*" in
  move:'--move '*'/sys '*) exit 1;;
esac;;
e2fsck) [ "$FAILURE" != fsck ] || exit 4;;
switch_root) [ "$FAILURE" != exec ] || exit 1; echo 'BUILDROOT INIT' >> "$LEDGER";;
esac
exit 0
'''
            for name in ('mount','umount','mknod','sleep','chroot','switch_root','e2fsck',
                         'y2-platform-start','y2-abi-check','y2-usb-status','y2-status'):
                target=p/('sbin' if name.startswith('y2-') or name=='e2fsck' else 'bin')/name
                target.write_text('#!/nonexistent-interpreter\n' if name=='switch_root' and failure=='exec' else wrapper)
                target.chmod(0o755)
            (p/'sbin/y2-storage').write_text('''y2_find_partition() {
 [ "$FAILURE" != missing ] || return 1
 case "$1" in Y2ROOT) echo /dev/mmcblk0p5;; Y2DATA) echo /dev/mmcblk0p7;; esac
}
''')
            ledger=p/'ledger'
            build=Path(os.environ.get('Y2_ARTIFACT_TEST_ROOT','/nonexistent'))
            if (build/'buildroot/target/bin/busybox').exists() and shutil.which('qemu-arm'):
                shell=['qemu-arm','-cpu','cortex-a7','-L',str(build/'buildroot/target'),
                       str(build/'buildroot/target/bin/busybox'),'sh']
            else:
                shell=[str(ROOT/'.cache/environment/lib/ld-musl-x86_64.so.1'),
                       str(ROOT/'.cache/environment/bin/busybox'),'sh']
            result=subprocess.run(shell+['-c',script],env=os.environ|{'LEDGER':str(ledger),'FAILURE':failure},
                                  text=True,capture_output=True,timeout=5)
            events=ledger.read_text().replace(str(p),'')
            log=(p/'dev/kmsg').read_text() if (p/'dev/kmsg').exists() else ''
            return result,events,log

    def test_no_sd_root_data_mounts_then_switch_root(self):
        result,events,log=self.exercise()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('switching to Buildroot',log)
        sequence=['mount -t ext4 -o ro,noload /dev/mmcblk0p5 /newroot',
                  'mount -t ext4 -o ro,noload /dev/mmcblk0p7 /newdata',
                  'e2fsck -p /dev/mmcblk0p5','e2fsck -p /dev/mmcblk0p7',
                  'mount -t ext4 -o rw /dev/mmcblk0p5 /newroot',
                  'mount -t ext4 -o rw,nosuid,nodev /dev/mmcblk0p7 /newroot/data',
                  'mount --bind /run/y2/modules /newroot/lib/modules',
                  'mount --move /proc /newroot/proc','mount --move /sys /newroot/sys',
                  'mount --move /run /newroot/run','mount --move /tmp /newroot/tmp',
                  'mount --move /dev /newroot/dev',
                  'switch_root -c /dev/console /newroot /sbin/init','BUILDROOT INIT']
        positions=[events.index(event) for event in sequence]
        self.assertEqual(positions,sorted(positions))
        self.assertNotIn('mmcblk1',events)

    def test_missing_identity_schema_and_fsck_stay_in_rescue(self):
        for failure in ('missing','schema','fsck'):
            with self.subTest(failure=failure):
                result,events,_=self.exercise(failure)
                self.assertEqual(result.returncode,99,result.stderr)
                self.assertIn('RESCUE ',events)
                self.assertNotIn('switch_root ',events)
                self.assertNotIn('mount -t ext4 -o rw ',events)

    def test_explicit_rescue_never_mounts_a_filesystem(self):
        result,events,_=self.exercise(cmdline='rdinit=/init y2.rescue=1')
        self.assertEqual(result.returncode,99,result.stderr)
        self.assertNotIn('mount -t ext4',events)

    def test_failed_mount_move_rolls_back_before_rescue(self):
        result,events,_=self.exercise('move')
        self.assertEqual(result.returncode,99,result.stderr)
        self.assertIn('mount --move /newroot/proc /proc',events)
        self.assertNotIn('switch_root ',events)

    def test_failed_exec_restores_mounts_and_enters_rescue(self):
        result,events,_=self.exercise('exec')
        self.assertEqual(result.returncode,99,result.stderr)
        for name in ('dev','tmp','run','sys','proc'):
            self.assertIn('mount --move /newroot/'+name+' /'+name,events)
        self.assertIn('RESCUE switch_root exec failed',events)
        self.assertNotIn('BUILDROOT INIT',events)
