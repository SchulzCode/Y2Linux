"""Actual Ed25519/gzip/journal/offline writer faults on private regular files.

The production binary has neither a fixture root nor fault controls. This suite
never opens a block device and is not power-loss durability evidence.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools/platform"))

from tools.update.package import build, canonical, keygen, sign

PROJECT = Path(__file__).resolve().parents[1]


class UpdateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tools = tempfile.TemporaryDirectory(prefix='y2-update-tools-')
        cls.executable = Path(cls.tools.name) / 'core'
        # Optional actual target compiler/QEMU command, selected only by host harness.
        cc = os.environ.get('Y2_UPDATE_TEST_CC', 'cc')
        subprocess.run([cc, '-O2', '-Wall', '-Wextra', '-Werror', '-DY2_UPDATE_TEST',
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
        self.write('etc/y2linux/update-compat.json', dict(schema=1, product='Y2',
                   hardware_revision='innioasis-y2-mt6582', kernel='6.18-test',
                   rootfs_contract='y2-platform-v1', sequence=0))
        self.write('etc/y2linux/versions.json', self.versions)
        # A raw-file image fixture with the real ext4 identity and extent fields;
        # tests the stream boundary, not e2fsck or a simulated electrical device.
        raw = bytearray(8*1024**2)
        struct.pack_into('<I', raw, 1024+4, len(raw)//4096)
        struct.pack_into('<I', raw, 1024+24, 2)
        struct.pack_into('<H', raw, 1024+56, 0xef53)
        raw[1024+104:1024+120] = bytes.fromhex('79324c696e7548018000000000000101')
        raw[1024+120:1024+127] = b'Y2ROOT\0'
        self.old = bytes(raw)
        (self.root / 'root.img').write_bytes(self.old)
        raw[1024*1024:1024*1024+16] = b'new-root-content'
        self.new = bytes(raw)
        (self.root / 'new.img').write_bytes(self.new)
        self.manifest = build(self.root / 'new.img', self.versions, self.base / 'pending',
                              self.private, 'test', 1, rollback_allowed=True)

    def write(self, name, value):
        (self.root / name).write_bytes(canonical(value))

    def boot(self, name):
        (self.root / 'proc/sys/kernel/random/boot_id').write_text(name)

    def run_core(self, *args, expected=0, fault=None):
        env = os.environ.copy()
        env['Y2_TEST_ROOT'] = str(self.root)
        if fault:
            env['Y2_TEST_FAULT'] = fault
        result = subprocess.run([*self.prefix, str(self.executable), *args], env=env,
                                capture_output=True, timeout=15)
        self.assertEqual(result.returncode, expected, result.stderr.decode()+result.stdout.decode())
        return json.loads(result.stdout) if result.stdout else None

    def resign(self, manifest):
        raw = canonical(manifest)
        (self.base / 'pending/manifest.json').write_bytes(raw)
        (self.base / 'pending/manifest.sig').write_bytes(sign(raw, self.private))

    def test_authentication_compatibility_and_corrupt_payload_fail_before_root_write(self):
        for field, value in [('product', 'wrong-device'), ('data_schema', 2), ('kernel', 'new-kernel'),
                             ('rescue_api', 9)]:
            manifest = copy.deepcopy(self.manifest)
            manifest[field] = value
            self.resign(manifest)
            self.run_core('queue', expected=1)
        self.resign(self.manifest)
        signature = self.base / 'pending/manifest.sig'
        signature.write_bytes(b'x'*64)
        self.run_core('queue', expected=1)
        self.resign(self.manifest)
        payload = self.base / 'pending/rootfs.ext4.gz'
        good = payload.read_bytes()
        payload.write_bytes(good[:-5])
        self.run_core('queue', expected=1)
        payload.write_bytes(good[:-10]+b'corruption')
        self.run_core('queue', expected=1)
        self.assertEqual((self.root / 'root.img').read_bytes(), self.old)
        self.assertFalse((self.base / 'state.json').exists())

    def test_install_readback_and_exact_boot_health_acknowledgement(self):
        self.run_core('queue')
        self.assertEqual(self.run_core('rescue')['state'], 'PendingHealth')
        self.assertEqual((self.root / 'root.img').read_bytes(), self.new)
        self.run_core('ack', expected=1)
        self.write('run/y2/application-ready.json', {'boot_id': 'wrong'})
        self.run_core('ack', expected=1)
        self.write('run/y2/application-ready.json', {'boot_id': 'a'})
        self.assertEqual(self.run_core('ack')['state'], 'Acknowledged')
        self.boot('b')
        self.assertEqual(self.run_core('rescue')['state'], 'Acknowledged')
        # Release key rejects replay even if a signed manifest requests downgrade.
        trust = json.loads((self.root / 'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0]['purpose'] = 'release'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue', expected=1)

    def test_interrupted_root_write_and_missing_health_restore_verified_previous_root(self):
        self.run_core('queue')
        self.run_core('rescue', expected=91, fault='write_interrupt')
        self.assertEqual(json.loads((self.base/'state.json').read_text())['state'], 'Installing')
        self.boot('b')
        self.assertEqual(self.run_core('rescue')['state'], 'RolledBack')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.run_core('queue')
        self.run_core('rescue')
        self.boot('c')
        self.assertEqual(self.run_core('rescue')['state'], 'RolledBack')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)

    def test_readback_failure_restores_and_bad_backup_requires_rescue_without_loop(self):
        self.run_core('queue')
        self.assertEqual(self.run_core('rescue', fault='readback')['state'], 'RolledBack')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.run_core('queue')
        self.run_core('rescue', expected=91, fault='write_interrupt')
        (self.base/'previous.ext4.gz').write_bytes(b'bad')
        self.boot('b')
        self.run_core('rescue', expected=1)
        self.assertEqual(json.loads((self.base/'state.json').read_text())['state'], 'RescueRequired')
        before = (self.root/'root.img').read_bytes()
        self.run_core('rescue', expected=1)
        self.assertEqual((self.root/'root.img').read_bytes(), before)

    def test_backup_space_failure_preserves_root_and_stage_then_cancel(self):
        self.run_core('queue')
        result = self.run_core('rescue', fault='reserve')
        self.assertEqual(result['state'], 'Failed')
        self.assertIn('reserve', result['failure'])
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        self.run_core('queue')
        self.assertEqual(self.run_core('cancel')['state'], 'Failed')
        self.run_core('rescue')
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)

    def test_download_stage_authenticates_before_payload_and_never_queues_partial(self):
        from y2_platform.common import Context
        from y2_platform.update import stage, base_url, copy_stream
        source = self.root/'package'
        (self.base/'pending').rename(source)
        volume = {'path':'/data','state':'Ready','generation':'boot:1:179:7'}
        def runner(argv, **kw):
            result = subprocess.run([*self.prefix,str(self.executable),*argv[1:]],
                env=os.environ|{'Y2_TEST_ROOT':str(self.root)},capture_output=True,timeout=15)
            return {'ok':result.returncode==0,'output':result.stdout.decode(),'reason':None if result.returncode==0 else 'command_failed'}
        ctx=Context(self.root,runner)
        with patch('y2_platform.transfer.storage',return_value={'volumes':[volume]}):
            checked=stage(ctx,source,check_only=True)
            self.assertEqual(checked['sequence'],1)
            self.assertFalse((self.base/'state.json').exists())
            payload=source/'rootfs.ext4.gz'; original=payload.read_bytes()
            payload.write_bytes(original[:-10])
            with self.assertRaisesRegex(ValueError,'incomplete_download'):
                stage(ctx,source)
            self.assertFalse((self.base/'state.json').exists())
            payload.write_bytes(original)
            self.assertEqual(stage(ctx,source)['state'],'Queued')
        self.assertEqual((self.root/'root.img').read_bytes(),self.old)
        for url in ('http://example.com/manifest.json','https://u:p@example.com/manifest.json',
                    'https://example.com/manifest.json?redirect=other','https://example.com/other'):
            with self.assertRaises(ValueError): base_url(url)
        self.assertEqual(base_url('https://example.com/release/manifest.json'),'https://example.com/release/')

    def test_missing_power_key_revocation_journal_corruption_and_symlinks(self):
        trust = json.loads((self.root / 'etc/y2linux/update-trust.json').read_text())
        trust['keys'][0]['status'] = 'revoked'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue', expected=1)
        trust['keys'][0]['status'] = 'active'
        self.write('etc/y2linux/update-trust.json', trust)
        self.run_core('queue')
        (self.root/'external-power').write_text('0')
        self.run_core('rescue', expected=1)
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)
        (self.base/'state.json').write_text('{broken')
        self.run_core('status', expected=1)
        (self.base/'state.json').unlink()
        (self.base/'state.json').symlink_to(self.root/'secret/key')
        self.run_core('status', expected=1)
        self.assertEqual((self.root/'root.img').read_bytes(), self.old)


if __name__ == '__main__':
    unittest.main()
