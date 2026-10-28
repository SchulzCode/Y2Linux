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
                      'newroot/sbin','newroot/data','newdata','sys/kernel/debug','sys/firmware/y2_boot'):
                (p/d).mkdir(parents=True,exist_ok=True)
            (p/'newroot/etc/y2linux/layout-version').write_text('1\n')
            (p/'newroot/etc/y2linux/platform-contract').write_text('y2-platform-v1\n')
            (p/'newdata/.y2data-schema').write_text('2\n' if failure=='schema' else '1\n')
            (p/'newroot/sbin/init').write_text('#!/bin/sh\nexit 0\n');(p/'newroot/sbin/init').chmod(0o755)
            (p/'proc/cmdline').write_text(cmdline)
            (p/'proc/sys/kernel').mkdir(parents=True)
            (p/'proc/sys/kernel/panic').write_text('0\n')
            (p/'sys/firmware/y2_boot/normal_boot').write_text('0\n')
            (p/'display.ko').write_bytes(b'module fixture')
            (p/'boot-modules/fixture/kernel').mkdir(parents=True)
            (p/'boot-modules/fixture/kernel/g_ncm.ko').write_bytes(b'NCM fixture')
            (p/'boot-modules/fixture/modules.dep').write_text('kernel/g_ncm.ko: kernel/usb_f_ncm.ko\n')
            (p/'newdata/updates').mkdir()
            if failure.startswith('update-') or cmdline.endswith('y2.recovery=1'):
                (p/'newdata/updates/state.json').write_text('{"schema":2,"state":"PendingHealth"}')
            if failure == 'poweroff-mode':
                (p/'newdata/updates/restart.json').write_text('{"mode": "poweroff"}')
            script=(ROOT/'initramfs/production/init').read_text()
            # Replace the terminal rescue loop with a finite test exit, keeping
            # every call site and root handover branch as production implements it.
            start=script.index('rescue() {');end=script.index('\ncase ',start)
            script=script[:start]+"rescue() { echo \"RESCUE $*\" >> \"$LEDGER\"; exit 99; }\n"+script[end:]
            paths=lambda text:re.sub(r'(?<![A-Za-z0-9_./])/(?:sbin|bin|dev|proc|sys|run|tmp|newroot|newdata|oldroot|boot-modules|display\.ko)(?=[/\s"\']|$)',
                                     lambda m:str(p)+m.group(),text)
            script=paths(script)
            # The recovery image's script, with its two endless PID1 loops made finite.
            recovery=(ROOT/'initramfs/production/recovery.sh').read_text()
            recovery=recovery.replace('while :; do sleep 60; done','exit 98')
            start=recovery.index('    while :; do\n        /bin/busybox sh');end=recovery.index('    done\n',start)+9
            recovery=recovery[:start]+'    exit 97\n'+recovery[end:]
            (p/'sbin/y2-recovery').write_text(paths(recovery))
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
y2-offline-charge) [ "$FAILURE" != boot-metadata ] || exit 2;;
y2-update-core)
  case "$1:$FAILURE" in
  boot-needed:*) exit 0;;
  boot-check:update-recover) exit 20;;
  boot-check:update-attention) exit 11;;
  boot-check:*) exit 10;;
  recovery:recovery-stuck) echo '{"outcome":"rescue_required"}'; exit 1;;
  recovery:*) echo '{"outcome":"installed","state":"PendingHealth"}';;
  esac;;
poweroff) [ "$FAILURE" != poweroff-fails ] || exit 1;;
reboot) :;;
switch_root) [ "$FAILURE" != exec ] || exit 1; echo 'BUILDROOT INIT' >> "$LEDGER";;
esac
exit 0
'''
            for name in ('mount','umount','mknod','sleep','chroot','switch_root','e2fsck',
                         'y2-platform-start','y2-offline-charge','y2-abi-check','y2-usb-status','y2-status','y2-update-core',
                         'poweroff','reboot'):
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
            self.assertEqual((p/'run/y2/modules/fixture/kernel/g_ncm.ko').read_bytes(), b'NCM fixture')
            events=ledger.read_text().replace(str(p),'')
            log=(p/'dev/kmsg').read_text() if (p/'dev/kmsg').exists() else ''
            self.panic=(p/'proc/sys/kernel/panic').read_text()
            return result,events,log

    def test_no_sd_root_data_mounts_then_switch_root(self):
        result,events,log=self.exercise()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('switching to Buildroot',log)
        sequence=['y2-offline-charge', 'mount -t ext4 -o ro,noload /dev/mmcblk0p7 /newdata',
                  'umount /newdata', 'mount -t ext4 -o ro,noload /dev/mmcblk0p5 /newroot',
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
        for failure in ('missing','schema','fsck','boot-metadata'):
            with self.subTest(failure=failure):
                result,events,_=self.exercise(failure)
                self.assertEqual(result.returncode,99,result.stderr)
                self.assertIn('RESCUE ',events)
                self.assertNotIn('switch_root ',events)
                self.assertNotIn('mount -t ext4 -o rw ',events)
                if failure == 'boot-metadata': self.assertNotIn('mount -t ext4',events)

    def test_unfinished_update_is_decided_before_the_root_is_trusted(self):
        # OTA-35: the rescue never installs; it lets a new system start, routes
        # an unfinished one to the recovery image (power off) or stays in RAM.
        result, events, _ = self.exercise('update-continue')
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertLess(events.index('e2fsck -p /dev/mmcblk0p7'),events.index('y2-update-core boot-check'))
        self.assertLess(events.index('y2-update-core boot-check'),events.index('mount -t ext4 -o ro,noload /dev/mmcblk0p5 /newroot'))
        self.assertIn('BUILDROOT INIT',events)
        result, events, _ = self.exercise('update-recover')
        self.assertEqual(result.returncode,99,result.stderr)
        self.assertIn('poweroff -f',events)
        self.assertNotIn('mount -t ext4 -o ro,noload /dev/mmcblk0p5',events)
        self.assertNotIn('switch_root ',events)
        result, events, _ = self.exercise('update-attention')
        self.assertEqual(result.returncode,99,result.stderr)
        self.assertNotIn('mount -t ext4 -o rw /dev/mmcblk0p5',events)
        self.assertNotIn('poweroff',events)

    def test_recovery_image_installs_and_restarts_without_starting_the_system(self):
        # OTA-46: restart into the new system by default.
        result, events, log = self.exercise(cmdline='rdinit=/init y2.recovery=1')
        self.assertEqual(result.returncode,98,result.stderr)
        sequence=['y2-platform-start','e2fsck -p /dev/mmcblk0p7','mount -t ext4 -o rw,nosuid,nodev /dev/mmcblk0p7 /newdata',
                  'y2-update-core recovery','umount /newdata','reboot -f']
        positions=[events.index(event) for event in sequence]
        self.assertEqual(positions,sorted(positions))
        for absent in ('y2-offline-charge','switch_root','y2-abi-check'):
            self.assertNotIn(absent,events)
        # RELEASE-49: the old root is only read (its radio firmware kept on
        # Y2DATA) and released before the install writes it.
        self.assertEqual([l for l in events.splitlines() if 'mmcblk0p5' in l],
                         ['mount -t ext4 -o ro,noload /dev/mmcblk0p5 /oldroot'])
        self.assertLess(events.index('umount /oldroot'),events.index('y2-update-core recovery'))
        self.assertNotIn('poweroff',events.split('reboot -f')[0])
        # The owner's override turns off instead; Power starts the new system.
        result, events, log = self.exercise('poweroff-mode', cmdline='rdinit=/init y2.recovery=1')
        self.assertEqual(result.returncode,98,result.stderr)
        self.assertIn('poweroff -f',events)
        self.assertNotIn('reboot',events)
        self.assertIn('power off',log)   # the fixture kmsg keeps the last line only
        # A low battery without charger always turns off: a restart would loop.
        script=(ROOT/'initramfs/production/recovery.sh').read_text()
        self.assertRegex(script, r"charger_online \|\| \{[^}]*recovery_restart=; recovery_off 10; \}")
        # F34: the SoC watchdog is fed only while the installer runs and reports progress; a frozen
        # install restarts like a press of Power instead of staying on "Step 1 of 4" for hours.
        loop = script[script.index('while :; do\n    recovery_watchdog_start'):]
        self.assertLess(loop.index('recovery_watchdog_start'), loop.index('/sbin/y2-update-core recovery'))
        self.assertLess(loop.index('/sbin/y2-update-core recovery'), loop.index('recovery_watchdog_stop'))
        self.assertNotIn('echo 7 > /proc/sys/kernel/printk', script)
        self.assertLess(loop.index('recovery_watchdog_stop'), loop.index('until charger_online'))
        keeper = script[script.index('recovery_watchdog_start() {'):script.index('recovery_watchdog_stop() {')]
        self.assertIn('if [ "$still" -ge "$2" ]; then', keeper)
        self.assertIn('printf V >&3', keeper)   # magic close: a normal stop disarms it
        self.assertIn('recovery_watchdog_start /run/y2/recovery-progress 120', loop)
        # F34 gap (report 5497435a): a run froze after "init recovery", before the installer. The
        # watchdog runs from the first step; the installer's own keeper replaces it (one opener of
        # /dev/watchdog), and a stuck screen waits for a person without restarts behind it.
        early = script[script.index('y2_stage recovery\n'):script.index('while :; do\n    recovery_watchdog_start')]
        self.assertIn('recovery_watchdog_start /run/y2/recovery-step 300', early)
        self.assertLess(early.index('recovery_watchdog_start'), early.index('y2_find_partition Y2DATA'))
        self.assertLess(early.index('e2fsck -p'), early.index('recovery_watchdog_stop'))
        for step in ('storage_discovery', 'data_check', 'data_mount', 'radio_firmware'):
            self.assertIn('recovery_step ' + step, early)
        stuck_fn = script[script.index('recovery_stuck() {'):]
        self.assertLess(stuck_fn.index('recovery_watchdog_stop'), stuck_fn.index('recovery_screen'))
        self.assertIn('recovery_keep_previous_boot', script)
        # F23: the UI stops and the light goes out before reboot(2)/poweroff, as in a normal restart.
        off = script[script.index('recovery_off() {'):script.index('\n}\n', script.index('recovery_off() {'))]
        self.assertLess(off.index('recovery_display_off'), off.index('reboot -f'))
        self.assertIn('/sbin/y2-recovery-ui & recovery_ui=$!', script)
        result, events, log = self.exercise('recovery-stuck', cmdline='rdinit=/init y2.recovery=1')
        self.assertEqual(result.returncode,97,result.stderr)
        self.assertNotIn('reboot',events)
        # B8: stuck on battery, the player stays on for ten minutes (300 polls
        # of 2 s without a charger), then turns off; the request stays set.
        stuck = events.split('y2-update-core recovery')[1]
        self.assertEqual(stuck.split('poweroff -f')[0].count('sleep 2'), 300)
        self.assertTrue(stuck.rstrip().endswith('poweroff -f'))
        self.assertIn('power off', log)   # the fixture kmsg keeps the last line only
        # With a charger connected the count starts again: it never turns off.
        self.assertIn('if charger_online; then idle=0; else idle=$((idle+1)); fi', script)

    def test_recovery_screen_lines_fit_its_28_columns(self):
        # #55: the screen cuts every line after 28 characters ("This takes less than a minut").
        ui = (ROOT/'tools/update/recovery-ui.c').read_text()
        columns = int(re.search(r'#define COLUMNS (\d+)', ui).group(1))
        script = (ROOT/'initramfs/production/recovery.sh').read_text()
        calls = re.findall(r"recovery_screen ((?:'[^']*'|\"[^\"]*\"|\"\$1\")(?: (?:'[^']*'|\"[^\"]*\"))*)", script)
        self.assertGreater(len(calls), 10)
        for call in calls:
            for line in re.findall(r"'([^']*)'|\"([^\"]*)\"", call):
                text = line[0] or line[1]
                self.assertLessEqual(len(text), columns, text)
        for reason in re.findall(r"recovery_stuck '([^']*)'", script):
            self.assertLessEqual(len(reason), columns, reason)

    def test_explicit_rescue_never_mounts_a_filesystem(self):
        result,events,_=self.exercise(cmdline='rdinit=/init y2.rescue=1')
        self.assertEqual(result.returncode,99,result.stderr)
        self.assertNotIn('mount -t ext4',events)

    def test_running_system_restarts_after_a_panic_but_the_rescue_never_does(self):
        # Owner decision 2026-10-28: panic=10 from the handover on (the command
        # line's panic=0 stays for the rescue); a failed handover puts it back.
        result,events,_=self.exercise()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(self.panic,'10\n')
        for failure in ('move','exec','fsck'):
            with self.subTest(failure=failure):
                result,events,_=self.exercise(failure)
                self.assertEqual(result.returncode,99,result.stderr)
                self.assertEqual(self.panic,'0\n')

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


class InitramfsCommands(unittest.TestCase):
    def test_every_command_in_the_initramfs_scripts_exists_there(self):
        # The initramfs links only a few BusyBox applets. `touch` (8.23 test build: recovery never
        # restarted after an install), `head` and `date` (8.22 test build: empty health files) were
        # missing and failed silently. Call others through /bin/busybox.
        import re
        builder = (ROOT/'tools/build/dev_initramfs.py').read_text()
        linked = set(re.findall(r"'([a-z_0-9]+)'", builder[builder.index('for applet in ('):
                                                            builder.index("put('bin/'+applet")]))
        placed = {p.rsplit('/', 1)[1] for p in re.findall(r"(?:binary|put)\('((?:s?bin)/[A-Za-z0-9_.-]+)'", builder)}
        builtins = {'echo', 'printf', 'read', 'cd', 'test', 'true', 'false', 'exit', 'return', 'set', 'trap',
                    'wait', 'exec', 'shift', 'eval', 'export', 'local', 'unset', 'command', 'break', 'continue',
                    'umask', 'in', 'case', 'for'}
        keywords = {'if', 'then', 'else', 'elif', 'do', 'while', 'until', '!', '{', '}', 'fi', 'done', 'esac', '[', ':', '.'}
        scripts = {name: (ROOT/'initramfs/production'/name).read_text() for name in ('init', 'recovery.sh', 'storage.sh')}
        functions = {f for text in scripts.values() for f in re.findall(r'^\s*([A-Za-z_][A-Za-z0-9_]*)\(\)\s*\{', text, re.M)}
        missing = []
        for name, text in scripts.items():
            for number, line in enumerate(text.splitlines(), 1):
                if line.lstrip().startswith('#'):
                    continue
                labels = {w for m in re.findall(r'^\s*([^()\s]+)\)', line) for w in m.split('|')}
                code = re.sub(r'"(?:[^"\\]|\\.)*"|\'[^\']*\'', 'X', line)
                code = re.sub(r'\$\(\([^)]*\)\)|\$\{[^}]*\}|\[ [^]]* \]', 'X', code.split(' #', 1)[0])
                for segment in re.split(r'\$\(|`|;;|;|&&|\|\||\||\(|\)|&', code):
                    words = segment.split()
                    while words and (words[0] in keywords or '=' in words[0] or words[0][0] in '<>'):
                        words.pop(0)
                    if (not words or not re.fullmatch(r'[a-z][a-z0-9_.-]*', words[0]) or words[0] in labels or
                            re.fullmatch(r'[0-9a-f]{32,}', words[0])):
                        continue
                    word = words[0]
                    if word in builtins or word in functions or word in linked or word in placed:
                        continue
                    missing.append('%s:%d: %s' % (name, number, word))
        self.assertEqual(missing, [])
