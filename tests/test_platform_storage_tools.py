import errno
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.bench import Scratch, distribution, storage_benchmark
from y2_platform.common import Context
from y2_platform.media import operation, reconcile
from y2_platform.observe import storage
from y2_platform.space import admission, disposable_cleanup


class StorageTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.ctx.path('/data').mkdir()
        self.put('/proc/sys/kernel/random/boot_id', 'boot-one')

    def put(self, path, value):
        p = self.ctx.path(path)
        p.parent.mkdir(exist_ok=True, parents=True)
        p.write_text(value)
        return p

    def test_real_scratch_readback_latency_and_no_live_data_changes(self):
        music = self.put('/data/music/track.flac', 'valuable music')
        state = self.put('/data/reborn/state/settings.json', 'valuable state')
        result = storage_benchmark(self.ctx, size_mib=1, operations=2, seconds=10, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'OK', result['record']['failure'])
        self.assertEqual(len(result['measurements']), 23)
        self.assertTrue(all(m['samples'] > 0 and m['p99_ms'] >= m['p50_ms'] for m in result['measurements']))
        self.assertEqual(music.read_text(), 'valuable music')
        self.assertEqual(state.read_text(), 'valuable state')
        self.assertEqual(list(self.ctx.path('/data/.y2-bench').iterdir()), [])

    def test_source_loss_during_operation_leaves_replacement_untouched(self):
        epoch = ['mount1']
        with Scratch(self.ctx, '/data', 4096, guard=lambda: epoch[0]) as scratch:
            fd = scratch.open('payload')
            os.write(fd, b'old media')
            os.close(fd)
            original = self.ctx.path('/data')
            original.rename(self.ctx.path('/detached'))
            original.mkdir()
            protected = self.put('/data/payload', 'new card')
            epoch[0] = 'mount2'
            with self.assertRaisesRegex(OSError, 'generation'):
                scratch.open('replacement')
        self.assertEqual(protected.read_text(), 'new card')
        self.assertFalse(self.ctx.path('/data/.y2-bench').exists())

    def test_symlink_scratch_and_arbitrary_volumes_refused(self):
        target = self.ctx.path('/state')
        target.mkdir()
        self.ctx.path('/data/.y2-bench').symlink_to(target)
        with self.assertRaises(OSError):
            Scratch(self.ctx, '/data', 4096, guard=lambda: 'mount1')
        with self.assertRaisesRegex(ValueError, 'not_allowed'):
            Scratch(self.ctx, '/data/music', 4096, guard=lambda: 'mount1')
        self.assertFalse(list(target.iterdir()))

    def test_io_error_is_reported_with_completed_samples_not_a_pass(self):
        with patch('y2_platform.bench.write_all', side_effect=OSError(errno.ENOSPC, 'disk full')):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'FAILED')
        self.assertEqual(result['record']['failure']['errno'], errno.ENOSPC)
        self.assertEqual(result['measurements'], [])
        self.assertEqual(list(self.ctx.path('/data/.y2-bench').iterdir()), [])

    def test_percentiles_and_units(self):
        result = distribution([1_000_000, 4_000_000, 2_000_000, 3_000_000], 1_000_000, 10_000_000)
        self.assertEqual(result['p50_ms'], 2)
        self.assertEqual(result['p95_ms'], 4)
        self.assertEqual(result['MB_per_second'], 100)
        self.assertEqual(result['operations_per_second'], 400)

    def card_fixture(self):
        partition = self.ctx.path('/sys/devices/platform/11240000.mmc/block/mmcblk1/mmcblk1p1')
        partition.mkdir(parents=True)
        (partition / 'partition').write_text('1')
        (partition.parent / 'device').mkdir()
        (partition.parent / 'device/cid').write_text('card-cid-one')
        for linkname in ('/sys/class/block/mmcblk1p1', '/sys/dev/block/179:9'):
            link = self.ctx.path(linkname)
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(partition)
        self.put('/proc/self/mountinfo', '')
        self.ctx.path('/media/sd').mkdir(parents=True)
        calls = []
        def runner(argv, **kwargs):
            calls.append(argv)
            output = ''
            if argv[0] == 'blkid':
                output = 'card-uuid-one' if 'UUID' in argv else 'TYPE=exfat\nUUID=card-uuid-one'
            if argv[0] == 'mount':
                self.put('/proc/self/mountinfo', '51 1 179:9 / /media/sd rw - exfat /dev/mmcblk1p1 rw')
            if argv[0] == 'umount':
                return {'ok': False, 'reason': 'command_failed', 'output': None}
            return {'ok': True, 'reason': None, 'output': output}
        self.ctx.runner = runner
        return calls

    def test_exfat_mount_claim_failed_eject_and_card_swap(self):
        calls = self.card_fixture()
        mounted = operation(self.ctx, 'mount')
        self.assertEqual(mounted['state'], 'Ready')
        self.assertEqual(storage(self.ctx)['volumes'][2]['state'], 'Ready')
        result = operation(self.ctx, 'unmount')
        self.assertEqual(result['state'], 'Failed')
        self.assertEqual(self.ctx.json('/run/y2/media-mount.json')['mount_id'], 51)
        result = operation(self.ctx, 'mount')
        self.assertEqual(result['reason'], 'mount_requires_unmounted_path')
        self.assertEqual(len([c for c in calls if c[0] == 'mount']), 1)
        self.ctx.runner = lambda *a, **k: {'ok': True, 'output': 'new-card-uuid', 'reason': None}
        self.assertEqual(storage(self.ctx)['volumes'][2]['reason'], 'mount_claim_changed_or_missing')

    def test_fsck_is_read_only_and_never_checks_mounted_media(self):
        calls = self.card_fixture()
        self.assertEqual(operation(self.ctx, 'check')['state'], 'Ready')
        self.assertIn(['fsck.exfat', '-n', '/dev/mmcblk1p1'], calls)
        operation(self.ctx, 'mount')
        self.assertEqual(operation(self.ctx, 'check')['reason'], 'already_mounted')
        self.assertEqual(sum(c[0] == 'fsck.exfat' for c in calls), 1)

    def test_removed_card_can_be_unmounted_only_with_its_retained_claim(self):
        calls = self.card_fixture()
        operation(self.ctx, 'mount')
        self.ctx.path('/sys/dev/block/179:9').unlink()
        old_runner = self.ctx.runner
        def runner(argv, **kw):
            if argv[0] == 'umount':
                self.put('/proc/self/mountinfo', '')
                return {'ok': True, 'reason': None, 'output': ''}
            return old_runner(argv, **kw)
        self.ctx.runner = runner
        self.assertEqual(operation(self.ctx, 'unmount')['reason'], 'ejected')
        self.put('/proc/self/mountinfo', '52 1 179:10 / /media/sd rw - ext4 /dev/unknown rw')
        self.assertEqual(operation(self.ctx, 'unmount')['reason'], 'mounted_source_not_sd')

    def test_automatic_mount_runs_once_per_insertion_and_failed_eject_is_bounded(self):
        self.card_fixture()
        self.assertEqual(reconcile(self.ctx)['state'], 'Ready')
        self.assertEqual(reconcile(self.ctx)['attempts'], 1)
        self.ctx.path('/sys/class/block/mmcblk1p1').unlink()
        for _ in range(8):
            value = reconcile(self.ctx)
        self.assertEqual(value['attempts'], 5)
        self.assertFalse(value['retry'])
        self.assertEqual(value['reason'], 'source_change_unmount_failed')

    def test_low_space_denies_disposable_writes_but_reserves_checkpoint_budget(self):
        volume = {'space_state': 'LowSpace', 'available_bytes': 80 * 1024**2}
        self.assertFalse(admission(volume, purpose='cache')['allowed'])
        self.assertFalse(admission(volume, purpose='update')['allowed'])
        self.assertTrue(admission(volume, 1024, 'user_state')['allowed'])
        self.assertFalse(admission(volume, 60 * 1024**2, 'database')['allowed'])
        volume['space_state'] = 'ReadOnlyRisk'
        self.assertFalse(admission(volume, 1, 'user_state')['allowed'])

    def test_cleanup_is_bounded_and_cannot_follow_links_or_delete_user_files(self):
        old = self.put('/data/reborn/cache/1-2-3.rgba', 'old cache')
        recent = self.put('/data/reborn/cache/4-5-6.rgba', 'recent cache')
        music = self.put('/data/music/track.flac', 'music')
        state = self.put('/data/reborn/state/settings.json', 'settings')
        self.ctx.path('/data/reborn/cache/7-8-9.rgba').symlink_to(music)
        os.utime(old, (1, 1))
        for n in range(4):
            p = self.put(f'/data/reborn/diagnostics/reborn-diagnostic-1-{n}.tar.gz', 'old diagnostic')
            os.utime(p, (1, 1))
        result = disposable_cleanup(self.ctx, max_files=2)
        self.assertEqual(result['files'], 2)
        self.assertFalse(old.exists())
        self.assertTrue(recent.exists())
        self.assertEqual(music.read_text(), 'music')
        self.assertEqual(state.read_text(), 'settings')
        self.assertTrue(self.ctx.path('/data/reborn/cache/7-8-9.rgba').is_symlink())


if __name__ == '__main__':
    unittest.main()
