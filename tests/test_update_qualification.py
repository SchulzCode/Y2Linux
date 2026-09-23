"""Real private ext4 fault derivative; no device or original-image mutation."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from tools.update.qualification_root import prepare, debugfs

class QualificationRoot(unittest.TestCase):
    def test_broken_app_fixture_is_explicit_and_original_root_is_unchanged(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);base=root/'candidate';tree=root/'tree';base.mkdir();(base/'metadata').mkdir()
            versions=dict(build_git_commit='1'*40,reborn_source_commit='2'*40,kernel_version='test',
                          rootfs_version='root-v1',release_version='1.0-test')
            for path,data in [('usr/bin/reborn','fixture app never executed'),('etc/y2linux/versions.json',json.dumps(versions))]:
                file=tree/path;file.parent.mkdir(parents=True,exist_ok=True);file.write_text(data)
            image=base/'Y2ROOT.img'
            with image.open('wb') as stream:stream.truncate(8*1024**2)
            subprocess.run(['mkfs.ext4','-q','-F','-L','Y2ROOT','-U','79324c69-6e75-4801-8000-000000000101','-d',str(tree),str(image)],check=True,capture_output=True)
            original=image.read_bytes();digest=hashlib.sha256(original).hexdigest()
            (base/'manifest.json').write_text(json.dumps({'payloads':[{'target_partition':'ANDROID','raw':{'file':'Y2ROOT.img','size_bytes':len(original),'sha256':digest}}]}))
            (base/'metadata/versions.json').write_text(json.dumps(versions))
            receipt=prepare(base,root/'fault')
            self.assertTrue(receipt['not_a_release']);self.assertFalse(receipt['device_accessed'])
            self.assertNotEqual(receipt['derived_root_sha256'],digest)
            self.assertEqual(image.read_bytes(),original)
            self.assertIn('fixture app never executed',debugfs(image,'cat /usr/bin/reborn'))
            self.assertIn('File not found',debugfs(root/'fault/INTENTIONALLY-BROKEN-APP.ext4','stat /usr/bin/reborn'))
            self.assertIn('qualification-no-app',receipt['versions']['rootfs_version'])
            with self.assertRaises(FileExistsError):prepare(base,root/'fault')

if __name__=='__main__':unittest.main()
