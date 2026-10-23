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
from y2_platform.observe import storage, device_instance
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
        self.assertEqual(len(result['measurements']), 24)
        self.assertEqual(len(result['integrity']), 5)
        self.assertTrue(all(r['expected_sha256'] == r['readback_sha256'] for r in result['integrity']))
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

    def test_retried_controller_fault_is_not_a_clean_benchmark_pass(self):
        before = {'cpu_ticks': None, 'irqs': None, 'controllers': {'sd': {'crc_events': 0}}}
        after = {'cpu_ticks': None, 'irqs': None, 'controllers': {'sd': {'crc_events': 1}}}
        with patch('y2_platform.bench.io_snapshot', side_effect=[before, after]):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'FAILED')
        self.assertIn('controller_fault_during_benchmark', result['record']['failure']['reason'])
        self.assertEqual(len(result['measurements']), 1)
        self.assertEqual(result['measurements'][0]['system']['controllers']['sd']['delta']['crc_events'], 1)

    def test_offset_specific_sha256_detects_address_aliasing(self):
        real_pread = os.pread
        def wrong_offset(fd, count, offset):
            return real_pread(fd, count, 0) if count == 4099 else real_pread(fd, count, offset)
        with patch('y2_platform.bench.os.pread', side_effect=wrong_offset):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'FAILED')
        self.assertIn('sha256_readback_mismatch', result['record']['failure']['reason'])
        self.assertEqual(list(self.ctx.path('/data/.y2-bench').iterdir()), [])

    def test_retried_fault_during_sha256_is_not_a_clean_integrity_pass(self):
        before = {'cpu_ticks': None, 'irqs': None, 'controllers': {'sd': {'crc_events': 0}}}
        after = {'cpu_ticks': None, 'irqs': None, 'controllers': {'sd': {'crc_events': 1}}}
        with patch('y2_platform.bench.io_snapshot', side_effect=[before] * 49 + [after]):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(len(result['integrity']), 5)
        self.assertEqual(result['record']['result'], 'FAILED')
        self.assertEqual(result['integrity_system']['controllers']['sd']['delta']['crc_events'], 1)

    def test_benchmark_reports_cpu_irq_and_storage_error_deltas(self):
        from y2_platform.bench import io_delta
        first = {'cpu_ticks': [1, 0, 1, 10, 2, 0, 0, 0], 'irqs': {'39': 5},
                 'controllers': {'sd': {'level': 'SDR104', 'crc_events': 0}}}
        last = {'cpu_ticks': [2, 0, 2, 12, 2, 0, 0, 0], 'irqs': {'39': 8},
                'controllers': {'sd': {'level': 'DDR50', 'crc_events': 1}}}
        delta = io_delta(first, last)
        self.assertEqual(delta['cpu_busy_percent'], 50)
        self.assertEqual(delta['irq_delta'], {'39': 3})
        self.assertEqual(delta['controllers']['sd']['delta']['crc_events'], 1)
        self.assertIsNone(delta['controllers']['sd']['delta']['timeout_events'])

    def test_missing_cache_advice_preserves_readback_without_cold_read_claim(self):
        with patch('y2_platform.bench.os.posix_fadvise', side_effect=OSError(errno.EINVAL, 'unavailable')):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'OK')
        self.assertEqual(len(result['cache_advice']), 13)
        self.assertTrue(all(a['state'] == 'unavailable' and a['errno'] == errno.EINVAL
                            for a in result['cache_advice']))
        self.assertEqual(result['record']['parameters']['cold_read'], 'not_guaranteed')
        self.assertTrue(any(m['operation'] == 'random_read_1048576' for m in result['measurements']))
        self.assertEqual(list(self.ctx.path('/data/.y2-bench').iterdir()), [])

    def test_cache_advice_io_fault_is_not_downgraded_to_unavailable(self):
        with patch('y2_platform.bench.os.posix_fadvise', side_effect=OSError(errno.EIO, 'media fault')):
            result = storage_benchmark(self.ctx, size_mib=1, operations=1, guard=lambda: 'mount1')
        self.assertEqual(result['record']['result'], 'FAILED')
        self.assertEqual(result['record']['failure']['errno'], errno.EIO)
        self.assertFalse(any('read_' in m['operation'] for m in result['measurements']))

    def card_fixture(self):
        partition = self.ctx.path('/sys/devices/platform/11240000.mmc/block/mmcblk1/mmcblk1p1')
        partition.mkdir(parents=True)
        (partition / 'partition').write_text('1')
        (partition / 'dev').write_text('179:9')
        (partition.parent / 'diskseq').write_text('17')
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

    def test_block_identity_survives_metadata_changes_and_rejects_reenumeration(self):
        self.card_fixture()
        entry = self.ctx.path('/sys/dev/block/179:9').resolve()
        expected = device_instance(self.ctx, '179:9')
        self.assertEqual(expected, 'block:179:9:diskseq:17')
        os.utime(entry, (1, 1))
        self.assertEqual(device_instance(self.ctx, '179:9'), expected)
        (entry.parent / 'diskseq').write_text('18')
        self.assertNotEqual(device_instance(self.ctx, '179:9'), expected)
        (entry.parent / 'diskseq').unlink()
        self.assertIsNone(device_instance(self.ctx, '179:9'))
        (entry.parent / 'diskseq').write_text('0')
        self.assertIsNone(device_instance(self.ctx, '179:9'))

    def test_missing_diskseq_cannot_validate_a_null_mount_claim(self):
        self.card_fixture()
        operation(self.ctx, 'mount')
        entry = self.ctx.path('/sys/dev/block/179:9').resolve()
        (entry.parent / 'diskseq').unlink()
        self.assertEqual(storage(self.ctx)['volumes'][2]['reason'], 'block_instance_changed')

    def test_status_exposes_passive_runtime_pm_and_tuning(self):
        base = '/sys/bus/platform/devices/11240000.mmc'
        self.put(base + '/y2_performance', 'cap_hz=100000000 actual_hz=99999885 transport_errors=0')
        self.put(base + '/y2_storage', 'level=SDR50 width=4 signal_mv=1800 crc_events=0')
        self.put(base + '/y2_runtime_pm', 'gated=1 retained=37 last_mismatch=0x0 error=0')
        self.put(base + '/power/runtime_status', 'suspended')
        self.put(base + '/y2_tune_diag', 'tune.taps=32 tune.runs=1')
        # The diagnostic attribute resumes hardware; status must not read it.
        self.put(base + '/y2_storage_diag', 'DO_NOT_READ')
        value = storage(self.ctx)['controllers'][0]
        self.assertEqual(value['runtime_pm']['gated'], 1)
        self.assertEqual(value['runtime_pm']['last_mismatch'], 0)
        self.assertEqual(value['runtime_status'], 'suspended')
        self.assertIn('tune.taps=32', value['tuning'])

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

    def test_zero_uuid_whole_card_uses_cid_and_zero_start(self):
        from y2_platform.media import media_identity
        disk = self.ctx.path('/sys/devices/platform/11240000.mmc/block/mmcblk1')
        (disk / 'device').mkdir(parents=True)
        (disk / 'device/cid').write_text('whole-card-cid')
        (disk / 'size').write_text('1000000')
        identity = media_identity(disk, 'vfat', None)
        self.assertEqual(identity['geometry']['start'], '0')
        self.assertIsNone(identity['geometry']['partition'])
        self.assertEqual(identity['cid'], 'whole-card-cid')

    def test_uuidless_fat_identity_needs_geometry_and_detects_replacement(self):
        self.card_fixture()
        entry = self.ctx.path('/sys/class/block/mmcblk1p1')
        (entry / 'start').write_text('2048')
        (entry / 'size').write_text('1000000')
        old = self.ctx.runner
        def runner(argv, **kw):
            if argv[0] == 'blkid':
                return {'ok': True, 'reason': None, 'output': '' if 'UUID' in argv else 'TYPE=exfat'}
            return old(argv, **kw)
        self.ctx.runner = runner
        self.assertEqual(operation(self.ctx, 'mount')['state'], 'Ready')
        self.assertEqual(storage(self.ctx)['volumes'][2]['state'], 'Ready')
        (entry / 'size').write_text('999999')
        self.assertEqual(storage(self.ctx)['volumes'][2]['reason'], 'media_identity_changed_or_missing')

    def test_fat_mount_enables_utf8_without_changing_private_mount_masks(self):
        calls = self.card_fixture()
        runner = self.ctx.runner
        def fat(argv, **kwargs):
            result = runner(argv, **kwargs)
            if argv[0] == 'blkid':
                result['output'] = result['output'].replace('exfat', 'vfat')
            if argv[0] == 'mount':
                self.put('/proc/self/mountinfo', '51 1 179:9 / /media/sd rw - vfat /dev/mmcblk1p1 rw')
            return result
        self.ctx.runner = fat
        self.assertEqual(operation(self.ctx, 'mount')['state'], 'Ready')
        args = next(c for c in calls if c[0] == 'mount')
        self.assertEqual(args[args.index('-o') + 1], 'rw,nosuid,nodev,noexec,noatime,fmask=0177,dmask=0077,utf8=1')

    PULLED_FAT = ('fsck.fat 4.2 (2021-01-31)\n'
                  'There are differences between boot sector and its backup.\n'
                  'This is mostly harmless. Differences: (offset:original/backup)\n'
                  '  65:01/00\n'
                  '  Not automatically fixing this.\n'
                  'Dirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n'
                  ' Automatically removing dirty bit.\n'
                  'Leaving filesystem unchanged.\n'
                  '/dev/mmcblk1p1: 13383 files, 1397005/1907917 clusters')

    def fat_card_checked_as(self, output):
        calls = self.card_fixture()
        runner = self.ctx.runner
        def fat(argv, **kwargs):
            if argv[0] == 'fsck.fat':
                calls.append(argv)
                self.assertTrue(kwargs.get('failure_output'))
                return {'ok': False, 'reason': 'command_failed', 'output': output}
            result = runner(argv, **kwargs)
            if argv[0] == 'blkid':
                result['output'] = result['output'].replace('exfat', 'vfat')
            if argv[0] == 'mount':
                self.put('/proc/self/mountinfo', '51 1 179:9 / /media/sd rw - vfat /dev/mmcblk1p1 rw')
            return result
        self.ctx.runner = fat
        return calls

    def test_fat_card_pulled_while_mounted_is_mounted_again(self):
        # Issue #44: fsck.fat -n exits 1 on the dirty bit alone.
        calls = self.fat_card_checked_as(self.PULLED_FAT)
        result = operation(self.ctx, 'mount')
        self.assertEqual(result['state'], 'Ready')
        self.assertEqual(result['filesystem_check']['finding'], 'unclean_unmount')
        self.assertIn(['fsck.fat', '-n', '/dev/mmcblk1p1'], calls)
        self.assertTrue(any(c[0] == 'mount' for c in calls))
        # Admission still never repairs: no -a/-w/-r run.
        self.assertFalse(any(c[0] == 'fsck.fat' and set(c) & {'-a', '-w', '-r', '-y'} for c in calls))
        self.assertEqual(storage(self.ctx)['volumes'][2]['state'], 'Ready')

    def test_fat_card_with_real_findings_stays_out_and_the_status_says_why(self):
        damaged = self.PULLED_FAT.replace(
            'Dirty bit', '/Music/Song.flac\n  File size is 4194304 bytes, cluster chain length is 1048576 bytes.\n'
            '  Truncating file to 1048576 bytes.\nDirty bit')
        calls = self.fat_card_checked_as(damaged)
        result = operation(self.ctx, 'mount')
        self.assertEqual((result['state'], result['reason']), ('Degraded', 'filesystem_check_failed'))
        self.assertFalse(any(c[0] == 'mount' for c in calls))
        # Checker output can name files: it is not kept.
        self.assertIsNone(result['filesystem_check']['output'])
        self.assertNotIn('Song.flac', self.ctx.path('/run/y2/media.json').read_text())
        sd = storage(self.ctx)['volumes'][2]
        self.assertEqual((sd['state'], sd['reason'], sd['filesystem']),
                         ('Degraded', 'filesystem_check_failed', 'vfat'))
        # Removed or reinserted (a new disk sequence number): no longer this card's verdict.
        self.ctx.path('/sys/devices/platform/11240000.mmc/block/mmcblk1/diskseq').write_text('18')
        sd = storage(self.ctx)['volumes'][2]
        self.assertEqual((sd['state'], sd['reason']), ('Unavailable', 'not_uniquely_mounted'))

    def test_fat_dirty_bit_with_other_boot_differences_is_mounted(self):
        # A backup boot sector that differs alone already passes (fsck.fat exits 0 and
        # never fixes it); with the dirty bit it used to keep the card out. fsck.fat
        # wraps a long offset list onto a line starting with ", ".
        calls = self.fat_card_checked_as(self.PULLED_FAT.replace(
            '65:01/00', '3:4d/6d, 4:53/6b, 5:57/66, 6:49/73, 7:4e/2e, 8:34/66, 9:2e/61, 10:31/74\n  , 65:01/00'))
        result = operation(self.ctx, 'mount')
        self.assertEqual(result['state'], 'Ready')
        self.assertEqual(result['filesystem_check']['findings'], ['backup_differs', 'dirty'])
        self.assertTrue(any(c[0] == 'mount' for c in calls))

    # fsck.fat 4.2 -n (LC_ALL=C) on host images with each change (dosfstools as on the player).
    STALE_FREE = ('fsck.fat 4.2 (2021-01-31)\n'
                  'Free cluster summary wrong (1234 vs. really 76393)\n'
                  '  Auto-correcting.\n\n'
                  'Leaving filesystem unchanged.\n'
                  '/dev/mmcblk1p1: 10 files, 250/76643 clusters\n')
    LABEL = ("Volume label 'MUSIC' stored in root directory and label 'Y2' stored in boot sector and different.\n"
             '  Auto-copying volume label from root directory to boot sector.\n')
    LOST = 'Reclaimed 4 unused clusters (16384 bytes).\n'

    def test_fat_findings_windows_mounts_over_are_admitted(self):
        # Reddit/F12: "needs repair" on a card the PC read; reformatting fixed it.
        from y2_platform.media import fat_findings
        dirty = self.PULLED_FAT
        cases = {
            'stale free count': (self.STALE_FREE, ['fsinfo_free']),
            'dirty + stale free count': (dirty.replace('Leaving', 'Free cluster summary wrong (1 vs. really 2)\n'
                                                       '  Auto-correcting.\nLeaving'), ['dirty', 'fsinfo_free']),
            'label mismatch': (self.STALE_FREE.replace('Free cluster summary wrong (1234 vs. really 76393)\n'
                                                      '  Auto-correcting.\n', self.LABEL), ['label_mismatch']),
            'lost clusters + dirty': (dirty.replace('Dirty bit', self.LOST + 'Dirty bit'), ['dirty', 'lost_clusters']),
            'one lost cluster': (dirty.replace('Dirty bit', 'Reclaimed 1 unused cluster (4096 bytes).\nDirty bit'),
                                 ['dirty', 'lost_clusters']),
            # Labels written by other systems' tools (dosfstools check.c).
            'invalid label': (dirty.replace('Dirty bit', "Volume label 'my*card' stored in root directory is not valid.\n"
                                            '  Auto-removing label.\nDirty bit'), ['dirty', 'label_mismatch']),
            'lowercase label': (dirty.replace('Dirty bit', "Volume label 'Music' stored in root directory contains "
                                              'lowercase characters.\n  Auto-changing lowercase characters to uppercase\n'
                                              'Dirty bit'), ['dirty', 'label_mismatch']),
        }
        for name, (output, findings) in cases.items():
            with self.subTest(name):
                self.assertEqual(fat_findings(output), findings)
        calls = self.fat_card_checked_as(self.STALE_FREE)
        result = operation(self.ctx, 'mount')
        self.assertEqual((result['state'], result['filesystem_check']['finding']), ('Ready', 'benign'))
        self.assertFalse(any(c[0] == 'fsck.fat' and set(c) & {'-a', '-w', '-r', '-y'} for c in calls))

    def test_fat_benign_lines_out_of_place_or_with_real_findings_stay_out(self):
        from y2_platform.media import fat_benign_only, fat_findings
        damaged = self.STALE_FREE.replace('Leaving', '/Music/Song.flac\n  Contains a free cluster (77). Assuming EOF.\nLeaving')
        self.assertFalse(fat_benign_only(damaged))
        self.assertIn('other', fat_findings(damaged))
        # "Auto-correcting." answers only the free count, not another finding.
        self.assertFalse(fat_benign_only(self.LOST + '  Auto-correcting.\n'))
        self.assertFalse(fat_benign_only('fsck.fat 4.2 (2021-01-31)\nLeaving filesystem unchanged.\n'))
        calls = self.fat_card_checked_as(damaged)
        result = operation(self.ctx, 'mount')
        self.assertEqual((result['state'], result['reason']), ('Degraded', 'filesystem_check_failed'))
        self.assertIn('other', result['filesystem_check']['findings'])
        self.assertFalse(any(c[0] == 'mount' for c in calls))

    def test_admissions_are_remembered_across_boots_without_names(self):
        # F12 (c75aae95): a card refused at every insertion; only the last card's
        # verdict was kept, in /run. Bounded, no checker output, no label or file names.
        from y2_platform.media import ADMISSIONS, ADMISSIONS_KEPT
        damaged = self.STALE_FREE.replace('Leaving', '/Music/Secret Song.flac\n  Bad cluster chain.\nLeaving')
        self.fat_card_checked_as(damaged)
        for _ in range(ADMISSIONS_KEPT + 3):
            operation(self.ctx, 'mount')
        text = self.ctx.path(ADMISSIONS).read_text()
        entries = json.loads(text)['admissions']
        self.assertEqual(len(entries), ADMISSIONS_KEPT)
        self.assertEqual((entries[-1]['ok'], entries[-1]['findings'], entries[-1]['filesystem']),
                         (False, ['fsinfo_free', 'other'], 'vfat'))
        self.assertIsInstance(entries[-1]['elapsed_ms'], int)
        self.assertNotIn('Secret', text)
        # 29ed4ccb (8.25): what the unclassified line said, without the name.
        self.assertEqual(entries[-1]['other'], {'shapes': ['Bad cluster chain.'], 'path_lines': 1})
        from y2_platform.media import fat_other_shapes
        self.assertEqual(fat_other_shapes("/A/B.mp3\n  Contains a free cluster (77). Assuming EOF.\n"
                                          "Duplicate directory entry 'SECRET  MP3'\n"),
                         {'shapes': ['Contains a free cluster (#). Assuming EOF.', "Duplicate directory entry '…'"],
                          'path_lines': 1})
        self.assertNotIn('Music', text)

    def test_a_check_that_times_out_is_named_apart(self):
        calls = self.card_fixture()
        runner = self.ctx.runner
        def slow(argv, **kwargs):
            if argv[0] == 'fsck.fat':
                return {'ok': False, 'reason': 'command_timeout', 'output': None}
            result = runner(argv, **kwargs)
            if argv[0] == 'blkid':
                result['output'] = result['output'].replace('exfat', 'vfat')
            return result
        self.ctx.runner = slow
        result = operation(self.ctx, 'mount')
        self.assertEqual((result['state'], result['filesystem_check']['reason'], result['filesystem_check']['findings']),
                         ('Degraded', 'command_timeout', ['timeout']))
        sd = storage(self.ctx)['volumes'][2]
        self.assertEqual((sd['reason'], sd['findings']), ('filesystem_check_failed', ['timeout']))

    def test_fsck_is_read_only_and_never_checks_mounted_media(self):
        calls = self.card_fixture()
        self.assertEqual(operation(self.ctx, 'check')['state'], 'Ready')
        self.assertIn(['fsck.exfat', '-n', '/dev/mmcblk1p1'], calls)
        operation(self.ctx, 'mount')
        self.assertEqual(operation(self.ctx, 'check')['reason'], 'already_mounted')
        self.assertEqual(sum(c[0] == 'fsck.exfat' for c in calls), 2)

    def test_mount_requires_read_only_check_and_refuses_dirty_or_timed_out_media(self):
        for reason in ('command_failed', 'timeout'):
            with self.subTest(reason=reason):
                if not self.ctx.path('/sys/class/block/mmcblk1p1').exists():
                    calls = self.card_fixture()
                calls.clear()
                original = self.ctx.runner
                def failure(argv, **kwargs):
                    if argv[0] == 'fsck.exfat':
                        calls.append(argv)
                        self.assertEqual(kwargs['timeout'], 60)
                        return {'ok': False, 'reason': reason, 'output': None}
                    return original(argv, **kwargs)
                self.ctx.runner = failure
                result = operation(self.ctx, 'mount')
                self.assertEqual(result['state'], 'Degraded')
                self.assertEqual(result['reason'], 'filesystem_check_failed')
                self.assertEqual(storage(self.ctx)['media']['last_operation']['filesystem_check']['reason'], reason)
                self.assertFalse(any(c[0] == 'mount' for c in calls))
                self.assertFalse(self.ctx.path('/run/y2/media-mount.json').exists())
                self.ctx.runner = original

    def test_card_replacement_during_check_prevents_mount(self):
        calls = self.card_fixture()
        original = self.ctx.runner
        def replacement(argv, **kwargs):
            if argv[0] == 'fsck.exfat':
                self.ctx.path('/sys/class/block/mmcblk1p1').resolve().parent.joinpath('device/cid').write_text('replacement-cid')
            return original(argv, **kwargs)
        self.ctx.runner = replacement
        result = operation(self.ctx, 'mount')
        self.assertEqual(result['reason'], 'source_changed_during_check')
        self.assertFalse(any(c[0] == 'mount' for c in calls))

    def test_same_card_reenumeration_during_check_prevents_mount(self):
        calls = self.card_fixture()
        original = self.ctx.runner
        def reinsert(argv, **kwargs):
            if argv[0] == 'fsck.exfat':
                self.ctx.path('/sys/class/block/mmcblk1p1').resolve().parent.joinpath('diskseq').write_text('18')
            return original(argv, **kwargs)
        self.ctx.runner = reinsert
        self.assertEqual(operation(self.ctx, 'mount')['reason'], 'source_changed_during_check')
        self.assertFalse(any(c[0] == 'mount' for c in calls))

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
