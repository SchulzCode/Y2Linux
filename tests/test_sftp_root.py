"""The owner's SFTP root (production overlay usr/libexec/sftp-server): Internal Storage when it is mounted
and its record says Ready, /data/music otherwise. The record is written compact ("state":"Ready")."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'buildroot/board/y2/production-overlay/usr/libexec/sftp-server'


class SftpRoot(unittest.TestCase):
    def root_for(self, record, mounted=True):
        with tempfile.TemporaryDirectory() as scratch:
            scratch = Path(scratch)
            (scratch / 'bin').mkdir()
            mountpoint = scratch / 'bin/mountpoint'
            mountpoint.write_text('#!/bin/sh\nexit %d\n' % (0 if mounted else 1))
            mountpoint.chmod(0o755)
            server = scratch / 'y2-sftp-server'
            server.write_text('#!/bin/sh\necho "$2"\n')
            server.chmod(0o755)
            path = scratch / 'internal-media.json'
            if record is not None:
                path.write_text(record)
            # The script's device paths, pointed at the scratch copies.
            text = SCRIPT.read_text().replace('/run/y2/internal-media.json', str(path)).replace(
                '/usr/libexec/y2-sftp-server', str(server))
            script = scratch / 'sftp-server'
            script.write_text(text)
            script.chmod(0o755)
            result = subprocess.run([str(script)], capture_output=True, text=True, check=True,
                                    env={**os.environ, 'PATH': '%s:/usr/bin:/bin' % (scratch / 'bin')})
            return result.stdout.strip()

    def test_compact_ready_record_roots_at_internal_storage(self):
        # atomic_json's separators: the form on the player (the grep wanted a space before).
        self.assertEqual(self.root_for('{"schema":1,"state":"Ready","mounted":true}'), '/media/internal/music')

    def test_spaced_ready_record_also_counts(self):
        self.assertEqual(self.root_for('{"state": "Ready"}'), '/media/internal/music')

    def test_other_states_and_missing_record_use_the_data_folder(self):
        self.assertEqual(self.root_for('{"state":"Unavailable"}'), '/data/music')
        self.assertEqual(self.root_for('{"state":"ReadyNot"}'), '/data/music')
        self.assertEqual(self.root_for(None), '/data/music')

    def test_unmounted_internal_storage_uses_the_data_folder(self):
        self.assertEqual(self.root_for('{"state":"Ready"}', mounted=False), '/data/music')


if __name__ == '__main__':
    unittest.main()
