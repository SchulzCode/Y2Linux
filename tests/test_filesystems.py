"""Real filesystem/checker behavior on newly created host scratch images only."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


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
