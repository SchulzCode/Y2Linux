import hashlib
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.production import application, ffmpeg9
from tools.production import validate as production_validate


class RebornReceipts(unittest.TestCase):
    def test_reborn_source_version_comes_from_workspace_package(self):
        self.assertEqual(
            application.source_version((ffmpeg9.REBORN / 'Cargo.toml').read_text()),
            '0.1.0',
        )

    def test_retained_reborn_version_comes_from_recorded_source_commit(self):
        commit = 'a' * 40
        with patch.object(application.subprocess, 'check_output', return_value='[workspace.package]\nversion = "0.4.2"\n') as show:
            version = application.source_version_at_commit('/repo/Y2Reborn', commit)
        self.assertEqual(version, '0.4.2')
        show.assert_called_once_with(['git', 'show', f'{commit}:Cargo.toml'], cwd=Path('/repo/Y2Reborn'), text=True)

    def test_retained_reborn_version_rejects_unverifiable_source_identity(self):
        with self.assertRaisesRegex(ValueError, 'missing or malformed'):
            application.source_version_at_commit('/repo/Y2Reborn', None)

    def test_application_receipt_describes_installed_reborn_files(self):
        manifest = {
            'reborn_version': '0.1.0',
            'application': application.receipt('0.1.0'),
        }
        self.assertTrue(application.validate(manifest))
        self.assertEqual(manifest['application']['binary'], '/usr/bin/reborn')
        self.assertEqual(manifest['application']['control_binary'], '/usr/bin/rebornctl')
        self.assertEqual(manifest['application']['state_root'], '/data/reborn')
        self.assertNotIn('implemented', manifest['application'])
        self.assertNotIn('builtin_path', manifest['application'])

    def test_current_receipt_rejects_legacy_y2player_assertion(self):
        manifest = {
            'reborn_version': '0.1.0',
            'application': {
                'name': 'Y2PlayerNative',
                'version': None,
                'implemented': False,
                'builtin_path': '/usr/bin/y2player',
                'partition': None,
            },
        }
        with self.assertRaises(ValueError):
            application.validate(manifest)

    def test_unversioned_old_receipt_is_classified_as_historical(self):
        self.assertFalse(application.validate({
            'application': {'name': 'Y2PlayerNative', 'implemented': False}
        }))

    def test_root_image_validator_checks_reborn_and_absence_of_legacy_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'Y2ROOT.img'
            root.touch()
            responses = {
                'stat /usr/bin/reborn': b'Inode: 1 Type: regular\n',
                'stat /usr/bin/rebornctl': b'Inode: 2 Type: regular\n',
                'stat /usr/lib/reborn/libreborn_media.so': b'Inode: 3 Type: regular\n',
                'stat /usr/bin/y2player': b'/usr/bin/y2player: File not found by ext2_lookup\n',
            }
            with patch.object(production_validate, 'run_debugfs', side_effect=lambda _root, command: responses[command]):
                production_validate.require_reborn_root_image(Path(directory))

    def test_root_image_validator_rejects_legacy_y2player_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'Y2ROOT.img'
            root.touch()
            responses = {
                'stat /usr/bin/reborn': b'Inode: 1 Type: regular\n',
                'stat /usr/bin/rebornctl': b'Inode: 2 Type: regular\n',
                'stat /usr/lib/reborn/libreborn_media.so': b'Inode: 3 Type: regular\n',
                'stat /usr/bin/y2player': b'Inode: 4 Type: regular\n',
            }
            with patch.object(production_validate, 'run_debugfs', side_effect=lambda _root, command: responses[command]):
                with self.assertRaisesRegex(ValueError, 'obsolete Y2PlayerNative'):
                    production_validate.require_reborn_root_image(Path(directory))

    def test_ffmpeg_archive_acquisition_verifies_then_atomically_installs(self):
        payload = b'pinned archive bytes'
        digest = hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(ffmpeg9, 'ARCHIVE_SHA256', digest), patch.object(
                ffmpeg9, 'urlopen', return_value=io.BytesIO(payload)
            ) as open_url:
                archive = ffmpeg9.ensure_archive(Path(directory))
            self.assertEqual(archive.read_bytes(), payload)
            open_url.assert_called_once_with(ffmpeg9.ARCHIVE_URL, timeout=120)
            self.assertFalse(list(archive.parent.glob('*.part-*')))

    def test_bad_ffmpeg_download_leaves_no_archive_or_partial_file(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(ffmpeg9, 'ARCHIVE_SHA256', '0' * 64), patch.object(
                ffmpeg9, 'urlopen', return_value=io.BytesIO(b'wrong archive')
            ):
                with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                    ffmpeg9.ensure_archive(Path(directory))
            self.assertFalse(list(Path(directory).rglob('*.tar.xz')))
            self.assertFalse(list(Path(directory).rglob('*.part-*')))


if __name__ == '__main__':
    unittest.main()
