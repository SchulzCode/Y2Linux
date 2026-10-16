"""RELEASE-49: radio firmware from the owner's own copy, never from an image.

Fixture files stand in for the MediaTek firmware (it is not in this
repository); the reviewed hash tables are swapped for theirs. With the owner's
stock package present, the real system.img is read as well.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from y2_platform import radiofw  # noqa: E402
from y2_platform.common import Context  # noqa: E402

STOCK = ROOT.parent / 'Y2Player/y2_v3.2.0_FM-20260813/system.img'


def runnable_mke2fs():
    """A working mke2fs with -d (host e2fsprogs); the locked container has none."""
    for candidate in (shutil.which('mke2fs'), '/build/buildroot/host/sbin/mke2fs'):
        try:
            if candidate and subprocess.run([candidate, '-V'], capture_output=True).returncode == 0:
                return candidate
        except OSError:
            pass
    return None


MKE2FS = runnable_mke2fs()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def custom_library():
    """A stand-in for libcustom_nvram.so with the layout defaults() reads."""
    data = bytearray(os.urandom(51224))
    data[:7] = b'\x7fELF\x01\x01\x01'
    wifi = bytearray(os.urandom(512))
    struct.pack_into('<HH', wifi, 0, 0x104, 0)
    wifi[196:198] = b'\x01\x00'
    data[0x5a8e:0x5c8e] = wifi
    return bytes(data)


def patch_bytes(sequence):
    data = bytearray(os.urandom(64))
    data[18:20] = b'PS'
    data[24] = 0x20 | sequence
    return bytes(data)


def sparse(raw, block=4096):
    """Android sparse encoding: raw chunks, zero runs as don't-care, one fill."""
    blocks = [raw[i:i + block].ljust(block, b'\0') for i in range(0, len(raw), block)]
    chunks, i = [], 0
    while i < len(blocks):
        j = i
        if blocks[i] == bytes(block):
            while j < len(blocks) and blocks[j] == bytes(block):
                j += 1
            chunks.append(struct.pack('<HHII', 0xCAC3, 0, j - i, 12))
        else:
            while j < len(blocks) and blocks[j] != bytes(block):
                j += 1
            chunks.append(struct.pack('<HHII', 0xCAC1, 0, j - i, 12 + (j - i) * block) + b''.join(blocks[i:j]))
        i = j
    # One fill chunk past the data (as stock images end).
    chunks.append(struct.pack('<HHII', 0xCAC2, 0, 2, 16) + b'\xaa\xbb\xcc\xdd')
    header = struct.pack('<IHHHHIIII', 0xED26FF3A, 1, 0, 28, 12, block, len(blocks) + 2, len(chunks), 0)
    return header + b''.join(chunks)


class RadioFirmware(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp(prefix='y2-radiofw-'))
        self.addCleanup(shutil.rmtree, self.temp, True)
        # Fixture firmware and the matching reviewed tables.
        self.files = {
            'WIFI_RAM_CODE_MT6582': os.urandom(160368),
            'WMT_SOC.cfg': b'coex_wmt_ant_mode=1\n'.ljust(80, b'\n'),
            'mt6572_82_patch_e1_0_hdr.bin': patch_bytes(2),
            'mt6572_82_patch_e1_1_hdr.bin': patch_bytes(1),
            'modem_1_2g_n.img': os.urandom(70000),
        }
        self.custom = custom_library()
        derived = radiofw.defaults(self.custom)
        files = {n: (len(d), sha(d)) for n, d in self.files.items()}
        defaults = {n: (len(d), sha(d)) for n, d in derived.items()}
        for name, value in (('FILES', files), ('DEFAULTS', defaults), ('ALL', {**files, **defaults}),
                            ('CUSTOM', ('/lib/libcustom_nvram.so', len(self.custom), sha(self.custom)))):
            p = patch.object(radiofw, name, value)
            p.start()
            self.addCleanup(p.stop)
        self.fm = {'mt6627_fm_v1_patch.bin': os.urandom(1028), 'mt6627_fm_v1_coeff.bin': os.urandom(1092)}
        p = patch.object(radiofw, 'FM', {n: (len(d), sha(d)) for n, d in self.fm.items()})
        p.start()
        self.addCleanup(p.stop)
        self.with_fm = False

    def image(self, extents=True):
        tree = self.temp / 'tree'
        (tree / 'etc/firmware').mkdir(parents=True)
        (tree / 'lib').mkdir()
        for name, data in self.files.items():
            (tree / 'etc/firmware' / name).write_bytes(data)
        if self.with_fm:
            (tree / 'etc/firmware/mt6627').mkdir()
            for name, data in self.fm.items():
                (tree / 'etc/firmware/mt6627' / name).write_bytes(data)
            # Stock also carries empty v2..v5 files; they are never taken.
            (tree / 'etc/firmware/mt6627/mt6627_fm_v2_patch.bin').write_bytes(b'')
        (tree / 'lib/libcustom_nvram.so').write_bytes(self.custom)
        raw = self.temp / 'raw.ext4'
        options = [] if extents else ['-O', '^extent,^64bit', '-t', 'ext3']
        if not MKE2FS:
            self.skipTest('mke2fs not available')
        subprocess.run([MKE2FS, '-q', '-F', '-b', '4096', *options, '-d', str(tree), str(raw), '16M'],
                       check=True, capture_output=True)
        shutil.rmtree(tree)
        return raw

    def test_sparse_and_raw_ext4_images_with_extents_and_block_maps(self):
        for extents in (True, False):
            raw = self.image(extents)
            packed = self.temp / 'system.img'
            packed.write_bytes(sparse(raw.read_bytes()))
            for path in (raw, packed):
                got = radiofw.from_system_image(path)
                self.assertEqual({n: got[n] for n in self.files}, self.files)
                self.assertEqual(set(got), set(radiofw.ALL))
            raw.unlink()

    def test_a_wrong_file_is_refused_by_name(self):
        self.files['WMT_SOC.cfg'] = b'x' * 80
        raw = self.image()
        with patch.object(radiofw, 'FILES', {**radiofw.FILES}), \
             patch.object(radiofw, 'ALL', {**radiofw.ALL}):
            radiofw.FILES['WMT_SOC.cfg'] = (80, 'f' * 64)
            radiofw.ALL['WMT_SOC.cfg'] = (80, 'f' * 64)
            with self.assertRaisesRegex(ValueError, 'firmware_mismatch:WMT_SOC.cfg'):
                radiofw.from_system_image(raw)
        (self.temp / 'other').write_bytes(b'\0' * 8192)
        with self.assertRaisesRegex(ValueError, 'not_ext4'):
            radiofw.from_system_image(self.temp / 'other')

    def ctx(self, calls):
        root = self.temp / 'root'
        for d in ('run/y2', 'data', 'media/sd'):
            (root / d).mkdir(parents=True, exist_ok=True)
        (root / 'run/y2/mtp-roots.json').write_text(json.dumps({'roots': ['/data/firmware/inbox', '/media/sd']}))

        def runner(argv, **kw):
            calls.append(argv)
            return {'ok': True, 'output': '', 'reason': None}
        return Context(root, runner)

    def test_owner_copy_is_taken_once_complete_then_deleted_and_radios_start(self):
        calls = []
        ctx = self.ctx(calls)
        self.assertEqual(radiofw.state(ctx)['state'], 'Missing')
        self.assertEqual(radiofw.scan(ctx)['state'], 'Missing')
        inbox = ctx.path('/data/firmware/inbox')
        self.assertIn('system.img', (inbox / 'README.txt').read_text())
        source = inbox / 'system.img'
        source.write_bytes(sparse(self.image().read_bytes()))
        # Still arriving: the first pass only notes its size and time.
        self.assertEqual(radiofw.scan(ctx)['state'], 'Missing')
        old = time.time() - 60
        os.utime(source, (old, old))
        self.assertEqual(radiofw.scan(ctx)['state'], 'Missing')   # the time changed
        self.assertEqual(radiofw.scan(ctx)['state'], 'Ready')
        self.assertFalse(source.exists())
        self.assertEqual(calls, [['/etc/init.d/S41y2-connectivity', 'stop'], ['/etc/init.d/S41y2-connectivity', 'start']])
        target = ctx.path(radiofw.TARGET)
        self.assertEqual({p.name for p in target.iterdir()}, set(radiofw.ALL))
        self.assertEqual(oct(target.stat().st_mode & 0o777), '0o700')
        self.assertTrue(radiofw.installed(ctx) and radiofw.present(ctx))
        self.assertEqual(radiofw.state(ctx), {'state': 'Ready', 'fm': 'Missing'})

    def test_a_rejected_copy_stays_failed_until_replaced(self):
        ctx = self.ctx([])
        sd = ctx.path('/media/sd/Firmware')
        sd.mkdir()
        (sd / 'system.img').write_bytes(b'\0' * 10000)
        old = time.time() - 60
        os.utime(sd / 'system.img', (old, old))
        radiofw.scan(ctx)
        failed = radiofw.scan(ctx)
        self.assertEqual(failed['state'], 'Failed')
        self.assertEqual(radiofw.scan(ctx)['state'], 'Failed')   # not decoded again
        self.assertTrue((sd / 'system.img').exists())
        self.assertEqual(radiofw.state(ctx)['state'], 'Failed')
        (sd / 'system.img').unlink()
        self.assertEqual(radiofw.scan(ctx)['state'], 'Missing')

    def test_fm_files_come_along_when_they_match_and_never_gate_the_radios(self):
        self.with_fm = True
        got = radiofw.from_system_image(self.image())
        self.assertEqual({n: got[n] for n in self.fm}, self.fm)
        self.assertEqual(set(got), set(radiofw.ALL) | set(radiofw.FM))
        # A different tuner patch is left out; Wi-Fi/Bluetooth are unaffected.
        self.fm['mt6627_fm_v1_coeff.bin'] = os.urandom(1092)
        got = radiofw.from_system_image(self.image())
        self.assertEqual(set(got), set(radiofw.ALL))
        with self.assertRaisesRegex(ValueError, 'firmware_mismatch:mt6627_fm_v1_coeff.bin'):
            radiofw.checked({**got, 'mt6627_fm_v1_coeff.bin': self.fm['mt6627_fm_v1_coeff.bin'],
                             'mt6627_fm_v1_patch.bin': self.fm['mt6627_fm_v1_patch.bin']})
        with self.assertRaisesRegex(ValueError, 'firmware_mismatch:extra'):
            radiofw.checked({**got, 'extra': b'x'})

    def test_fm_files_are_added_later_without_restarting_the_radios(self):
        calls = []
        ctx = self.ctx(calls)
        radiofw.install(ctx, radiofw.from_system_image(self.image()))
        self.assertEqual(radiofw.state(ctx), {'state': 'Ready', 'fm': 'Missing'})
        self.assertEqual(radiofw.scan(ctx), {'state': 'Ready', 'fm': 'Missing'})
        # The owner puts a newer copy in Firmware on the SD card.
        self.with_fm = True
        sd = ctx.path('/media/sd/Firmware')
        sd.mkdir()
        source = sd / 'system.img'
        source.write_bytes(sparse(self.image().read_bytes()))
        self.assertEqual(radiofw.scan(ctx), {'state': 'Ready', 'fm': 'Missing'})   # still arriving
        old = time.time() - 60
        os.utime(source, (old, old))
        self.assertEqual(radiofw.scan(ctx), {'state': 'Ready', 'fm': 'Missing'})   # the time changed
        self.assertEqual(radiofw.scan(ctx), {'state': 'Ready', 'fm': 'Ready'})
        self.assertFalse(source.exists())
        self.assertEqual(calls, [])                     # radios never restarted
        target = ctx.path(radiofw.TARGET)
        self.assertEqual({p.name for p in target.iterdir()}, set(radiofw.ALL) | set(radiofw.FM))
        self.assertTrue(radiofw.installed(ctx) and radiofw.fm_present(ctx))
        self.assertEqual(radiofw.state(ctx), {'state': 'Ready', 'fm': 'Ready'})
        # A root copy (update path) keeps the FM pair.
        self.assertEqual(set(radiofw.from_directory(target)), set(radiofw.ALL) | set(radiofw.FM))

    def test_a_copy_without_fm_files_is_refused_for_fm_only(self):
        ctx = self.ctx([])
        radiofw.install(ctx, radiofw.from_system_image(self.image()))
        sd = ctx.path('/media/sd/Firmware')
        sd.mkdir()
        (sd / 'system.img').write_bytes(sparse(self.image().read_bytes()))
        old = time.time() - 60
        os.utime(sd / 'system.img', (old, old))
        radiofw.scan(ctx)
        failed = radiofw.scan(ctx)
        self.assertEqual(failed, {'state': 'Ready', 'fm': 'Failed', 'reason': 'fm_files_missing'})
        self.assertEqual(radiofw.scan(ctx)['fm'], 'Failed')        # not decoded again
        self.assertTrue(radiofw.present(ctx) and not radiofw.fm_present(ctx))

    @unittest.skipUnless(STOCK.is_file(), 'owner stock package not present')
    def test_the_real_stock_system_image(self):
        patch.stopall()
        got = radiofw.from_system_image(STOCK)
        self.assertEqual({n: (len(d), sha(d)) for n, d in got.items()}, {**radiofw.ALL, **radiofw.FM})


class Watcher(unittest.TestCase):
    """service.radio_firmware notices firmware removed while running."""
    def test_removed_firmware_is_noticed_at_the_next_maintenance_pass(self):
        from y2_platform import service
        spawned = []

        class Child:
            def poll(self):
                return 0
        ctx = Context(Path(tempfile.mkdtemp(prefix='y2-watch-')), lambda argv, **kw: {'ok': True})
        self.addCleanup(shutil.rmtree, ctx.root, True)
        with patch('y2_platform.radiofw.present', return_value=False), \
             patch('subprocess.Popen', side_effect=lambda *a, **k: spawned.append(a[0]) or Child()):
            child, ready = service.radio_firmware(ctx, None, True, False)
            self.assertTrue(ready)                       # between passes: no work
            self.assertEqual(spawned, [])
            child, ready = service.radio_firmware(ctx, None, True, True)
            self.assertFalse(ready)                      # maintenance: re-checked
            self.assertEqual(spawned, [['/usr/sbin/y2-platform', 'firmware', 'scan']])
        with patch('y2_platform.radiofw.present', return_value=True):
            self.assertEqual(service.radio_firmware(ctx, None, False, False), (None, True))

    def test_fm_top_up_scans_only_at_maintenance_and_only_with_a_copy(self):
        from y2_platform import service
        spawned = []

        class Child:
            def poll(self):
                return None
        ctx = Context(Path(tempfile.mkdtemp(prefix='y2-watch-')), lambda argv, **kw: {'ok': True})
        self.addCleanup(shutil.rmtree, ctx.root, True)
        copy = ctx.root / 'media/sd/Firmware/system.img'
        with patch('y2_platform.radiofw.present', return_value=True), \
             patch('y2_platform.radiofw.fm_present', return_value=False), \
             patch('y2_platform.radiofw.candidates', return_value=[copy]), \
             patch('subprocess.Popen', side_effect=lambda *a, **k: spawned.append(a[0]) or Child()):
            self.assertEqual(service.radio_firmware(ctx, None, True, True), (None, True))   # no copy
            copy.parent.mkdir(parents=True)
            copy.write_bytes(b'x')
            self.assertEqual(service.radio_firmware(ctx, None, True, False), (None, True))  # between passes
            self.assertEqual(spawned, [])
            child, ready = service.radio_firmware(ctx, None, True, True)
            self.assertTrue(ready and child)
            self.assertEqual(len(spawned), 1)
            self.assertEqual(service.radio_firmware(ctx, child, True, True), (child, True))  # still running
            self.assertEqual(len(spawned), 1)


class Tables(unittest.TestCase):
    def test_one_reviewed_list_everywhere(self):
        from tools.connectivity.provision import CUSTOM_SHA, CUSTOM_SIZE, DEFAULTS, INVENTORY
        inventory = {x['filename']: (x['bytes'], x['sha256']) for x in json.loads(INVENTORY.read_text())['files']}
        self.assertEqual(radiofw.FILES, inventory)
        self.assertEqual(radiofw.DEFAULTS, DEFAULTS)
        self.assertEqual(radiofw.CUSTOM[1:], (CUSTOM_SIZE, CUSTOM_SHA))
        script = (ROOT / 'initramfs/production/recovery.sh').read_text()
        block = script.split("RADIO_FIRMWARE='", 1)[1].split("'", 1)[0]
        listed = {line.split('  ')[1]: line.split('  ')[0] for line in block.splitlines()}
        self.assertEqual(listed, {n: v[1] for n, v in radiofw.ALL.items()})


class RecoveryKeepsFirmware(unittest.TestCase):
    """recovery.sh copies the reviewed files from the old root to Y2DATA."""
    def run_keep(self, files, existing=None):
        temp = Path(tempfile.mkdtemp(prefix='y2-keep-'))
        self.addCleanup(shutil.rmtree, temp, True)
        old = temp / 'oldroot/lib/firmware/mediatek/mt6582'
        old.mkdir(parents=True)
        for name, data in files.items():
            (old / name).write_bytes(data)
        (temp / 'newdata').mkdir()
        if existing:
            (temp / 'newdata/firmware/mt6582').mkdir(parents=True)
        script = (ROOT / 'initramfs/production/recovery.sh').read_text()
        start = script.index('RADIO_FIRMWARE=')
        end = script.index('charger_online() {')
        body = script[start:end]
        block = body.split("RADIO_FIRMWARE='", 1)[1].split("'", 1)[0]
        listed = '\n'.join('%s  %s' % (sha(d), n) for n, d in files.items()
                           if n in [l.split('  ')[1] for l in block.splitlines()])
        body = body.replace(block, listed if len(files) == 7 else block)
        body = (body.replace('/bin/busybox sha256sum -c -s -', 'sha256sum -c - >/dev/null 2>&1')
                    .replace('/bin/busybox rm', 'rm')
                    .replace('/newdata', str(temp / 'newdata')).replace('/oldroot', str(temp / 'oldroot')))
        shell = ('set -u\ny2_find_partition() { echo /dev/fixture; }\nmount() { :; }\numount() { :; }\n'
                 'recovery_log() { echo "LOG $*"; }\n' + body + 'recovery_keep_radio_firmware\n')
        result = subprocess.run(['sh', '-c', shell], capture_output=True, text=True, timeout=30)
        return temp, result

    def test_reviewed_files_move_to_y2data(self):
        names = ['WIFI_RAM_CODE_MT6582', 'WMT_SOC.cfg', 'mt6572_82_patch_e1_0_hdr.bin',
                 'mt6572_82_patch_e1_1_hdr.bin', 'modem_1_2g_n.img', 'WIFI.defaults', 'BT.defaults']
        files = {n: os.urandom(100) for n in names}
        temp, result = self.run_keep(files)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('LOG radio firmware kept on Y2DATA', result.stdout)
        kept = temp / 'newdata/firmware/mt6582'
        self.assertEqual({p.name: p.read_bytes() for p in kept.iterdir()}, files)
        self.assertFalse((temp / 'newdata/firmware/.mt6582.keep').exists())

    def test_files_not_matching_the_reviewed_hashes_are_left_alone(self):
        temp, result = self.run_keep({'WIFI_RAM_CODE_MT6582': b'other'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((temp / 'newdata/firmware/mt6582').exists())


if __name__ == '__main__':
    unittest.main()
