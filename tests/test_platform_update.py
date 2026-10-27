"""Actual Ed25519/gzip/journal/offline writer faults on private regular files.

OTA-35: the normal system queues a signed BOOTIMG + Y2ROOT (+ RECOVERY) package,
installs a newer recovery image and asks LK for RECOVERY through the MISC
message; the recovery installer backs up, writes, reads back and restores both
images. The production binary has neither a fixture root nor fault controls.
This suite never opens a block device and is not power-loss durability evidence.
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
import unittest.mock
from unittest.mock import patch
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools/platform"))

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

    def test_download_stage_authenticates_before_payload_and_never_queues_partial(self):
        from y2_platform.common import Context
        from y2_platform.update import stage, base_url
        source = self.root/'package'
        (self.base/'pending').rename(source)
        volume = {'path':'/data','state':'Ready','generation':'boot:1:179:7'}
        def runner(argv, **kw):
            result = subprocess.run([*self.prefix,str(self.executable),*argv[1:]],
                env=os.environ|{'Y2_TEST_ROOT':str(self.root)},capture_output=True,timeout=30)
            return {'ok':result.returncode==0,'output':result.stdout.decode(),'reason':None if result.returncode==0 else 'command_failed'}
        ctx=Context(self.root,runner)
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}):
            checked=stage(ctx,source,check_only=True)
            self.assertEqual(checked['sequence'],1)
            self.assertFalse((self.base/'state.json').exists())
            for name in ('rootfs.ext4.gz','bootimg.img.gz'):
                payload=source/name; original=payload.read_bytes()
                payload.write_bytes(original[:-10])
                with self.assertRaisesRegex(ValueError,'incomplete_download'):
                    stage(ctx,source)
                self.assertEqual(json.loads((self.root/'run/y2/update-progress.json').read_text())['phase'],'failed')
                self.assertFalse((self.base/'state.json').exists())
                payload.write_bytes(original)
            self.assertEqual(stage(ctx,source)['state'],'Queued')
            progress=json.loads((self.base/'download.json').read_text())
            self.assertEqual(progress['state'],'Verified')
            # OTA-48: live progress for Reborn in RAM, and the one decoding
            # check (the queue's) reports its own percentage.
            live=json.loads((self.root/'run/y2/update-progress.json').read_text())
            self.assertEqual((live['phase'],live['bytes_done']),('done',live['bytes_total']))
            self.assertGreater(live['bytes_total'],0)
            self.assertEqual((self.root/'run/y2/update-check-progress').read_text(),'100 check\n')
            # The platform reads the core's own journal (schema 2) as the queued update.
            from y2_platform.update import status
            self.assertEqual(status(ctx)['state'],'Queued')
        self.assertEqual((self.root/'root.img').read_bytes(),self.old)
        # The installed release offered again: a check says up to date, not a failure.
        self.write('etc/y2linux/versions.json', dict(self.versions, release_version='test-v1'))
        trust=json.loads((self.root/'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0]['purpose']='release'
        self.write('etc/y2linux/update-trust.json', trust)
        compat=json.loads((self.root/'etc/y2linux/update-compat.json').read_text())
        compat['sequence']=1
        self.write('etc/y2linux/update-compat.json', compat)
        (self.base/'state.json').write_text(json.dumps({'schema':2,'state':'Acknowledged'}))
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}):
            self.assertEqual(stage(ctx,source,check_only=True)['state'],'UpToDate')
        checked=json.loads((self.base/'download.json').read_text())
        self.assertTrue(checked['up_to_date'])
        self.assertEqual(checked['release_version'],'test-v1')
        for url in ('http://example.com/manifest.json','https://u:p@example.com/manifest.json',
                    'https://example.com/manifest.json?redirect=other','https://example.com/other'):
            with self.assertRaises(ValueError): base_url(url)
        self.assertEqual(base_url('https://example.com/release/manifest.json'),'https://example.com/release/')

    def test_cancel_stops_a_running_download_and_offers_the_release_again(self):
        # Beta 7.1: Reborn's Cancel sends SIGTERM to `update stage`. The
        # download stops, its partial files go and the page shows the release
        # as available, exactly as after the check.
        import signal
        from y2_platform.common import Context
        from y2_platform import update
        source = self.root/'package'
        (self.base/'pending').rename(source)
        volume = {'path':'/data','state':'Ready','generation':'boot:1:179:7'}
        def runner(argv, **kw):
            result = subprocess.run([*self.prefix,str(self.executable),*argv[1:]],
                env=os.environ|{'Y2_TEST_ROOT':str(self.root)},capture_output=True,timeout=30)
            return {'ok':result.returncode==0,'output':result.stdout.decode(),'reason':None if result.returncode==0 else 'command_failed'}
        ctx=Context(self.root,runner)
        copy = update.copy_stream
        def interrupted(*args, **kwargs):
            # Cancel pressed mid-download (the manifest is read before).
            if signal.getsignal(signal.SIGTERM) is update._cancel:
                os.kill(os.getpid(), signal.SIGTERM)
            return copy(*args, **kwargs)
        before = signal.getsignal(signal.SIGTERM)
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}), \
                patch('y2_platform.update.copy_stream', side_effect=interrupted):
            with self.assertRaisesRegex(ValueError,'download_cancelled'):
                update.stage(ctx,source)
        self.assertEqual(signal.getsignal(signal.SIGTERM), before)
        self.assertEqual(list(self.base.glob('.incoming-*')), [])
        self.assertFalse((self.base/'pending').exists())
        self.assertFalse((self.base/'state.json').exists())
        offered=json.loads((self.base/'download.json').read_text())
        self.assertEqual((offered['state'],offered['sequence']),('Checked',1))
        # Nothing was changed: the same download works afterwards.
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}):
            self.assertEqual(update.stage(ctx,source)['state'],'Queued')

    def test_space_cleanup_stale_package_and_cancel_free_the_data_volume(self):
        # OTA-46: disposable data goes before the download is refused; music,
        # settings, live logs and the rollback copies stay.
        from types import SimpleNamespace
        from y2_platform.common import Context
        from y2_platform import update
        source = self.root/'package'
        (self.base/'pending').rename(source)
        volume = {'path':'/data','state':'Ready','generation':'boot:1:179:7'}
        def runner(argv, **kw):
            result = subprocess.run([*self.prefix,str(self.executable),*argv[1:]],
                env=os.environ|{'Y2_TEST_ROOT':str(self.root)},capture_output=True,timeout=30)
            return {'ok':result.returncode==0,'output':result.stdout.decode(),'reason':None if result.returncode==0 else 'command_failed'}
        ctx = Context(self.root, runner)
        logs = self.root/'data/logs'; logs.mkdir()
        platform = self.root/'data/system/platform'; platform.mkdir(parents=True)
        kept = [logs/'system.log', logs/'recovery-last.log', platform/'power-journal.jsonl',
                self.root/'data/settings.json']
        for path in kept:
            path.write_text('keep')
        backup = self.base/'previous.ext4.gz'
        backup.write_bytes(b'b'*5000)
        def disposable():
            (logs/'deepsleep-old').mkdir(exist_ok=True)
            (logs/'deepsleep-old'/'rows').write_bytes(b'x'*1000)
            (logs/'system.log.0').write_text('rotated')
            (platform/'power-journal.previous.jsonl').write_text('rotated')
            (platform/'fix02-abc').mkdir(exist_ok=True)
        disposable()
        # A package left by an earlier cancelled/failed update is replaced first.
        shutil.copytree(source, self.base/'pending')
        def fits_once_cleaned(fd):
            space = 0 if (logs/'deepsleep-old').exists() else 1 << 40
            return SimpleNamespace(f_flag=0, f_frsize=1, f_bavail=space, f_files=0, f_favail=0)
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}), \
             patch('y2_platform.update.os.fstatvfs', side_effect=fits_once_cleaned):
            self.assertEqual(update.stage(ctx, source)['state'], 'Queued')
        # Only as much as needed: the first entry was enough.
        self.assertFalse((logs/'deepsleep-old').exists())
        self.assertTrue((logs/'system.log.0').exists())
        self.assertGreaterEqual(json.loads((self.base/'download.json').read_text())['cleaned_bytes'], 1000)
        # Cancel frees the package at once.
        self.assertEqual(update.cancel(ctx)['state'], 'Failed')
        self.assertFalse((self.base/'pending').exists())
        # Still too little space: everything disposable goes, then a clear refusal
        # that names the total the update needs (the rollback copy counts as free).
        disposable()
        # The kept package of the running release (OTA-50) goes only as the last step.
        current = self.base/'current'
        current.mkdir()
        for name in ('manifest.json', 'manifest.sig', 'rootfs.ext4.gz'):
            shutil.copy(source/name, current/name)
        manifest = json.loads((source/'manifest.json').read_text())
        payloads = manifest['payloads']
        need = (sum(item['bytes'] for item in payloads.values()) + payloads['rootfs']['raw_bytes']//4 +
                update.RESERVE - 5000)
        # Room for the manifest, not for the payloads.
        never = lambda fd: SimpleNamespace(f_flag=0, f_frsize=1, f_bavail=update.RESERVE + (1 << 20),
                                           f_files=0, f_favail=0)
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}), \
             patch('y2_platform.update.os.fstatvfs', side_effect=never):
            with self.assertRaises(update.NoSpace):
                update.stage(ctx, source)
        failed = json.loads((self.base/'download.json').read_text())
        self.assertEqual((failed['failure'], failed['needed_bytes']), ('update_data_space_reserve', need))
        for path in (logs/'deepsleep-old', logs/'system.log.0', platform/'power-journal.previous.jsonl',
                     platform/'fix02-abc', current):
            self.assertFalse(path.exists(), path)
        for path in kept + [backup]:
            self.assertTrue(path.exists(), path)
        self.assertFalse(any(self.base.glob('.incoming-*')))

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



class PlatformPolicyTests(unittest.TestCase):
    """update.py and service.py decisions that need no native core."""
    def setUp(self):
        from y2_platform.common import Context
        self.temp = tempfile.TemporaryDirectory(prefix='y2-update-policy-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ('etc/y2linux', 'proc/sys/kernel/random', 'run/y2', 'data/updates'):
            (self.root/name).mkdir(parents=True)
        (self.root/'data/updates').chmod(0o700)
        (self.root/'proc/sys/kernel/random/boot_id').write_text('boot-a\n')
        (self.root/'data/.y2data-schema').write_text('1')
        (self.root/'etc/y2linux/update-channel.json').write_text(json.dumps({
            'schema': 1, 'name': 'stable', 'manifest_url': 'https://github.com/o/r/releases/latest/download/manifest.json',
            'allowed_redirect_hosts': ['github.com', 'release-assets.githubusercontent.com']}))
        self.calls = []
        def runner(argv, **kw):
            self.calls.append(argv)
            return {'ok': False, 'output': '', 'reason': 'not_run'}
        self.ctx = Context(self.root, runner)

    def power(self, **battery):
        (self.root/'run/y2/power.json').write_text(json.dumps({'boot_id': 'boot-a', 'battery': battery}))

    def test_health_uses_reborns_verdict_and_ignores_unreadable_songs(self):
        from y2_platform.update import application_healthy
        # B3: one song that cannot be read must never roll an update back.
        incomplete = {'healthy': True, 'graphics': {'available': True}, 'uptime_seconds': 40,
                      'library': {'error': 'scan incomplete; retained existing entries (1 traversal failures)'}}
        self.assertTrue(application_healthy(incomplete))
        self.assertFalse(application_healthy({**incomplete, 'healthy': False,
                                              'health_reasons': ['database failed']}))
        self.assertFalse(application_healthy({**incomplete, 'healthy': 'yes'}))
        older = {'graphics': {'available': True}, 'uptime_seconds': 40, 'library': {'error': 'x'}}
        self.assertTrue(application_healthy(older))
        self.assertFalse(application_healthy({**older, 'uptime_seconds': 3}))
        self.assertFalse(application_healthy({}))
        self.assertFalse(application_healthy(None))

    def test_a_download_whose_process_is_gone_shows_as_interrupted(self):
        # A user's 8.17 player said "Downloading Update… Keep Wi-Fi on." for a day: the
        # record stayed Staging after the download died, and that page had no way out.
        from y2_platform.update import status, automatic_check
        import os
        records = self.root/'data/updates'
        (records/'download.json').write_text(json.dumps({'schema': 1, 'state': 'Staging', 'id': '.incoming-x'}))
        lock = records/'.stage.lock'
        lock.write_text('')
        self.assertEqual(status(self.ctx)['download'], {'schema': 1, 'state': 'Failed', 'failure': 'download_interrupted'})
        # A live download (its flock in /proc/locks) is still shown as running.
        meta = lock.stat()
        (self.root/'proc/locks').write_text('1: FLOCK  ADVISORY  WRITE 4242 %02x:%02x:%d 0 EOF\n'
                                            % (os.major(meta.st_dev), os.minor(meta.st_dev), meta.st_ino))
        self.assertEqual(status(self.ctx)['download']['state'], 'Staging')
        # A failed background check never writes the leftover back.
        (self.root/'proc/locks').write_text('')
        (self.root/'run/y2/network.json').write_text(json.dumps({'state': 'Online'}))
        from unittest.mock import patch
        with patch('y2_platform.timekeeping.status', return_value={'tls_ready': True, 'entropy_ready': True}), \
             patch('y2_platform.update.stage', side_effect=ValueError('name resolution')):
            automatic_check(self.ctx, now=10**9)
        self.assertNotEqual((self.ctx.json('/data/updates/download.json') or {}).get('state'), 'Staging')

    def test_confirmed_health_restores_the_health_boot_kernel_settings(self):
        # F23: init raises the console level and turns a CPU stall into a panic until health is confirmed.
        from y2_platform.update import restore_health_debug
        kernel = self.root/'proc/sys/kernel'
        kernel.mkdir(parents=True, exist_ok=True)
        for name, value in (('printk', '7'), ('panic', '10'), ('panic_on_rcu_stall', '1')):
            (kernel/name).write_text(value + '\n')
        # As init wrote it on the first test build: a raw tab inside the JSON string.
        (self.root/'run/y2/health-debug.json').write_text(
            '{"printk":"3\t4\t1\t7","panic":"0","panic_on_rcu_stall":"0; reboot"}\n')
        restore_health_debug(self.ctx)
        self.assertEqual((kernel/'printk').read_text(), '3\t4\t1\t7\n')
        self.assertEqual((kernel/'panic').read_text(), '0\n')
        self.assertEqual((kernel/'panic_on_rcu_stall').read_text(), '1\n')   # not a number list: untouched
        self.assertFalse((self.root/'run/y2/health-debug.json').exists())
        restore_health_debug(self.ctx)   # nothing saved: nothing to do

    def test_battery_gate_needs_charger_or_half_battery(self):
        from y2_platform.update import battery_gate
        for battery, allowed in [({'percent': 50, 'charging': False}, True), ({'percent': 49}, False),
                                 ({'percent': 10, 'charging': True}, True), ({'status': 'Full'}, True),
                                 ({}, False), ({'percent': '80'}, False)]:
            self.power(**battery)
            if allowed:
                battery_gate(self.ctx)
            else:
                with self.assertRaisesRegex(ValueError, 'charger_or_half_battery'):
                    battery_gate(self.ctx)
        # A record from an earlier boot is not evidence for this one.
        (self.root/'run/y2/power.json').write_text(json.dumps({'boot_id': 'old', 'battery': {'percent': 90}}))
        with self.assertRaises(ValueError):
            battery_gate(self.ctx)

    def test_install_restarts_unless_the_owner_chose_power_off(self):
        from y2_platform import update
        self.assertEqual(update.restart_mode(self.ctx), 'reboot')
        (self.root/'data/updates/restart.json').write_text(json.dumps({'mode': 'poweroff'}))
        self.assertEqual(update.restart_mode(self.ctx), 'poweroff')
        (self.root/'data/updates/restart.json').write_text(json.dumps({'mode': 'anything'}))
        self.assertEqual(update.restart_mode(self.ctx), 'reboot')

    def test_channel_override_and_redirect_allowlist(self):
        from y2_platform.update import AllowlistedRedirect, channel
        self.assertEqual(channel(self.ctx)['name'], 'stable')
        (self.root/'data/updates/channel.json').write_text(json.dumps({
            'manifest_url': 'https://example.org/x/manifest.json', 'allowed_redirect_hosts': ['evil.example']}))
        value = channel(self.ctx)
        self.assertEqual(value['manifest_url'], 'https://example.org/x/manifest.json')
        self.assertNotIn('evil.example', value['allowed_redirect_hosts'])   # only the shipped list
        handler = AllowlistedRedirect(value['allowed_redirect_hosts'])
        import urllib.request
        request = urllib.request.Request('https://github.com/o/r/releases/latest/download/manifest.json')
        for url in ('http://release-assets.githubusercontent.com/a', 'https://evil.example/a',
                    'https://u:p@github.com/a', 'https://github.com.evil.example/a'):
            with self.assertRaisesRegex(ValueError, 'redirect_refused'):
                handler.redirect_request(request, None, 302, 'Found', {}, url)
        followed = handler.redirect_request(request, None, 302, 'Found', {}, 'https://release-assets.githubusercontent.com/a?sig=1')
        self.assertEqual(followed.full_url, 'https://release-assets.githubusercontent.com/a?sig=1')

    def test_journal_schemas_and_unpublished_channel(self):
        from y2_platform import update
        (self.root/'data/updates/state.json').write_text(json.dumps({'schema': 2, 'state': 'PendingHealth'}))
        self.assertEqual(update.status(self.ctx)['state'], 'PendingHealth')
        (self.root/'data/updates/state.json').write_text(json.dumps({'schema': 3, 'state': 'Idle'}))
        self.assertEqual(update.status(self.ctx)['failure'], 'journal_unreadable')
        (self.root/'data/updates/state.json').unlink()
        (self.root/'etc/y2linux/versions.json').write_text(json.dumps({'release_version': 'v35'}))
        (self.root/'run/y2/network.json').write_text(json.dumps({'state': 'Online'}))
        import urllib.error
        missing = urllib.error.HTTPError('https://github.com/x/manifest.json', 404, 'Not Found', {}, None)
        opener = unittest.mock.MagicMock()
        opener.open.side_effect = missing
        with patch('y2_platform.timekeeping.status', return_value={'tls_ready': True, 'entropy_ready': True}), \
             patch('y2_platform.update.data_volume', return_value={'generation': 1}), \
             patch('urllib.request.build_opener', return_value=opener):
            self.assertEqual(update.stage(self.ctx, url='https://github.com/o/r/releases/latest/download/manifest.json',
                                          check_only=True)['state'], 'UpToDate')
            checked = json.loads((self.root/'data/updates/download.json').read_text())
            self.assertEqual((checked['state'], checked['release_version'], checked['published']), ('Checked', 'v35', False))
            with self.assertRaises(urllib.error.HTTPError):   # a download never treats 404 as success
                update.stage(self.ctx, url='https://github.com/o/r/releases/latest/download/manifest.json')

    def test_daily_check_is_rate_limited_and_never_shows_a_failed_update(self):
        from y2_platform import update
        (self.root/'run/y2/network.json').write_text(json.dumps({'state': 'Online'}))
        (self.root/'data/updates/download.json').write_text(json.dumps({'schema': 1, 'state': 'Checked', 'release_version': 'old'}))
        with patch('y2_platform.timekeeping.status', return_value={'tls_ready': True}), \
             patch.object(update, 'stage', side_effect=ValueError('HTTP_status')) as staged:
            result = update.automatic_check(self.ctx, now=1000)
            self.assertEqual(result['result'], 'failed')
            self.assertEqual(json.loads((self.root/'data/updates/download.json').read_text())['release_version'], 'old')
            # A failed check is tried again (the service paces attempts hourly);
            # it used to wait a whole day.
            staged.side_effect = None
            staged.return_value = {'release_version': 'new'}
            self.assertEqual(update.automatic_check(self.ctx, now=1000 + 3600)['release_version'], 'new')
            self.assertEqual(staged.call_count, 2)
            self.assertIsNone(update.automatic_check(self.ctx, now=1000 + 7200))   # within a day of a real check
            self.assertEqual(update.automatic_check(self.ctx, now=1000 + 3600 + 86401)['release_version'], 'new')
        # A failed record written by ≤ 8.14 kept `time` (ef22a23d): it no longer holds off a day.
        (self.root/'data/updates/last-check.json').write_text(json.dumps(
            {'schema': 1, 'attempt': 5000, 'time': 5000, 'result': 'failed', 'failure': '[Errno 11]'}))
        with patch('y2_platform.timekeeping.status', return_value={'tls_ready': True}), \
             patch.object(update, 'stage', return_value={'release_version': 'newer'}) as staged:
            self.assertEqual(update.automatic_check(self.ctx, now=5000 + 3601)['release_version'], 'newer')
            staged.assert_called_once()
        # Offline: no network call, but the attempt is remembered.
        (self.root/'run/y2/network.json').write_text(json.dumps({'state': 'Associating'}))
        with patch.object(update, 'stage') as staged:
            self.assertIsNone(update.automatic_check(self.ctx, now=1000 + 3*86400))
            staged.assert_not_called()
        self.assertEqual(json.loads((self.root/'data/updates/last-check.json').read_text())['attempt'], 1000 + 3*86400)

    def test_a_check_waits_for_wifi_retries_and_keeps_the_offered_release(self):
        """A Check for Updates right after a wake (Wi-Fi still reconnecting) failed
        at once and replaced 'Update Available' with 'Connect to Wi-Fi' (reports
        61caa81e/297f1032)."""
        from y2_platform import update
        import urllib.error
        url = 'https://github.com/o/r/releases/latest/download/manifest.json'
        (self.root/'etc/y2linux/versions.json').write_text(json.dumps({'release_version': 'v35'}))
        offered = {'schema': 1, 'state': 'Checked', 'release_version': 'v36', 'bytes': 1}
        (self.root/'data/updates/download.json').write_text(json.dumps(offered))
        network = self.root/'run/y2/network.json'
        network.write_text(json.dumps({'state': 'Associating'}))
        opener = unittest.mock.MagicMock()
        opener.open.side_effect = urllib.error.URLError('[Errno -3] Temporary failure in name resolution')
        common = (patch('y2_platform.timekeeping.status', return_value={'tls_ready': True, 'entropy_ready': True}),
                  patch('y2_platform.update.data_volume', return_value={'generation': 1}),
                  patch('urllib.request.build_opener', return_value=opener),
                  patch.object(update, 'NETWORK_WAIT_S', 0.05), patch('time.sleep'))
        with common[0], common[1], common[2], common[3], common[4]:
            # Never online: no request at all, the offered release stays.
            with self.assertRaisesRegex(ValueError, 'network_not_online'):
                update.stage(self.ctx, url=url, check_only=True)
            opener.open.assert_not_called()
            kept = json.loads((self.root/'data/updates/download.json').read_text())
            self.assertEqual((kept['state'], kept['release_version']), ('Checked', 'v36'))
            # Online, but every lookup fails: three tries, then the error; still offered.
            network.write_text(json.dumps({'state': 'Online'}))
            with self.assertRaises(urllib.error.URLError):
                update.stage(self.ctx, url=url, check_only=True)
            self.assertEqual(opener.open.call_count, update.MANIFEST_ATTEMPTS)
            kept = json.loads((self.root/'data/updates/download.json').read_text())
            self.assertEqual((kept['state'], kept['release_version']), ('Checked', 'v36'))
            self.assertIn('name resolution', kept['last_check_failure'])
            # A download that fails is still reported as failed.
            opener.open.reset_mock()
            with self.assertRaises(urllib.error.URLError):
                update.stage(self.ctx, url=url)
            self.assertEqual(json.loads((self.root/'data/updates/download.json').read_text())['state'], 'Failed')

    def test_media_reconcile_runs_in_the_background_and_stops_a_hung_check(self):
        from y2_platform import service
        with patch.object(service, 'media_reconcile_needed', return_value=False), \
             patch('subprocess.Popen') as popen:
            self.assertEqual(service.media_reconcile(self.ctx, None, 100.0), {})
            self.assertEqual(popen.call_count, 0)
        with patch.object(service, 'media_reconcile_needed', return_value=True), \
             patch('subprocess.Popen') as popen, patch.object(service, 'kill_tree') as kill:
            run = service.media_reconcile(self.ctx, None, 100.0)
            self.assertEqual(popen.call_args[0][0], ['/usr/sbin/y2-platform', 'media', 'reconcile'])
            run['process'].poll.return_value = None
            # A long card check is left alone (it used to be killed after 7 s) ...
            self.assertIs(service.media_reconcile(self.ctx, run, 100.0 + 60), run)
            self.assertEqual(popen.call_count, 1)
            # ... a hung one is stopped with everything it started, then backs off.
            stopped = service.media_reconcile(self.ctx, run, 100.0 + service.RECONCILE_LIMIT_S)
            kill.assert_called_once_with(run['process'].pid)
            self.assertIn('retry_at', stopped)
            self.assertEqual(service.media_reconcile(self.ctx, stopped, 100.0 + service.RECONCILE_LIMIT_S + 1), stopped)
            self.assertEqual(popen.call_count, 1)
            service.media_reconcile(self.ctx, stopped, stopped['retry_at'])
            self.assertEqual(popen.call_count, 2)

    def test_media_verify_runs_one_background_check_while_a_card_is_checking(self):
        from y2_platform import service
        with patch.object(service, 'media_verify_needed', return_value=False), \
             patch('subprocess.Popen') as popen:
            self.assertEqual(service.media_verify(self.ctx, None, 100.0), {})
            self.assertEqual(popen.call_count, 0)
        with patch.object(service, 'media_verify_needed', return_value=True), \
             patch('subprocess.Popen') as popen, patch.object(service, 'kill_tree') as kill:
            run = service.media_verify(self.ctx, None, 100.0)
            self.assertEqual(popen.call_args[0][0], ['/usr/sbin/y2-platform', 'media', 'verify'])
            run['process'].poll.return_value = None
            # A long check is never stopped by the loop (media.CHECK_SECONDS bounds it).
            self.assertIs(service.media_verify(self.ctx, run, 100.0 + 3600), run)
            kill.assert_not_called()
            # Ended without a verdict: again only after VERIFY_RETRY_S.
            run['process'].poll.return_value = 0
            ended = service.media_verify(self.ctx, run, 4000.0)
            self.assertEqual(ended, {'retry_at': 4000.0 + service.VERIFY_RETRY_S})
            self.assertEqual(service.media_verify(self.ctx, ended, 4001.0), ended)
            service.media_verify(self.ctx, ended, ended['retry_at'])
            self.assertEqual(popen.call_count, 2)
        claim = self.ctx.path('/run/y2/media-mount.json')
        claim.parent.mkdir(parents=True, exist_ok=True)
        boot = self.ctx.read('/proc/sys/kernel/random/boot_id')
        claim.write_text(json.dumps({'read_only': 'checking', 'boot_id': boot}))
        self.assertTrue(service.media_verify_needed(self.ctx))
        claim.write_text(json.dumps({'read_only': 'protected', 'boot_id': boot}))
        self.assertFalse(service.media_verify_needed(self.ctx))

    def test_a_timed_out_command_also_kills_what_its_child_started(self):
        import sys, time
        from y2_platform.common import command
        marker = self.root/'grandchild.pid'
        child = ('import sys; sys.path.insert(0, %r)\n'
                 'from y2_platform.common import command\n'
                 'import subprocess, os\n'
                 'p = subprocess.Popen(["sleep", "30"], start_new_session=True)\n'
                 'open(%r, "w").write(str(p.pid))\n'
                 'p.wait()\n') % (str(PROJECT/'tools/platform'), str(marker))
        result = command([sys.executable, '-I', '-c', child], timeout=1.5)
        self.assertEqual(result['reason'], 'command_timeout')
        pid = int(marker.read_text())
        for _ in range(50):
            try:
                with open('/proc/%d/stat' % pid) as stream:
                    if stream.read().rpartition(')')[2].split()[0] == 'Z':
                        break
            except FileNotFoundError:
                break
            time.sleep(0.02)
        else:
            os.kill(pid, 9)
            self.fail('grandchild survived the timeout')

    def test_service_spawns_the_check_only_when_due(self):
        from y2_platform.service import daily_update_check
        import time as clock
        with patch('subprocess.Popen') as popen:
            self.assertIsNone(daily_update_check(self.ctx, {'state': 'Scanning'}, None))
            daily_update_check(self.ctx, {'state': 'Online'}, None)
            self.assertEqual(popen.call_count, 1)
            (self.root/'data/updates/last-check.json').write_text(json.dumps({'attempt': clock.time() - 60}))
            self.assertIsNone(daily_update_check(self.ctx, {'state': 'Online'}, None))
            (self.root/'data/updates/last-check.json').write_text(json.dumps(
                {'attempt': clock.time() - 7200, 'time': clock.time() - 7200, 'result': 'checked'}))
            self.assertIsNone(daily_update_check(self.ctx, {'state': 'Online'}, None))
            self.assertEqual(popen.call_count, 1)
            # Report ef22a23d: ≤ 8.14 stamped `time` on a failed check too; that
            # one waits an hour like any failed attempt, not a day.
            (self.root/'data/updates/last-check.json').write_text(json.dumps(
                {'attempt': clock.time() - 7200, 'time': clock.time() - 7200, 'result': 'failed',
                 'failure': '[Errno 11] Resource temporarily unavailable'}))
            daily_update_check(self.ctx, {'state': 'Online'}, None)
            self.assertEqual(popen.call_count, 2)
            running = popen.return_value
            running.poll.return_value = None
            self.assertIs(daily_update_check(self.ctx, {'state': 'Online'}, running), running)


if __name__ == '__main__':
    unittest.main()
