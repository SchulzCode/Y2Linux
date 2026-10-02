import base64
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from unittest.mock import patch
from tools.production import first_owner as f
from tools.production.layout import TARGETS


class FirstOwner(unittest.TestCase):
    @unittest.skipUnless(shutil.which('mke2fs') and shutil.which('debugfs'), 'host e2fsprogs required')
    def test_fresh_seed_has_only_public_key_and_empty_state(self):
        key = b'ssh-ed25519 '+base64.b64encode(struct.pack('>I',11)+b'ssh-ed25519'+struct.pack('>I',32)+bytes(range(32)))+b' fixture\n'
        with tempfile.TemporaryDirectory() as temporary:
            image = Path(temporary)/'seed.img'
            # Same implementation on a small ext4 image; production size is an
            # existing partition contract already covered by layout tests.
            with patch.dict(TARGETS, USRDATA=dict(TARGETS['USRDATA'], size=32*1024**2)):
                f.make_seed(image, key)
                self.assertEqual(image.stat().st_size, 32*1024**2)
                with self.assertRaises(ValueError): f.make_seed(image, key)

    def test_extra_images_cannot_hide_behind_regenerated_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            out=Path(temporary)
            for name in ('manifest.json','system-manifest.json','owner-authorized.pub','Y2DATA.img',
                         'MT6582_Y2DATA_only_scatter.txt','README.txt','SHA256SUMS','preloader.bin'):
                (out/name).write_bytes(b'fixture')
            with self.assertRaisesRegex(ValueError,'exact first-owner file allowlist'):
                f.validate(out)


if __name__ == '__main__': unittest.main()
