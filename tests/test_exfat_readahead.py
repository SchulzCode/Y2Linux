"""POWER-FINAL-30: cold exFAT directory reads on the SD card.

Measured on 29 (boot cac3e209, SD128 at SDR104, exFAT 128 KiB clusters): a cold
listing of one directory issued ~200 separate 512-byte reads (0 merged, ~50 ms);
the 118-directory card took 7.95 s cold and 38 ms warm, and Reborn's start-up
scan spent ~7 s on it. exfat_dir_readahead() submitted the cluster's sectors
unplugged; exfat_load_bitmap() in the same driver already plugs its readahead.
"""
from pathlib import Path
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ExfatDirReadahead(unittest.TestCase):
    def test_directory_readahead_is_plugged(self):
        patch = (ROOT / 'kernel/patches/0068-exfat-dir-readahead-plug.patch').read_text()
        self.assertIn('+#include <linux/blkdev.h>', patch)
        added = [l[1:].strip() for l in patch.splitlines() if l.startswith('+') and not l.startswith('+++')]
        start = added.index('blk_start_plug(&plug);')
        finish = added.index('blk_finish_plug(&plug);')
        self.assertLess(start, finish)
        # Only the plug is new: the readahead count and loop are upstream's.
        self.assertNotIn('sb_breadahead(sb, (sector_t)(sec + i));', added)
        manifest = json.loads((ROOT / 'kernel/patches/manifest.json').read_text())
        spec = [o for o in manifest['overlays'] if o['path'] == 'fs/exfat/dir.c']
        self.assertEqual([s['patch'] for s in spec], ['0068-exfat-dir-readahead-plug.patch'])

    def test_fat_and_ext4_batches_are_plugged_too(self):
        for name, loop in (('0069-fat-dir-readahead-plug.patch', 'sb_breadahead(sb, phys + sec);'),
                           ('0070-ext4-bread-batch-plug.patch', 'ext4_read_bh_lock(bhs[i], REQ_META | REQ_PRIO, false);')):
            patch = (ROOT / 'kernel/patches' / name).read_text()
            lines = patch.splitlines()
            start = next(i for i, l in enumerate(lines) if l == '+\tblk_start_plug(&plug);' or l == '+\t\tblk_start_plug(&plug);')
            finish = next(i for i, l in enumerate(lines) if l.endswith('blk_finish_plug(&plug);'))
            body = [l for l in lines[start:finish] if loop in l]
            self.assertTrue(body and all(not l.startswith(('+', '-')) for l in body), name)
        self.assertIn('+#include <linux/blkdev.h>', (ROOT / 'kernel/patches/0069-fat-dir-readahead-plug.patch').read_text())

    def test_every_card_format_mounts_noatime(self):
        media = (ROOT / 'tools/platform/y2_platform/media.py').read_text()
        self.assertIn("options = 'rw,nosuid,nodev,noexec,noatime'", media)
        internal = (ROOT / 'tools/platform/y2_platform/internal_media.py').read_text()
        self.assertIn("OPTIONS = 'rw,nosuid,nodev,noexec,noatime,noinit_itable'", internal)


if __name__ == '__main__':
    unittest.main()
