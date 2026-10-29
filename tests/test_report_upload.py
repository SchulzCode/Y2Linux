"""Error reports R4: the release build carries the upload key only when there is a checked one.

The uploader itself (queue, upload, the service's runs, the receiver's triggers and schema) is tested in
Rust: platform/crates/y2-platform/src/commands/reports/upload_tests.rs and
src/commands/service/daemon_reports_tests.rs."""
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
KEY = 'k' * 43


class BuildAndReceiver(unittest.TestCase):
    def test_the_release_build_carries_a_checked_upload_key_only_when_there_is_one(self):
        from tools.production.build import report_upload_key
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source, out = tmp / 'report-upload.json', tmp / 'out'
            out.mkdir()
            self.assertFalse(report_upload_key(out, source))
            source.write_text(json.dumps({'upload_key': KEY}))
            source.chmod(0o644)
            with self.assertRaises(SystemExit):
                report_upload_key(out, source)
            source.chmod(0o600)
            self.assertTrue(report_upload_key(out, source))
            self.assertEqual(oct((out / 'report-upload.json').stat().st_mode & 0o777), '0o600')
            self.assertEqual(json.loads((out / 'report-upload.json').read_text()), {'upload_key': KEY})
            for bad in ({'upload_key': 'short'}, {'upload_key': KEY, 'extra': 1}, {'key': KEY}):
                source.write_text(json.dumps(bad))
                with self.subTest(bad=bad), self.assertRaises(SystemExit):
                    report_upload_key(out, source)
                self.assertFalse((out / 'report-upload.json').exists())
        post = (ROOT / 'buildroot/board/y2/post-build-production.sh').read_text()
        self.assertIn('install -D -m 600 "$Y2_ARTIFACT_DIR/report-upload.json" '
                      '"$target/usr/share/y2linux/report-upload.json"', post)
        self.assertIn('rm -f "$target/usr/share/y2linux/report-upload.json"', post)

    def test_the_key_maker_writes_a_private_key_and_never_replaces_one_by_accident(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('new_upload_key', ROOT / 'tools/reports/new_upload_key.py')
        maker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(maker)
        with tempfile.TemporaryDirectory() as tmp:
            path = maker.make(Path(tmp) / 'k.json')
            self.assertEqual(oct(path.stat().st_mode & 0o777), '0o600')
            key = json.loads(path.read_text())['upload_key']
            self.assertRegex(key, r'^[A-Za-z0-9_-]{43}$')
            with self.assertRaises(SystemExit):
                maker.make(path)
            maker.make(path, replace=True)
            self.assertNotEqual(json.loads(path.read_text())['upload_key'], key)


if __name__ == '__main__':
    unittest.main()
