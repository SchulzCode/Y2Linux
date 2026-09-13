import hashlib,io,json,tarfile,tempfile,unittest
from pathlib import Path
from tools.production.prepare import buildroot_source
class Prepare(unittest.TestCase):
    def test_locked_archive_extraction_and_corruption_rejection(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'buildroot').mkdir();(p/'.cache/downloads').mkdir(parents=True)
            archive=p/'.cache/downloads/buildroot-test.tar.xz'
            with tarfile.open(archive,'w:xz') as t:
                info=tarfile.TarInfo('buildroot-test/Makefile');info.size=4;t.addfile(info,io.BytesIO(b'all:'))
            lock={'version':'test','sha256':hashlib.sha256(archive.read_bytes()).hexdigest()}
            (p/'buildroot/inputs.lock.json').write_text(json.dumps(lock))
            source,actual=buildroot_source(p);self.assertEqual(actual,lock);self.assertEqual((source/'Makefile').read_bytes(),b'all:')
            archive.write_bytes(b'corrupt')
            with self.assertRaisesRegex(ValueError,'hash mismatch'):buildroot_source(p)
