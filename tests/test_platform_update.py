"""Actual Ed25519/gzip/journal/offline writer faults on private regular files.

OTA-35: the normal system queues a signed BOOTIMG + Y2ROOT (+ RECOVERY) package,
installs a newer recovery image and asks LK for RECOVERY through the MISC
message; the recovery installer backs up, writes, reads back and restores both
images. The production binary has neither a fixture root nor fault controls.
This suite never opens a block device and is not power-loss durability evidence.
The platform's side (`update` check/download/cancel/space, health, the automatic
check) is tested in platform/crates/y2-platform/src/commands/service/update_tests.rs.
"""
import copy
import json
import shutil
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest

from tools.build.bootimage import pack
from tools.update.package import build, canonical, keygen, sign

PROJECT = Path(__file__).resolve().parents[1]
BOOTIMG, RECOVERY, MISC = 60416*512, 93184*512, 138240*512
SPAN = 16*1024**2


def root_image(marker, size=8*1024**2):
    # A raw-file image fixture with the real ext4 identity and extent fields;
    # tests the stream boundary, not e2fsck or a simulated electrical device.
    raw = bytearray(size)
    struct.pack_into('<I', raw, 1024+4, len(raw)//4096)
    struct.pack_into('<I', raw, 1024+24, 2)
    struct.pack_into('<H', raw, 1024+56, 0xef53)
    raw[1024+104:1024+120] = bytes.fromhex('79324c696e7548018000000000000101')
    raw[1024+120:1024+127] = b'Y2ROOT\0'
    raw[1024*1024:1024*1024+len(marker)] = marker
    return bytes(raw)


class UpdateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tools = tempfile.TemporaryDirectory(prefix='y2-update-tools-')
        cls.executable = Path(cls.tools.name) / 'core'
        # Optional actual target compiler/QEMU command, selected only by host harness.
        cc = os.environ.get('Y2_UPDATE_TEST_CC', 'cc')
        subprocess.run([cc, '-O2', '-Wall', '-Wextra', '-Werror', '-DY2_UPDATE_TEST', '-pthread',
                        str(PROJECT / 'tools/update/core.c'), '-lsodium', '-ljson-c', '-lz',
                        '-o', str(cls.executable)], check=True)
        cls.prefix = json.loads(os.environ.get('Y2_UPDATE_TEST_PREFIX', '[]'))

    @classmethod
    def tearDownClass(cls):
        cls.tools.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='y2-update-fixture-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.private = self.root / 'secret' / 'key'
        public = keygen(self.private)
        self.base = self.root / 'data/updates'
        self.base.mkdir(parents=True, mode=0o700)
        (self.root/'data/.y2data-schema').write_text('1')
        (self.root / 'newdata').symlink_to('data')
        for name in ('etc/y2linux', 'proc/sys/kernel/random', 'run/y2'):
            (self.root / name).mkdir(parents=True)
        self.boot('a')
        (self.root / 'external-power').write_text('1')
        self.versions = dict(release_version='test-v1', kernel_version='6.18-test',
                             rootfs_version='test-root', build_git_commit='1'*40,
                             reborn_source_commit='2'*40)
        self.write('etc/y2linux/update-trust.json', dict(schema=1, keys=[dict(
            id='test', public_key_hex=public, status='active', purpose='development',
            minimum_sequence=1, maximum_sequence=100, allow_downgrade=True)]))
        self.write('etc/y2linux/update-compat.json', dict(schema=2, product='Y2',
                   hardware_revision='innioasis-y2-mt6582', rootfs_contract='y2-platform-v1',
                   layout_schema=1, data_schema=1, recovery_api=1, sequence=0))
        self.write('etc/y2linux/versions.json', self.versions)
        self.old = root_image(b'old-root-content')
        self.new = root_image(b'new-root-content')
        (self.root / 'root.img').write_bytes(self.old)
        (self.root / 'new.img').write_bytes(self.new)
        self.old_boot = pack(b'old-kernel'*100, b'old-ramdisk'*50)
        self.new_boot = pack(b'new-kernel'*120, b'new-ramdisk'*70)
        self.old_recovery = pack(b'old-kernel'*100, b'old-installer'*50, recovery=True)
        self.new_recovery = pack(b'new-kernel'*120, b'new-installer'*70, recovery=True)
        for name, data in (('new-boot.img', self.new_boot), ('new-recovery.img', self.new_recovery)):
            (self.root / name).write_bytes(data)
        # Sparse logical disk up to the end of MISC; stock MISC ENV block kept intact.
        with (self.root / 'disk.img').open('wb') as disk:
            disk.truncate(MISC + 512*1024)
            disk.seek(BOOTIMG); disk.write(self.old_boot)
            disk.seek(RECOVERY); disk.write(self.old_recovery)
            disk.seek(MISC + 0x20000); disk.write(b'ENV_v1\0\0data_free_size_th=52428800')
        self.env_block = self.region(MISC + 0x20000, 64)
        self.manifest = build(self.root / 'new.img', self.versions, self.base / 'pending',
                              self.private, 'test', 1, rollback_allowed=True,
                              bootimg=self.root/'new-boot.img', recovery=self.root/'new-recovery.img')

    def write(self, name, value):
        (self.root / name).write_bytes(canonical(value))

    def boot(self, name):
        (self.root / 'proc/sys/kernel/random/boot_id').write_text(name)

    def region(self, offset, length):
        with (self.root / 'disk.img').open('rb') as disk:
            disk.seek(offset)
            return disk.read(length)

    def span(self, offset, image):
        return self.region(offset, SPAN) == image + bytes(SPAN - len(image))

    def misc(self):
        message = self.region(MISC, 2048)
        command = message[:32].split(b'\0')[0]
        return (command, message[64:64+768].split(b'\0')[0])

    def journal(self):
        return json.loads((self.base/'state.json').read_text())

    def run_core(self, *args, expected=0, fault=None, recovery=False):
        env = os.environ.copy()
        env['Y2_TEST_ROOT'] = str(self.root)
        env.pop('Y2_TEST_RECOVERY', None)
        if recovery:
            env['Y2_TEST_RECOVERY'] = '1'
        if fault:
            env['Y2_TEST_FAULT'] = fault
        result = subprocess.run([*self.prefix, str(self.executable), *args], env=env,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, expected, result.stderr.decode()+result.stdout.decode())
        return json.loads(result.stdout) if result.stdout else None

    def resign(self, manifest):
        raw = canonical(manifest)
        (self.base / 'pending/manifest.json').write_bytes(raw)
        (self.base / 'pending/manifest.sig').write_bytes(sign(raw, self.private))

    def install(self):
        self.run_core('queue')
        self.run_core('prepare')
        self.boot('recovery-1')
        result = self.run_core('recovery', recovery=True)
        self.assertEqual(result['outcome'], 'installed')
        self.boot('new-1')
        return result

    def test_authentication_compatibility_and_corrupt_payload_fail_before_any_write(self):
        for field, value in [('product', 'wrong-device'), ('data_schema', 2), ('recovery_api', 9),
                             ('schema', 1), ('rootfs_contract', 'other'), ('layout_schema', 2)]:
            manifest = copy.deepcopy(self.manifest)
            manifest[field] = value
            self.resign(manifest)
            self.run_core('queue', expected=1)
        manifest = copy.deepcopy(self.manifest)
        del manifest['payloads']['bootimg']
        self.resign(manifest)
        self.run_core('queue', expected=1)
        manifest = copy.deepcopy(self.manifest)
        manifest['payloads']['extra'] = manifest['payloads']['rootfs']
        self.resign(manifest)
        self.run_core('queue', expected=1)
        self.resign(self.manifest)
        signature = self.base / 'pending/manifest.sig'
        signature.write_bytes(b'x'*64)
        self.run_core('queue', expected=1)
        self.resign(self.manifest)
        for name in ('rootfs.ext4.gz', 'bootimg.img.gz', 'recovery.img.gz'):
            payload = self.base / 'pending' / name
            good = payload.read_bytes()
            payload.write_bytes(good[:-5])
            self.run_core('queue', expected=1)
            payload.write_bytes(good[:-10]+b'corruption')
            self.run_core('queue', expected=1)
            payload.write_bytes(good)
        self.assertEqual((self.root / 'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        self.assertFalse((self.base / 'state.json').exists())

    def test_a_boot_image_in_the_wrong_slot_is_refused(self):
        # A BOOTIMG offered as recovery (or the reverse) is never written.
        for payload, swapped in (('bootimg', self.new_recovery), ('recovery', self.new_boot)):
            source = self.root / ('swap-' + payload)
            source.write_bytes(swapped)
            with self.assertRaises(ValueError):
                build(self.root/'new.img', self.versions, self.root/('out-'+payload), self.private, 'test', 2,
                      bootimg=source if payload == 'bootimg' else self.root/'new-boot.img',
                      recovery=source if payload == 'recovery' else None)
        # The core checks independently of the host tool.
        import gzip, hashlib
        manifest = copy.deepcopy(self.manifest)
        data = gzip.compress(self.new_recovery, mtime=0)
        (self.base/'pending/bootimg.img.gz').write_bytes(data)
        manifest['payloads']['bootimg'].update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
                                               raw_bytes=len(self.new_recovery),
                                               raw_sha256=hashlib.sha256(self.new_recovery).hexdigest())
        self.resign(manifest)
        self.assertIn('identity', self.run_core('queue', expected=1)['failure'])

    def test_queue_reports_its_own_check_progress(self):
        # OTA-48: the queue decodes and checks every payload once and reports its own
        # percentage to Reborn (the core's part of the platform's download test).
        self.run_core('queue')
        self.assertEqual((self.root/'run/y2/update-check-progress').read_text(), '100 check\n')

    def test_full_install_writes_both_images_reads_back_and_clears_request(self):
        self.run_core('queue')
        self.assertEqual(self.journal()['state'], 'Queued')
        prepared = self.run_core('prepare')
        # The newer installer is written first by the running (BOOTIMG) system.
        self.assertTrue(self.span(RECOVERY, self.new_recovery))
        self.assertEqual(prepared['recovery_installed'], self.manifest['payloads']['recovery']['raw_sha256'])
        self.assertEqual(self.misc(), (b'boot-recovery', b'recovery\n--y2-update\n'))
        self.assertEqual(self.region(MISC + 0x20000, 64), self.env_block)
        self.run_core('recovery', expected=1)          # not the recovery environment
        self.boot('recovery-1')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        self.assertEqual((self.root / 'root.img').read_bytes(), self.new)
        self.assertTrue(self.span(BOOTIMG, self.new_boot))
        self.assertEqual(self.misc(), (b'', b''))
        self.assertEqual(self.journal()['state'], 'PendingHealth')
        self.assertEqual((self.root/'run/y2/recovery-progress').read_text(), '100 installed\n')
        # First boot of the new system: rescue lets it start and counts the boot.
        self.boot('new-1')
        self.assertEqual(self.run_core('boot-check', expected=10)['health_boots'], 1)
        self.run_core('ack', expected=1)
        self.write('run/y2/application-ready.json', {'boot_id': 'wrong'})
        self.run_core('ack', expected=1)
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.assertEqual(self.run_core('ack')['state'], 'Acknowledged')
        self.assertEqual(json.loads((self.base/'accepted.json').read_text())['current_sequence'], 1)
        self.boot('b')
        self.assertEqual(self.run_core('boot-check', expected=10)['state'], 'Acknowledged')
        # Release key rejects replay even if a signed manifest requests downgrade.
        trust = json.loads((self.root / 'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0]['purpose'] = 'release'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue', expected=1)

    def test_recovery_of_the_new_release_installs_its_own_release(self):
        # Found on the device (35 -> 36): the recovery image written by prepare
        # is the new release's, so its compat file names the package's sequence.
        trust = json.loads((self.root/'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0].update(purpose='release', allow_downgrade=False)
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue')
        self.run_core('prepare')
        compat = json.loads((self.root/'etc/y2linux/update-compat.json').read_text())
        self.write('etc/y2linux/update-compat.json', dict(compat, sequence=1))   # the recovery's own files
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        self.assertEqual((self.root/'root.img').read_bytes(), self.new)
        # A replay is still refused where it belongs: in the normal system.
        self.boot('new-1')
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.run_core('ack')
        build(self.root/'new.img', self.versions, self.base/'pending', self.private, 'test', 1,
              bootimg=self.root/'new-boot.img', recovery=self.root/'new-recovery.img')
        self.assertIn('replay', self.run_core('queue', expected=1)['failure'])

    def test_only_changed_chunks_are_written_and_steps_are_logged(self):
        (self.root/'dev').mkdir()
        (self.root/'dev/kmsg').write_text('')
        self.install()
        changed = self.journal()['root_bytes_written']
        self.assertGreater(changed, 0)
        self.assertLess(changed, len(self.new) // 4)     # only the chunks that differ
        lines = (self.root/'dev/kmsg').read_text().splitlines()
        steps = [line.split()[1] for line in lines]
        self.assertEqual(steps, ['step=verify', 'step=backup', 'step=system', 'step=check',
                                 'step=boot', 'step=installed'])
        # OTA-50: each step but the last carries the installer's estimate,
        # which only falls here (a small root: every phase beats its model).
        left = [int(line.split('left=')[1].rstrip('s')) for line in lines[:-1]]
        self.assertEqual(left, sorted(left, reverse=True))
        self.assertTrue(all(1 <= s <= 10 for s in left), left)
        self.assertNotIn('left=', lines[-1])
        # Installing the same release again (development reinstall, as the device
        # test package) writes nothing and still reads back.
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.run_core('ack')
        build(self.root/'new.img', self.versions, self.base/'again', self.private, 'test', 1,
              allow_downgrade=True, bootimg=self.root/'new-boot.img', recovery=self.root/'new-recovery.img')
        self.assertFalse((self.base/'pending').exists())   # kept as current/ by the ack
        (self.base/'again').rename(self.base/'pending')
        self.run_core('queue')
        self.run_core('prepare')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        self.assertEqual(self.journal()['root_bytes_written'], 0)
        self.assertEqual((self.root/'root.img').read_bytes(), self.new)

    def test_a_payload_changed_after_queueing_is_refused_before_any_write(self):
        self.run_core('queue')
        self.run_core('prepare')
        payload = self.base/'pending/rootfs.ext4.gz'
        data = bytearray(payload.read_bytes())
        data[len(data)//2] ^= 1
        payload.write_bytes(bytes(data))
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'failed_unchanged')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        self.assertFalse((self.base/'previous.ext4.gz').exists())

    def test_backups_recorded_before_38_still_restore(self):
        import hashlib
        self.run_core('queue')
        self.run_core('prepare')
        self.run_core('recovery', expected=91, fault='write_interrupt', recovery=True)
        journal = self.journal()
        root = journal['backup']['root']
        del root['raw_crc32']
        root['raw_sha256'] = hashlib.sha256(self.old).hexdigest()
        boot = journal['backup']['bootimg']
        del boot['raw_crc32']
        boot['raw_sha256'] = hashlib.sha256(self.old_boot + bytes(SPAN - len(self.old_boot))).hexdigest()
        (self.base/'state.json').write_text(json.dumps(journal))
        self.boot('recovery-2')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        # A wrong legacy hash is caught before it is trusted.
        self.run_core('queue')
        self.run_core('prepare')
        self.run_core('recovery', expected=91, fault='write_interrupt', recovery=True)
        journal = self.journal()
        del journal['backup']['root']['raw_crc32']
        journal['backup']['root']['raw_sha256'] = '0' * 64
        (self.base/'state.json').write_text(json.dumps(journal))
        self.assertEqual(self.run_core('recovery', expected=1, recovery=True)['outcome'], 'rescue_required')

    def test_an_unchanged_recovery_image_is_not_rewritten(self):
        with (self.root/'disk.img').open('r+b') as disk:
            disk.seek(RECOVERY); disk.write(self.new_recovery)
        self.run_core('queue')
        self.assertNotIn('recovery_installed', self.run_core('prepare'))

    def test_interrupted_installs_restore_both_images_on_the_next_recovery_boot(self):
        for fault, code in (('write_interrupt', 91), ('boot_interrupt', 92)):
            self.run_core('queue')
            self.run_core('prepare')
            self.boot('recovery-1')
            self.run_core('recovery', expected=code, fault=fault, recovery=True)
            self.assertEqual(self.journal()['state'], 'Installing')
            self.assertEqual(self.misc()[0], b'boot-recovery')   # power loss returns to recovery
            self.boot('recovery-2')
            self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
            self.assertEqual((self.root/'root.img').read_bytes(), self.old)
            self.assertTrue(self.span(BOOTIMG, self.old_boot))
            self.assertEqual(self.journal()['state'], 'RolledBack')
            self.assertEqual(self.misc(), (b'', b''))

    def test_readback_failure_restores_and_bad_backup_requires_rescue_without_loop(self):
        self.run_core('queue')
        self.run_core('prepare')
        self.assertEqual(self.run_core('recovery', fault='readback', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        self.run_core('queue')
        self.run_core('prepare')
        self.run_core('recovery', expected=91, fault='write_interrupt', recovery=True)
        (self.base/'previous.ext4.gz').write_bytes(b'bad')
        self.boot('b')
        self.assertEqual(self.run_core('recovery', expected=1, recovery=True)['outcome'], 'rescue_required')
        self.assertEqual(self.journal()['state'], 'RescueRequired')
        before = (self.root/'root.img').read_bytes()
        self.run_core('recovery', expected=1, recovery=True)
        self.assertEqual((self.root/'root.img').read_bytes(), before)
        self.assertEqual(self.run_core('boot-check', expected=11)['state'], 'RescueRequired')

    def test_unconfirmed_health_routes_to_recovery_and_restores(self):
        self.install()
        for boot in ('new-1', 'new-2'):
            self.boot(boot)
            self.run_core('boot-check', expected=10)
        self.boot('new-3')
        result = self.run_core('boot-check', expected=20)
        self.assertEqual(result['state'], 'RollbackPending')
        self.assertEqual(self.misc()[0], b'boot-recovery')
        self.boot('recovery-2')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        self.assertEqual(json.loads((self.base/'accepted.json').read_text())['current_sequence'], 0)

    def test_owner_rollback_after_acknowledgement(self):
        self.install()
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.run_core('ack')
        self.assertEqual(self.run_core('rollback')['state'], 'RollbackPending')
        self.run_core('prepare')
        self.assertEqual(self.misc()[0], b'boot-recovery')
        self.boot('recovery-2')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))

    def next_release(self):
        # Release 1 installed and acknowledged; the running system's own files
        # name it. Release 2 is queued for the next install.
        self.install()
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.run_core('ack')
        compat = json.loads((self.root/'etc/y2linux/update-compat.json').read_text())
        self.write('etc/y2linux/update-compat.json', dict(compat, sequence=1))
        third = root_image(b'third-root-content')
        (self.root/'third.img').write_bytes(third)
        versions = dict(self.versions, release_version='test-v2', build_git_commit='3'*40)
        build(self.root/'third.img', versions, self.base/'pending', self.private, 'test', 2,
              rollback_allowed=True, bootimg=self.root/'new-boot.img', recovery=self.root/'new-recovery.img')
        self.write('etc/y2linux/versions.json', self.versions)
        return third

    def test_the_kept_package_is_the_next_rollback_copy(self):
        # OTA-50: the acknowledged package stays as current/ without its boot
        # images, and the next install uses its root payload as the backup.
        import hashlib
        self.install()
        self.write('run/y2/application-ready.json', {'boot_id': 'new-1'})
        self.run_core('ack')
        self.assertFalse((self.base/'pending').exists())
        self.assertEqual(sorted(os.listdir(self.base/'current')), ['manifest.json', 'manifest.sig', 'rootfs.ext4.gz'])
        kept = (self.base/'current/rootfs.ext4.gz').read_bytes()
        self.assertEqual(hashlib.sha256(kept).hexdigest(), self.manifest['payloads']['rootfs']['sha256'])

    def test_the_kept_package_restores_the_running_release(self):
        root = self.manifest['payloads']['rootfs']
        third = self.next_release()
        self.assertIn('kept_manifest_sha256', self.run_core('queue'))
        self.run_core('prepare')
        self.boot('recovery-2')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        record = self.journal()['backup']['root']
        self.assertEqual((record['source'], record['sha256'], record['raw_crc32']),
                         ('package', root['sha256'], root['raw_crc32']))
        self.assertFalse((self.base/'current').exists())
        self.assertEqual((self.root/'root.img').read_bytes(), third)
        # Each phase's time is in the journal (the step lines only mark starts).
        self.assertEqual(sorted(self.journal()['timings']),
                         ['backup_boot', 'backup_root', 'boot', 'check', 'system', 'verify'])
        self.assertEqual(self.run_core('rollback')['state'], 'RollbackPending')
        self.run_core('prepare')
        self.boot('recovery-3')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.new)
        self.assertTrue(self.span(BOOTIMG, self.new_boot))
        self.assertEqual(sorted(self.journal()['timings']), ['restore', 'restore_boot', 'restore_check'])

    def test_a_kept_package_of_another_release_is_dropped(self):
        # Another running build (e.g. flashed by computer): dropped at queue.
        third = self.next_release()
        self.write('etc/y2linux/versions.json', dict(self.versions, reborn_source_commit='4'*40))
        self.assertNotIn('kept_manifest_sha256', self.run_core('queue'))
        self.assertFalse((self.base/'current').exists())
        self.run_core('prepare')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        self.assertNotIn('source', self.journal()['backup']['root'])
        self.assertEqual((self.root/'root.img').read_bytes(), third)

    def test_a_kept_package_changed_after_queueing_falls_back_to_the_live_backup(self):
        third = self.next_release()
        self.run_core('queue')
        payload = self.base/'current/rootfs.ext4.gz'
        data = bytearray(payload.read_bytes())
        data[len(data)//2] ^= 1
        payload.write_bytes(bytes(data))
        self.run_core('prepare')
        self.boot('recovery-2')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')
        self.assertNotIn('source', self.journal()['backup']['root'])
        self.assertFalse((self.base/'current').exists())
        self.assertEqual((self.root/'root.img').read_bytes(), third)
        self.run_core('rollback')
        self.run_core('prepare')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), self.new)

    def test_a_root_of_several_batches_installs_and_restores(self):
        # 20 segments: three batches of the four-core pipeline in every step.
        size = 20*1024**2
        old, new = bytearray(root_image(b'old', size)), bytearray(root_image(b'new', size))
        for at in range(0, size, 3*1024**2 + 4099):
            old[at+8192:at+8200] = b'old-data'
            new[at+8192:at+8200] = b'new-data'
        (self.root/'root.img').write_bytes(old)
        (self.root/'new.img').write_bytes(new)
        shutil.rmtree(self.base/'pending')
        build(self.root/'new.img', self.versions, self.base/'pending', self.private, 'test', 1,
              rollback_allowed=True, bootimg=self.root/'new-boot.img', recovery=self.root/'new-recovery.img')
        self.install()
        self.assertEqual((self.root/'root.img').read_bytes(), new)
        self.assertLess(self.journal()['root_bytes_written'], size // 4)
        self.assertEqual(len(self.journal()['backup']['root']['segments']), 20)
        self.run_core('rollback')
        self.run_core('prepare')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'rolled_back')
        self.assertEqual((self.root/'root.img').read_bytes(), old)

    def test_the_segment_index_is_signed_and_checked(self):
        import gzip, zlib
        root = self.manifest['payloads']['rootfs']
        data = (self.base/'pending/rootfs.ext4.gz').read_bytes()
        self.assertEqual(gzip.decompress(data), self.new)
        self.assertEqual((root['segment_bytes'], len(root['segments'])), (1024**2, 8))
        self.assertEqual(sum(root['segments']) + 18, root['bytes'])
        self.assertEqual(root['raw_crc32'], zlib.crc32(self.new))
        lengths = root['segments']
        for wrong in ([lengths[0] + 1, lengths[1] - 1, *lengths[2:]], [*lengths[:-1], lengths[-1] + 1],
                      [lengths[0] + lengths[1], *lengths[2:]], [*lengths[:-1], lengths[-1] - 1, 1]):
            manifest = copy.deepcopy(self.manifest)
            manifest['payloads']['rootfs']['segments'] = wrong
            self.resign(manifest)
            with self.subTest(wrong=wrong):
                self.run_core('queue', expected=1)
        manifest = copy.deepcopy(self.manifest)
        manifest['payloads']['rootfs']['segment_bytes'] = 2*1024**2
        self.resign(manifest)
        self.run_core('queue', expected=1)
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)

    def test_lost_request_never_loops(self):
        # LK booted BOOTIMG although the request was set: nothing was written,
        # so the update is abandoned and the intact system starts.
        self.run_core('queue')
        self.run_core('prepare')
        result = self.run_core('boot-check', expected=10)
        self.assertEqual(result['state'], 'Failed')
        self.assertEqual(self.misc(), (b'', b''))
        # An interrupted install whose request vanished asks again, three times at most.
        self.run_core('queue')
        self.run_core('prepare')
        self.run_core('recovery', expected=91, fault='write_interrupt', recovery=True)
        for expected in (20, 20, 11):   # prepare already asked once
            with (self.root/'disk.img').open('r+b') as disk:
                disk.seek(MISC); disk.write(bytes(2048))
            self.run_core('boot-check', expected=expected)
        self.assertEqual(self.journal()['state'], 'RescueRequired')

    def test_probe_and_stale_requests(self):
        self.assertEqual(self.run_core('misc')['recovery_image_valid'], True)
        self.run_core('probe')
        self.assertEqual(self.misc(), (b'boot-recovery', b'recovery\n--y2-probe\n'))
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'probe')
        self.assertEqual(json.loads((self.base/'recovery-probe.json').read_text())['result'], 'recovery_started')
        self.assertEqual(self.misc(), (b'', b''))
        # A recovery entry with nothing unfinished just clears the request.
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'nothing')
        # No request is ever made without a Y2 recovery image in RECOVERY.
        with (self.root/'disk.img').open('r+b') as disk:
            disk.seek(RECOVERY); disk.write(self.old_boot)
        self.assertEqual(self.run_core('misc')['recovery_image_valid'], False)
        self.run_core('probe', expected=1)
        self.assertEqual(self.misc(), (b'', b''))

    def test_backup_space_and_battery_failures_leave_both_images(self):
        self.run_core('queue')
        self.run_core('prepare')
        result = self.run_core('recovery', fault='reserve', recovery=True)
        self.assertEqual(result['outcome'], 'failed_unchanged')
        self.assertIn('reserve', result['failure'])
        self.assertEqual(self.misc(), (b'', b''))
        self.run_core('queue')
        self.run_core('prepare')
        (self.root/'external-power').write_text('0')
        self.assertEqual(self.run_core('recovery', expected=1, recovery=True)['outcome'], 'needs_power')
        self.assertEqual(self.misc()[0], b'boot-recovery')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))
        self.assertEqual(self.journal()['state'], 'Queued')
        (self.root/'external-power').write_text('1')
        self.assertEqual(self.run_core('recovery', recovery=True)['outcome'], 'installed')

    def test_key_revocation_journal_corruption_and_symlinks(self):
        trust = json.loads((self.root / 'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0]['status'] = 'revoked'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue', expected=1)
        trust['keys'][0]['status'] = 'active'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue')
        (self.base/'state.json').write_text('{broken')
        self.run_core('status', expected=1)
        self.run_core('boot-check', expected=1)
        (self.base/'state.json').unlink()
        (self.base/'state.json').symlink_to(self.root/'secret/key')
        self.run_core('status', expected=1)
        self.assertEqual((self.root / 'root.img').read_bytes(), self.old)
        self.assertTrue(self.span(BOOTIMG, self.old_boot))



if __name__ == '__main__':
    unittest.main()
