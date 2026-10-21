"""Real filesystem/checker behavior on newly created host scratch images only."""
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.media import fat_benign_only, fat_findings  # noqa: E402
from y2_platform import internal_media  # noqa: E402


class Filesystems(unittest.TestCase):
    def image(self, directory, size=64 * 1024**2):
        path = Path(directory) / 'scratch.img'
        with path.open('xb') as stream:
            stream.truncate(size)
        return path

    def digest(self, path):
        with path.open('rb') as stream:
            return hashlib.file_digest(stream, 'sha256').hexdigest()

    def run_tool(self, *argv):
        return subprocess.run(argv, capture_output=True, text=True,
                              env={**os.environ, 'LC_ALL': 'C.UTF-8'}, timeout=30)

    @unittest.skipUnless(shutil.which('mkfs.fat') and shutil.which('fsck.fat')
                         and shutil.which('mcopy'), 'native FAT tools required')
    def test_fat32_unicode_long_names_hashes_and_read_only_dirty_check(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.image(directory)
            self.assertEqual(self.run_tool('mkfs.fat', '-F', '32', str(image)).returncode, 0)
            original = Path(directory) / ('音楽-Été-' + 'a' * 180 + '.flac')
            original.write_bytes(bytes(range(256)) * 4096)
            self.assertEqual(self.run_tool('mcopy', '-i', str(image), str(original), '::').returncode, 0)
            copy = Path(directory) / 'readback'
            self.assertEqual(self.run_tool('mcopy', '-i', str(image), '::' + original.name,
                                          str(copy)).returncode, 0)
            self.assertEqual(self.digest(original), self.digest(copy))
            before = self.digest(image)
            self.assertEqual(self.run_tool('fsck.fat', '-n', str(image)).returncode, 0)
            self.assertEqual(self.digest(image), before)
            # Only this new image: clear FAT32's clean-unmount bit in both FATs.
            with image.open('r+b') as stream:
                boot = stream.read(512)
                sector = struct.unpack_from('<H', boot, 11)[0]
                reserved = struct.unpack_from('<H', boot, 14)[0]
                fats = boot[16]
                fat_sectors = struct.unpack_from('<I', boot, 36)[0]
                for index in range(fats):
                    offset = (reserved + index * fat_sectors) * sector + 4
                    stream.seek(offset)
                    flags = struct.unpack('<I', stream.read(4))[0]
                    stream.seek(offset)
                    stream.write(struct.pack('<I', flags & ~(1 << 27)))
            dirty = self.digest(image)
            checked = self.run_tool('fsck.fat', '-n', str(image))
            self.assertIn('Dirty bit is set', checked.stdout)
            self.assertNotEqual(checked.returncode, 0)
            self.assertEqual(self.digest(image), dirty)
            # Issue #44: the Linux driver also sets the boot sector's dirty
            # flag (offset 65), so it differs from its backup. A card left
            # like this (pulled while mounted) is admitted; checker output as
            # the platform runs it (LC_ALL=C).
            with image.open('r+b') as stream:
                stream.seek(65)
                stream.write(b'\x01')
            unclean = subprocess.run(['fsck.fat', '-n', str(image)], capture_output=True, text=True, errors='replace',
                                     env={**os.environ, 'LC_ALL': 'C'}, timeout=30)
            self.assertEqual(unclean.returncode, 1)
            self.assertIn('65:01/00', unclean.stdout)
            self.assertTrue(fat_benign_only(unclean.stdout), unclean.stdout)
            # A real finding on top (a file longer than its clusters) is not.
            with image.open('r+b') as stream:
                boot = stream.read(512)
                sector = struct.unpack_from('<H', boot, 11)[0]
                reserved = struct.unpack_from('<H', boot, 14)[0]
                fat_sectors = struct.unpack_from('<I', boot, 36)[0]
                root = (reserved + boot[16] * fat_sectors) * sector
                stream.seek(root)
                entries = stream.read(sector * boot[13])
                index = next(i for i in range(0, len(entries), 32)
                             if entries[i + 11] not in (0x0F, 0x08) and entries[i] not in (0, 0xE5))
                stream.seek(root + index + 28)
                stream.write(struct.pack('<I', 4 * 1024**2))
            broken = subprocess.run(['fsck.fat', '-n', str(image)], capture_output=True, text=True, errors='replace',
                                    env={**os.environ, 'LC_ALL': 'C'}, timeout=30)
            self.assertNotEqual(broken.returncode, 0)
            self.assertFalse(fat_benign_only(broken.stdout), broken.stdout)

    @unittest.skipUnless(shutil.which('mkfs.fat') and shutil.which('fsck.fat'), 'native FAT tools required')
    def test_fat_findings_windows_mounts_over_on_real_images(self):
        """F12 (Reddit "needs repair", fine on the PC): what a card pulled while
        mounted or relabelled on Windows looks like to the player's checker."""
        def edit(path, offset, data):
            with path.open('r+b') as stream:
                stream.seek(offset)
                stream.write(data)
        with tempfile.TemporaryDirectory() as directory:
            image = self.image(directory)
            self.assertEqual(self.run_tool('mkfs.fat', '-F', '32', '-n', 'Y2', str(image)).returncode, 0)
            boot = image.read_bytes()[:512]
            sector, reserved = struct.unpack_from('<H', boot, 11)[0], struct.unpack_from('<H', boot, 14)[0]
            fats, fat_sectors = boot[16], struct.unpack_from('<I', boot, 36)[0]
            fsinfo = struct.unpack_from('<H', boot, 48)[0]
            check = lambda: subprocess.run(['fsck.fat', '-n', str(image)], capture_output=True, text=True,
                                           errors='replace', env={**os.environ, 'LC_ALL': 'C'}, timeout=30)
            edit(image, 65, b'\x01')                                       # dirty (#44)
            edit(image, fsinfo * sector + 488, struct.pack('<I', 1234))    # stale free count
            for index in range(fats):                                      # an unused chain still marked used
                fat = (reserved + index * fat_sectors) * sector
                for cluster in range(5000, 5004):
                    edit(image, fat + cluster * 4, struct.pack('<I', cluster + 1 if cluster < 5003 else 0x0FFFFFFF))
            edit(image, 3, b'MSWIN4.1')                                    # backup differs elsewhere
            before = self.digest(image)
            result = check()
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertEqual(self.digest(image), before)
            self.assertEqual(fat_findings(result.stdout), ['backup_differs', 'dirty', 'fsinfo_free', 'lost_clusters'])
            self.assertTrue(fat_benign_only(result.stdout), result.stdout)

    @unittest.skipUnless(shutil.which('mkfs.ext4') and shutil.which('dumpe2fs'), 'native ext4 tools required')
    def test_y2media_layout_is_pinned_to_the_first_players(self):
        """STORAGE-SETUP: every player's Y2Media equals the owner's (made 2026-10-10), whatever
        e2fsprogs defaults are: features, inodes, blocks, reserve, label and UUID."""
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / 'p8.img'
            with image.open('xb') as stream:
                stream.truncate(int(internal_media.SIZE) * 512)          # sparse, p8's exact size
            made = self.run_tool(*internal_media.MKFS, str(image))
            self.assertEqual(made.returncode, 0, made.stderr)
            header = dict(line.split(':', 1) for line in
                          self.run_tool('dumpe2fs', '-h', str(image)).stdout.splitlines() if ':' in line)
            field = lambda name: header[name].strip()
            self.assertEqual(field('Filesystem features').split(),
                             ['has_journal', 'ext_attr', 'resize_inode', 'dir_index', 'orphan_file', 'filetype',
                              'extent', '64bit', 'flex_bg', 'metadata_csum_seed', 'sparse_super', 'large_file',
                              'huge_file', 'dir_nlink', 'extra_isize', 'metadata_csum'])
            self.assertEqual((field('Inode count'), field('Block count'), field('Reserved block count'),
                              field('Block size'), field('Inode size')),
                             ('358336', '1432576', '0', '4096', '256'))
            self.assertEqual((field('Filesystem volume name'), field('Filesystem UUID')),
                             (internal_media.LABEL, internal_media.UUID))

    @unittest.skipUnless(shutil.which('mkfs.exfat') and shutil.which('fsck.exfat'),
                         'native exFAT tools required')
    def test_exfat_read_only_checker_preserves_new_image(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.image(directory)
            self.assertEqual(self.run_tool('mkfs.exfat', str(image)).returncode, 0)
            before = self.digest(image)
            self.assertEqual(self.run_tool('fsck.exfat', '-n', str(image)).returncode, 0)
            self.assertEqual(self.digest(image), before)

    @unittest.skipUnless(shutil.which('mkfs.ext4') and shutil.which('e2fsck'),
                         'native ext4 tools required')
    def test_ext4_read_only_checker_preserves_new_image(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.image(directory)
            self.assertEqual(self.run_tool('mkfs.ext4', '-q', '-F', str(image)).returncode, 0)
            before = self.digest(image)
            self.assertEqual(self.run_tool('e2fsck', '-f', '-n', str(image)).returncode, 0)
            self.assertEqual(self.digest(image), before)


if __name__ == '__main__':
    unittest.main()
