"""Every overlay patch must reproduce its manifest result hash.

POWER-FINAL-16: run.py caches overlays by result_sha256, so a patch edit whose
manifest hash was not updated silently built the previous overlay (the MMSYS
DCM hook in 0011 never reached candidate 16).
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [Path('/src'), ROOT / '.cache/sources/linux-6.18']


class OverlayHashes(unittest.TestCase):
    def test_patches_reproduce_manifest_results(self):
        source = next((s for s in SOURCES if (s / 'Makefile').exists()), None)
        if source is None:
            self.skipTest('pinned Linux source unavailable')
        specs = json.loads((ROOT / 'kernel/patches/manifest.json').read_text())['overlays']
        with tempfile.TemporaryDirectory() as temp:
            for spec in specs:
                work = Path(temp) / 'file'
                shutil.copyfile(source / spec['path'], work)
                # The build container's /src already carries the overlays:
                # reverse the patch first, then check both directions.
                if hashlib.sha256(work.read_bytes()).hexdigest() == spec['result_sha256']:
                    subprocess.run(['patch', '--batch', '--fuzz=0', '-R', str(work)],
                                   input=(ROOT / 'kernel/patches' / spec['patch']).read_bytes(),
                                   check=True, capture_output=True)
                self.assertEqual(hashlib.sha256(work.read_bytes()).hexdigest(), spec['base_sha256'], spec['path'])
                subprocess.run(['patch', '--batch', '--fuzz=0', str(work)],
                               input=(ROOT / 'kernel/patches' / spec['patch']).read_bytes(),
                               check=True, capture_output=True)
                self.assertEqual(hashlib.sha256(work.read_bytes()).hexdigest(), spec['result_sha256'],
                                 spec['patch'])


class OverlayMtimes(unittest.TestCase):
    def test_an_overlay_that_changed_for_its_path_gets_a_new_mtime(self):
        # Test build 6452: a reverted patch brought back an older cached overlay,
        # make kept the newer object and the backed-out code shipped.
        import importlib.util, os, time
        spec = importlib.util.spec_from_file_location('y2_run', ROOT / 'tools/build/run.py')
        run = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(run)
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary)
            a, b = out / 'a', out / 'b'
            for path in (a, b):
                path.write_text('x')
                os.utime(path, (1000, 1000))
            specs = [{'path': 'drivers/a.c', 'result_sha256': 'aa'}, {'path': 'drivers/b.c', 'result_sha256': 'bb'}]
            run.touch_changed_overlays(out, specs, [a, b])      # no record: everything is touched
            self.assertGreater(a.stat().st_mtime, time.time() - 60)
            for path in (a, b):
                os.utime(path, (1000, 1000))
            run.touch_changed_overlays(out, specs, [a, b])      # unchanged: left alone
            self.assertEqual((a.stat().st_mtime, b.stat().st_mtime), (1000, 1000))
            specs[1] = {'path': 'drivers/b.c', 'result_sha256': 'b-older'}
            run.touch_changed_overlays(out, specs, [a, b])      # b went back: touched
            self.assertEqual(a.stat().st_mtime, 1000)
            self.assertGreater(b.stat().st_mtime, time.time() - 60)


if __name__ == '__main__':
    unittest.main()
