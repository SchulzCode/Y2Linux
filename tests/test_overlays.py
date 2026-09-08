"""Prevent concurrent invocations from exposing unpatched overlay bytes."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
from pathlib import Path
import tempfile
import unittest
from tools.build.run import prepare_overlay

class OverlayCache(unittest.TestCase):
    def test_atomic_publication_and_corruption_rejection(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);base=root/'.cache/sources/linux-6.18/a'
            base.parent.mkdir(parents=True);base.write_bytes(b'old\n')
            patch=root/'kernel/patches/change.patch';patch.parent.mkdir(parents=True)
            patch.write_bytes(b'--- a\n+++ a\n@@ -1 +1 @@\n-old\n+new\n')
            spec={'path':'a','patch':'change.patch',
                'base_sha256':hashlib.sha256(b'old\n').hexdigest(),
                'result_sha256':hashlib.sha256(b'new\n').hexdigest()}
            with ThreadPoolExecutor(max_workers=8) as pool:
                paths=list(pool.map(lambda _:prepare_overlay(root,spec), range(32)))
            self.assertEqual(len(set(paths)),1)
            p=paths[0];self.assertEqual(p.read_bytes(),b'new\n')
            # Reuse must preserve the inode currently held by another build.
            with p.open('rb') as bound:
                inode=p.stat().st_ino
                prepare_overlay(root,spec)
                self.assertEqual(p.stat().st_ino,inode)
                self.assertEqual(bound.read(),b'new\n')
            p.write_bytes(b'bad\n')
            with self.assertRaises(ValueError): prepare_overlay(root,spec)
            self.assertEqual(p.read_bytes(),b'bad\n')
            self.assertEqual(base.read_bytes(),b'old\n')
