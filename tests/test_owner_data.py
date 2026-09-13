"""Public-only personalization and explicit data initialization boundaries."""
import base64
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
from tools.production.layout import make_data_scatter, make_scatter, scatter_rows
from tools.production.owner_data import public_key

ROOT = Path(__file__).resolve().parents[1]


class OwnerData(unittest.TestCase):
    def test_private_paths_and_symlinks_are_refused_before_read(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            private = p/'y2linux_ed25519'; private.write_bytes(b'must never be read')
            public = p/'y2linux_ed25519.pub'; public.symlink_to(private)
            with patch.object(Path, 'read_bytes', side_effect=AssertionError('content read')):
                for path in (private, public):
                    with self.assertRaises(ValueError): public_key(path)
            public.unlink()
            # Synthetic public encoding only: no private key or identity generation.
            blob = struct.pack('>I', 11)+b'ssh-ed25519'+struct.pack('>I', 32)+bytes(range(32))
            data = b'ssh-ed25519 '+base64.b64encode(blob)+b' fixture\n'
            public.write_bytes(data); self.assertEqual(public_key(public), data)
            public.write_bytes(data+data)
            with self.assertRaises(ValueError): public_key(public)

    def test_data_reset_profile_never_changes_normal_preserving_update(self):
        stock = (ROOT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
        old = scatter_rows(stock); new = scatter_rows(make_data_scatter(stock))
        self.assertEqual([r['partition_name'] for r in new if r['is_download']=='true'], ['USRDATA'])
        for a, b in zip(old, new):
            for field in a.keys()-{'file_name', 'is_download'}: self.assertEqual(a[field], b[field])
            self.assertEqual(b['file_name'], 'Y2DATA.img' if b['partition_name']=='USRDATA' else 'NONE')
        normal = scatter_rows(make_scatter(stock, False))
        self.assertEqual([r['partition_name'] for r in normal if r['is_download']=='true'], ['BOOTIMG', 'ANDROID'])
