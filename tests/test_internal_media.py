"""Y2Media (eMMC p8): exact-identity mount, owner-confirmed provisioning, verified migration."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform import internal_media as im
from y2_platform.common import Context
from y2_platform.observe import storage

STOCK_SERIAL = '3F93-07E6'
MOUNT_LINE = '9 1 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw'
OLD_LINE = '12 1 179:8 / /run/y2/old-storage ro - vfat /dev/mmcblk0p8 ro'


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.put('/proc/sys/kernel/random/boot_id', 'boot-one')
        self.put('/proc/self/mountinfo', '')
        self.add_partition('mmcblk0p8', '3742720', '11460608', '179:8')
        self.add_partition('mmcblk0p7', '2104320', '1638400', '179:7')
        self.fs = {'TYPE': 'vfat', 'UUID': STOCK_SERIAL}
        self.calls = []
        self.fsck_ok = True
        self.fat_check = {'ok': True, 'reason': None, 'output': ''}
        self.mkfs_ok = True

        def runner(argv, **kwargs):
            self.calls.append(argv)
            ok = {'ok': True, 'reason': None, 'output': ''}
            if argv[0] == 'blkid':
                text = '\n'.join('%s=%s' % kv for kv in self.fs.items())
                return {'ok': True, 'reason': None, 'output': text}
            if argv[0] == 'e2fsck':
                return ok if self.fsck_ok else {'ok': False, 'reason': 'command_failed', 'output': None}
            if argv[0] == 'mkfs.ext4':
                if not self.mkfs_ok:
                    return {'ok': False, 'reason': 'command_failed', 'output': None}
                self.fs = {'TYPE': 'ext4', 'LABEL': argv[argv.index('-L') + 1], 'UUID': argv[argv.index('-U') + 1]}
                return ok
            if argv[0] == 'fsck.fat':
                return dict(self.fat_check)
            if argv[0] == 'mount':
                self.mount_line(OLD_LINE if argv[2] == 'vfat' else MOUNT_LINE, True)
                return ok
            if argv[0] == 'umount':
                if argv[1] == im.OLD:
                    self.mount_line(OLD_LINE, False)
                else:
                    self.put('/proc/self/mountinfo', '')
                return ok
            return ok
        self.ctx.runner = runner

    def put(self, path, value):
        p = self.ctx.path(path)
        p.parent.mkdir(exist_ok=True, parents=True)
        if isinstance(value, bytes):
            p.write_bytes(value)
        else:
            p.write_text(value)
        return p

    def add_partition(self, name, start, size, dev):
        disk = self.ctx.path('/sys/devices/platform/11230000.mmc/block/mmcblk0')
        part = disk / name
        part.mkdir(parents=True, exist_ok=True)
        for key, value in (('partition', name[-1]), ('start', start), ('size', size), ('dev', dev)):
            (part / key).write_text(value)
        link = self.ctx.path('/sys/class/block/' + name)
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(part)
        dev_link = self.ctx.path('/sys/dev/block/' + dev)
        dev_link.parent.mkdir(parents=True, exist_ok=True)
        dev_link.symlink_to(part)

    def mount_line(self, line, present):
        lines = [l for l in self.ctx.read('/proc/self/mountinfo', limit=65536).splitlines() if l and l != line] \
            if self.ctx.path('/proc/self/mountinfo').exists() else []
        if present:
            lines.append(line)
        self.put('/proc/self/mountinfo', '\n'.join(lines))

    def provisioned(self):
        self.fs = {'TYPE': 'ext4', 'LABEL': im.LABEL, 'UUID': im.UUID}

    def names(self):
        return [c[0] for c in self.calls]

    def music(self, files):
        for name, data in files.items():
            self.put('/data/music/' + name, data)
        self.ctx.path('/media/internal').mkdir(parents=True, exist_ok=True)


class InternalMedia(Fixture):
    # ---- identity -------------------------------------------------------------------------

    def test_partition_needs_the_exact_geometry_on_the_internal_controller_only(self):
        self.assertEqual(im.partition(self.ctx).name, 'mmcblk0p8')
        self.ctx.path('/sys/devices/platform/11230000.mmc/block/mmcblk0/mmcblk0p8/size').write_text('11460607')
        self.assertIsNone(im.partition(self.ctx))
        self.ctx.path('/sys/devices/platform/11230000.mmc/block/mmcblk0/mmcblk0p8/size').write_text(im.SIZE)
        # the same numbers on the removable-card controller are not Y2Media
        sd = self.ctx.path('/sys/devices/platform/11240000.mmc/block/mmcblk1/mmcblk1p9')
        sd.mkdir(parents=True)
        for key, value in (('partition', '9'), ('start', im.START), ('size', im.SIZE)):
            (sd / key).write_text(value)
        self.ctx.path('/sys/class/block/mmcblk1p9').symlink_to(sd)
        self.assertEqual(im.partition(self.ctx).name, 'mmcblk0p8')

    def test_mount_checks_then_mounts_only_the_exact_identity(self):
        self.provisioned()
        result = im.mount(self.ctx)
        self.assertEqual((result['state'], result['reason']), ('Ready', None))
        order = self.names()
        self.assertLess(order.index('e2fsck'), order.index('mount'))
        mount = next(c for c in self.calls if c[0] == 'mount')
        self.assertEqual(mount[:4], ['mount', '-t', 'ext4', '-o'])
        self.assertEqual(mount[4], 'rw,nosuid,nodev,noexec,noatime,noinit_itable')
        self.assertEqual(mount[5:], ['/dev/mmcblk0p8', '/media/internal'])
        self.assertEqual(next(c for c in self.calls if c[0] == 'e2fsck')[:2], ['e2fsck', '-p'])
        self.assertTrue(im.ready(self.ctx))
        self.assertEqual(im.music_root(self.ctx), '/media/internal/music')
        # idempotent: an existing exact mount is recorded, not remounted
        self.calls.clear()
        self.assertEqual(im.mount(self.ctx)['state'], 'Ready')
        self.assertNotIn('mount', self.names())

    def test_nothing_else_is_ever_mounted(self):
        for fs in ({'TYPE': 'vfat', 'UUID': STOCK_SERIAL},
                   {'TYPE': 'ext4', 'LABEL': 'Y2DATA', 'UUID': im.UUID},
                   {'TYPE': 'ext4', 'LABEL': im.LABEL, 'UUID': '79324c69-6e75-4801-8000-000000000102'},
                   {'TYPE': 'exfat', 'LABEL': im.LABEL, 'UUID': im.UUID}, {}):
            self.fs, self.calls = fs, []
            result = im.mount(self.ctx)
            self.assertEqual((result['state'], result['reason']), ('Unavailable', 'filesystem_identity_not_y2media'), fs)
            self.assertNotIn('mount', self.names())
            self.assertNotIn('e2fsck', self.names())
        self.assertFalse(im.ready(self.ctx))
        self.assertEqual(im.music_root(self.ctx), '/data/music')

    def test_failed_check_or_foreign_mount_is_not_ready(self):
        self.provisioned()
        self.fsck_ok = False
        self.assertEqual(im.mount(self.ctx)['reason'], 'filesystem_check_failed')
        self.assertNotIn('mount', self.names())
        self.fsck_ok = True
        self.put('/proc/self/mountinfo', '9 1 179:7 / /media/internal rw - ext4 /dev/mmcblk0p7 rw')
        self.assertEqual((im.mount(self.ctx)['state'], im.mount(self.ctx)['reason']), ('Failed', 'foreign_mount_at_path'))
        self.assertFalse(im.ready(self.ctx))

    def test_ready_follows_the_live_mount_and_this_boot(self):
        self.provisioned()
        im.mount(self.ctx)
        self.assertTrue(im.ready(self.ctx))
        self.put('/proc/self/mountinfo', '10 1 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw')   # remounted: new id
        self.assertFalse(im.ready(self.ctx))
        self.put('/proc/self/mountinfo', MOUNT_LINE)
        self.assertTrue(im.ready(self.ctx))
        self.put('/proc/sys/kernel/random/boot_id', 'boot-two')
        self.assertFalse(im.ready(self.ctx))
        self.put('/proc/sys/kernel/random/boot_id', 'boot-one')
        self.put('/proc/self/mountinfo', '')
        self.assertFalse(im.ready(self.ctx))

    def test_storage_reports_the_fourth_volume_and_rejects_the_wrong_uuid(self):
        self.provisioned()
        self.put('/proc/self/mountinfo', MOUNT_LINE)
        self.ctx.path('/media/internal').mkdir(parents=True)
        volumes = storage(self.ctx)['volumes']
        self.assertEqual(len(volumes), 4)
        self.assertEqual(volumes[3]['path'], '/media/internal')
        self.fs = {'TYPE': 'ext4', 'LABEL': im.LABEL, 'UUID': 'something-else'}
        self.ctx.runner = lambda argv, **k: {'ok': True, 'reason': None, 'output': 'something-else'}
        self.assertEqual(storage(self.ctx)['volumes'][3]['reason'], 'filesystem_identity_invalid')

    # ---- provisioning ---------------------------------------------------------------------

    def test_provision_needs_the_stock_serial_and_replaces_nothing_else(self):
        for fs, confirm, reason in (({'TYPE': 'vfat', 'UUID': STOCK_SERIAL}, None, 'confirmation_does_not_match_stock_filesystem'),
                                    ({'TYPE': 'vfat', 'UUID': STOCK_SERIAL}, 'AAAA-BBBB', 'confirmation_does_not_match_stock_filesystem'),
                                    ({'TYPE': 'ext4', 'LABEL': 'Y2DATA', 'UUID': 'x'}, 'x', 'confirmation_does_not_match_stock_filesystem'),
                                    ({'TYPE': 'ntfs', 'UUID': 'x'}, 'x', 'confirmation_does_not_match_stock_filesystem')):
            self.fs, self.calls = fs, []
            result = im.provision(self.ctx, confirm)
            self.assertEqual((result['state'], result['reason']), ('Failed', reason), (fs, confirm))
            self.assertNotIn('mkfs.ext4', self.names())

    def test_a_mounted_foreign_filesystem_is_never_replaced(self):
        self.put('/proc/self/mountinfo', '9 1 179:8 / /mnt/other rw - vfat /dev/mmcblk0p8 rw')
        result = im.provision(self.ctx, STOCK_SERIAL)
        self.assertEqual((result['state'], result['reason']), ('Failed', 'partition_is_mounted'))
        self.assertNotIn('mkfs.ext4', self.names())

    def test_provision_formats_the_confirmed_stock_fat_mounts_and_creates_the_music_folder(self):
        self.ctx.path('/media/internal').mkdir(parents=True)
        result = im.provision(self.ctx, STOCK_SERIAL, migrate_music=False)
        self.assertEqual((result['state'], result['reason']), ('Ready', None))
        mkfs = next(c for c in self.calls if c[0] == 'mkfs.ext4')
        self.assertEqual(mkfs[-1], '/dev/mmcblk0p8')
        self.assertEqual(mkfs[mkfs.index('-L') + 1], 'Y2MEDIA')
        self.assertEqual(mkfs[mkfs.index('-U') + 1], im.UUID)
        self.assertEqual(mkfs[mkfs.index('-m') + 1], '0')
        self.assertLess(self.names().index('mkfs.ext4'), self.names().index('mount'))
        self.assertTrue(self.ctx.path('/media/internal/music').is_dir())
        self.assertEqual(sum(1 for c in self.calls if c[0] == 'mkfs.ext4'), 1)

    def test_a_failed_mkfs_leaves_the_state_failed_and_mounts_nothing(self):
        self.mkfs_ok = False
        result = im.provision(self.ctx, STOCK_SERIAL)
        self.assertEqual(result['state'], 'Failed')
        self.assertTrue(result['reason'].startswith('mkfs_failed'))
        self.assertNotIn('mount', self.names())

    def test_provision_moves_the_library_with_hashes_and_removes_only_verified_originals(self):
        self.music({'a.flac': b'A' * 3000, 'album/b.mp3': b'B' * 70000, 'album/disc2/c.ogg': b'C' * 11})
        os.utime(self.ctx.path('/data/music/a.flac'), ns=(1_000_000_000, 1_700_000_000_000_000_000))
        result = im.provision(self.ctx, STOCK_SERIAL)
        self.assertEqual(result['state'], 'Ready')
        self.assertEqual(result['migration'], {'files': 3, 'bytes': 73011, 'copied': 3, 'removed': 3})
        for name, data in (('a.flac', b'A' * 3000), ('album/b.mp3', b'B' * 70000), ('album/disc2/c.ogg', b'C' * 11)):
            self.assertEqual(self.ctx.path('/media/internal/music/' + name).read_bytes(), data)
            self.assertFalse(self.ctx.path('/data/music/' + name).exists())
        self.assertEqual(self.ctx.path('/media/internal/music/a.flac').stat().st_mtime_ns, 1_700_000_000_000_000_000)
        self.assertTrue(self.ctx.path('/data/music').is_dir())                    # the folder itself stays
        self.assertFalse(list(self.ctx.path('/media/internal/music').rglob('*.part')))

    def test_any_migration_problem_keeps_every_original(self):
        self.music({'a.flac': b'A' * 100, 'b.flac': b'B' * 100})
        with patch.object(im, 'digest', side_effect=lambda path: 'x' if path.endswith('.part') else 'y'):
            result = im.provision(self.ctx, STOCK_SERIAL)
        self.assertEqual(result['state'], 'Degraded')
        self.assertIn('migration_hash_mismatch', result['reason'])
        for name in ('a.flac', 'b.flac'):
            self.assertTrue(self.ctx.path('/data/music/' + name).exists())
        self.assertFalse(list(self.ctx.path('/media/internal/music').rglob('*.part')))

    def test_symlinks_special_files_and_oversized_libraries_are_refused_untouched(self):
        self.music({'a.flac': b'A' * 100})
        outside = self.ctx.path('/data/elsewhere')
        outside.mkdir()
        self.ctx.path('/data/music/link').symlink_to(outside, target_is_directory=True)
        result = im.provision(self.ctx, STOCK_SERIAL)
        self.assertEqual(result['state'], 'Degraded')
        self.assertIn('migration_refuses_symlink', result['reason'])
        self.assertTrue(self.ctx.path('/data/music/a.flac').exists())
        self.ctx.path('/data/music/link').unlink()
        self.provisioned()
        with patch.object(im, 'RESERVE', 1 << 62):
            result = im.provision(self.ctx, None)
        self.assertIn('migration_does_not_fit', result['reason'])
        self.assertTrue(self.ctx.path('/data/music/a.flac').exists())

    def test_rerun_after_success_is_idempotent_and_resumes_a_partial_copy(self):
        self.music({'a.flac': b'A' * 100})
        self.assertEqual(im.provision(self.ctx, STOCK_SERIAL)['state'], 'Ready')
        self.calls.clear()
        again = im.provision(self.ctx, None)                       # already Y2Media: no confirmation, no mkfs
        self.assertEqual(again['state'], 'Ready')
        self.assertNotIn('mkfs.ext4', self.names())
        # a previously copied but not yet removed file is recognised by hash, not duplicated
        self.put('/data/music/b.flac', 'B' * 50)
        self.put('/media/internal/music/b.flac', 'B' * 50)
        result = im.provision(self.ctx, None)
        self.assertEqual(result['migration']['copied'], 0)
        self.assertEqual(result['migration']['removed'], 1)
        # but a different file at the same name is never overwritten
        self.put('/data/music/c.flac', 'one')
        self.put('/media/internal/music/c.flac', 'two!')
        result = im.provision(self.ctx, None)
        self.assertIn('migration_target_differs', result['reason'])
        self.assertTrue(self.ctx.path('/data/music/c.flac').exists())

    def test_unmount_and_status(self):
        self.provisioned()
        im.mount(self.ctx)
        self.assertTrue(im.status(self.ctx)['ready'])
        self.assertEqual(im.unmount(self.ctx)['reason'], 'unmounted')
        status = im.status(self.ctx)
        self.assertFalse(status['ready'])
        self.assertTrue(status['identity_valid'])
        self.assertEqual(im.unmount(self.ctx)['reason'], 'not_mounted')


if __name__ == '__main__':
    unittest.main()


class Setup(Fixture):
    """STORAGE-SETUP: the stock FAT becomes Y2Media by itself where that is safe."""

    def setUp(self):
        super().setUp()
        self.put('/sys/class/power_supply/BAT0/status', 'Discharging')
        self.put('/sys/class/power_supply/BAT0/voltage_now', '3995068')
        self.ctx.path('/media/internal').mkdir(parents=True)
        self.ctx.path('/data').mkdir(exist_ok=True)
        self.ctx.path('/run/reborn-splash').mkdir(parents=True)
        self.phases = []
        real = im.splash
        def splash(ctx, step):
            real(ctx, step)
            self.phases.append(step)
        patcher = patch.object(im, 'splash', splash)
        patcher.start()
        self.addCleanup(patcher.stop)

    def old(self, files):
        """Files on the stock storage (seen through its read-only mount)."""
        for name, data in files.items():
            self.put(im.OLD + '/' + name, data)

    def test_stock_leftovers_only_set_up_at_once_with_the_pinned_layout(self):
        self.old({'Android/data/com.android.providers.media/albumthumbs/1': b'x' * 10,
                  'DCIM/.thumbnails/t.jpg': b't', '.android_secure/x': b'', 'Music/.nomedia': b''})
        self.ctx.path(im.OLD + '/Ringtones').mkdir()
        result = im.setup(self.ctx, automatic=True)
        self.assertEqual((result['state'], result['kept']), ('Done', 0))
        mkfs = next(c for c in self.calls if c[0] == 'mkfs.ext4')
        self.assertEqual(mkfs, im.MKFS + ['/dev/mmcblk0p8'])
        self.assertIn('-O', mkfs)
        self.assertEqual(mkfs[mkfs.index('-O') + 1].split(','),
                         ['none', 'has_journal', 'ext_attr', 'resize_inode', 'dir_index', 'orphan_file',
                          'filetype', 'extent', '64bit', 'flex_bg', 'metadata_csum_seed', 'sparse_super',
                          'large_file', 'huge_file', 'dir_nlink', 'extra_isize', 'metadata_csum'])
        self.assertTrue(im.ready(self.ctx))
        self.assertFalse(self.ctx.path(im.SETUP).exists())
        self.assertFalse(self.ctx.path('/media/internal/music/' + im.KEPT).exists())
        self.assertEqual(self.phases[0], 0)
        self.assertEqual(self.phases[-1], 100)
        self.assertEqual(self.ctx.read('/run/reborn-splash/phase'), 'media_setup_100')
        # Next start: nothing to do.
        self.calls.clear()
        self.assertEqual(im.setup(self.ctx, automatic=True)['state'], 'NotNeeded')
        self.assertNotIn('mkfs.ext4', self.names())

    def test_a_few_files_are_kept_in_old_player_files_with_hashes_and_times(self):
        self.old({'Themes/neon/skin.json': b'{"a":1}', 'Music/Song.mp3': b'M' * 5000,
                  'Android/data/com.android.providers.media/albumthumbs/9': b'cache'})
        os.utime(self.ctx.path(im.OLD + '/Music/Song.mp3'), ns=(1, 1_600_000_000_000_000_000))
        self.music({'legacy.flac': b'L' * 10})
        result = im.setup(self.ctx, automatic=True)
        self.assertEqual((result['state'], result['kept']), ('Done', 2))
        kept = self.ctx.path('/media/internal/music/' + im.KEPT)
        self.assertEqual((kept / 'Themes/neon/skin.json').read_bytes(), b'{"a":1}')
        self.assertEqual((kept / 'Music/Song.mp3').read_bytes(), b'M' * 5000)
        self.assertEqual((kept / 'Music/Song.mp3').stat().st_mtime_ns, 1_600_000_000_000_000_000)
        self.assertFalse((kept / 'Android').exists())
        self.assertEqual(self.ctx.path('/media/internal/music/legacy.flac').read_bytes(), b'L' * 10)
        self.assertFalse(self.ctx.path(im.KEPT_STAGING).exists())
        self.assertFalse(self.ctx.path(im.SETUP).exists())
        order = self.names()
        self.assertLess(order.index('fsck.fat'), order.index('mkfs.ext4'))
        self.assertEqual(im.setup_status(self.ctx), {'state': 'Done', 'reason': None, 'kept': 2})

    def test_a_large_library_waits_for_the_listener_and_nothing_is_written(self):
        self.old({'Music/a.flac': b'A' * 2000, 'Music/b.flac': b'B' * 3000})
        with patch.object(im, 'KEEP_LIMIT', 1000):
            result = im.setup(self.ctx, automatic=True)
        self.assertEqual((result['state'], result['reason'], result['files'], result['bytes'], result['serial']),
                         ('Waiting', 'old_storage_has_files', 2, 5000, STOCK_SERIAL))
        self.assertNotIn('mkfs.ext4', self.names())
        self.assertFalse(self.ctx.path(im.SETUP).exists())
        self.assertIn(OLD_LINE, self.ctx.read('/proc/self/mountinfo'))          # Old Storage stays readable
        mount = next(c for c in self.calls if c[0] == 'mount')
        self.assertTrue(mount[4].startswith('ro,'))
        # Set Up Now: only with the old storage's serial; then it is erased, nothing kept.
        self.assertEqual(im.setup(self.ctx, confirm_serial='AAAA-BBBB')['reason'],
                         'confirmation_does_not_match_stock_filesystem')
        self.assertNotIn('mkfs.ext4', self.names())
        with patch.object(im, 'KEEP_LIMIT', 1000):
            result = im.setup(self.ctx, confirm_serial=STOCK_SERIAL)
        self.assertEqual((result['state'], result['kept']), ('Done', 0))
        self.assertNotIn(OLD_LINE, self.ctx.read('/proc/self/mountinfo'))

    def test_y2data_must_keep_its_reserve(self):
        self.old({'Music/a.flac': b'A' * 2000})
        with patch.object(im, 'DATA_RESERVE', 1 << 62):
            self.assertEqual(im.setup(self.ctx, automatic=True)['state'], 'Waiting')
        self.assertNotIn('mkfs.ext4', self.names())

    def test_an_unreadable_old_storage_is_never_set_up_automatically(self):
        self.fat_check = {'ok': False, 'reason': 'command_failed', 'output': '/Music/x.mp3\n  File size is 9 bytes'}
        result = im.setup(self.ctx, automatic=True)
        self.assertEqual((result['state'], result['reason']), ('Waiting', 'old_storage_check_failed'))
        self.assertNotIn('mkfs.ext4', self.names())
        # A card pulled once (dirty bit only) is fine (#44 rules).
        self.fat_check = {'ok': False, 'reason': 'command_failed', 'output':
                          'Dirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n'
                          ' Automatically removing dirty bit.\nLeaving filesystem unchanged.'}
        self.assertEqual(im.setup(self.ctx, automatic=True)['state'], 'Done')

    def test_it_waits_for_a_confirmed_update_charge_or_enough_battery(self):
        self.put('/data/updates/state.json', json.dumps({'schema': 2, 'state': 'PendingHealth'}))
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'update_in_progress')
        self.ctx.path('/data/updates/state.json').unlink()
        self.put('/sys/class/power_supply/BAT0/voltage_now', '3600000')
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'battery_low')
        self.ctx.path('/sys/class/power_supply/BAT0/voltage_now').unlink()
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'battery_unknown')
        self.assertFalse(any(c[0] in ('mkfs.ext4', 'fsck.fat', 'mount') for c in self.calls))
        self.put('/sys/class/power_supply/BAT0/status', 'Charging')
        self.assertEqual(im.setup(self.ctx, automatic=True)['state'], 'Done')

    def test_anything_but_the_stock_fat_is_left_alone(self):
        for fs, reason in (({'TYPE': 'ext4', 'LABEL': 'Y2DATA', 'UUID': 'x'}, 'not_stock_storage'),
                           ({}, 'not_stock_storage')):
            self.fs, self.calls = fs, []
            self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], reason, fs)
            self.assertNotIn('mkfs.ext4', self.names())

    def resume_from(self, phase, fs, staged=None):
        im.save(self.ctx, {'schema': 1, 'device': '/dev/mmcblk0p8', 'start': im.START, 'size': im.SIZE,
                           'serial': STOCK_SERIAL, 'phase': phase, 'files': len(staged or {}),
                           'bytes': sum(len(v) for v in (staged or {}).values())})
        for name, data in (staged or {}).items():
            self.put(im.KEPT_STAGING + '/' + name, data)
        self.fs = fs
        return im.setup(self.ctx, automatic=True)

    def test_power_lost_while_formatting_formats_again_only_with_the_record(self):
        result = self.resume_from('formatting', {}, {'Music/a.mp3': b'A' * 10})   # p8 unreadable now
        self.assertEqual((result['state'], result['kept']), ('Done', 1))
        self.assertEqual(self.ctx.path('/media/internal/music/%s/Music/a.mp3' % im.KEPT).read_bytes(), b'A' * 10)
        # Without a record an unreadable p8 is only reported.
        self.calls.clear()
        self.fs = {}
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'not_stock_storage')
        self.assertNotIn('mkfs.ext4', self.names())

    def test_power_lost_after_formatting_or_while_moving_resumes_without_formatting(self):
        self.provisioned()
        result = self.resume_from('formatting', dict(self.fs), {'a.mp3': b'A'})
        self.assertEqual(result['state'], 'Done')
        self.assertNotIn('mkfs.ext4', self.names())
        self.put('/proc/self/mountinfo', '')
        result = self.resume_from('migrating', dict(self.fs), {'b.mp3': b'B'})
        self.assertEqual(result['state'], 'Done')
        self.assertNotIn('mkfs.ext4', self.names())
        self.assertTrue(self.ctx.path('/media/internal/music/%s/b.mp3' % im.KEPT).exists())

    def test_power_lost_while_copying_redoes_the_copy_from_the_unchanged_old_storage(self):
        self.old({'Music/a.mp3': b'A' * 100})
        self.put(im.KEPT_STAGING + '/Music/a.mp3.part', b'AA')                       # interrupted copy
        result = self.resume_from('staging', {'TYPE': 'vfat', 'UUID': STOCK_SERIAL})
        self.assertEqual(result['state'], 'Done')
        self.assertEqual(self.ctx.path('/media/internal/music/%s/Music/a.mp3' % im.KEPT).read_bytes(), b'A' * 100)
        self.assertFalse(list(self.ctx.path('/media/internal/music').rglob('*.part')))

    def test_a_record_for_another_partition_or_a_changed_old_storage_stops(self):
        im.save(self.ctx, {'schema': 1, 'device': '/dev/mmcblk0p9', 'start': im.START, 'size': im.SIZE,
                           'serial': STOCK_SERIAL, 'phase': 'formatting'})
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'setup_record_mismatch')
        result = self.resume_from('formatting', {'TYPE': 'vfat', 'UUID': 'AAAA-BBBB'})
        self.assertEqual(result['reason'], 'old_storage_changed')
        self.ctx.path(im.SETUP).write_text('{broken')
        self.assertEqual(im.setup(self.ctx, automatic=True)['reason'], 'setup_record_unreadable')
        self.assertNotIn('mkfs.ext4', self.names())

    def test_leftover_rules(self):
        for path in ('Android/data/com.android.providers.media/albumthumbs/1', 'DCIM/.thumbnails/a.jpg',
                     '.android_secure/x', 'Music/.nomedia', 'System Volume Information/IndexerVolumeGuid',
                     'Music/._Song.mp3', '.DS_Store', 'Pictures/Thumbs.db'):
            self.assertTrue(im.leftover(path), path)
        for path in ('Music/Song.mp3', 'LOST.DIR/0001', 'Themes/x/skin.json', 'Android/data/com.app/files/a',
                     'Android/data/com.android.providers.media/other'):
            self.assertFalse(im.leftover(path), path)
        self.assertEqual([im.decide([], 0), im.decide([('a', 10)], 10 ** 9), im.decide([('a', 10 ** 9)], 10 ** 10)],
                         [1, 2, 3])
