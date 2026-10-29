"""RELEASE-49: radio firmware from the owner's own copy, never from an image.

The recovery's part (recovery.sh keeps the reviewed files of an old root). The
platform's part (`firmware`, the service's watcher, the reviewed tables) is tested
in platform/crates/y2-platform/src/commands/service/radiofw_tests.rs.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


class RecoveryKeepsFirmware(unittest.TestCase):
    """recovery.sh copies the reviewed files from the old root to Y2DATA."""
    def run_keep(self, files, existing=None):
        temp = Path(tempfile.mkdtemp(prefix='y2-keep-'))
        self.addCleanup(shutil.rmtree, temp, True)
        old = temp / 'oldroot/lib/firmware/mediatek/mt6582'
        old.mkdir(parents=True)
        for name, data in files.items():
            (old / name).write_bytes(data)
        (temp / 'newdata').mkdir()
        if existing:
            (temp / 'newdata/firmware/mt6582').mkdir(parents=True)
        script = (ROOT / 'initramfs/production/recovery.sh').read_text()
        start = script.index('RADIO_FIRMWARE=')
        end = script.index('charger_online() {')
        body = script[start:end]
        block = body.split("RADIO_FIRMWARE='", 1)[1].split("'", 1)[0]
        listed = '\n'.join('%s  %s' % (sha(d), n) for n, d in files.items()
                           if n in [l.split('  ')[1] for l in block.splitlines()])
        body = body.replace(block, listed if len(files) == 7 else block)
        body = (body.replace('/bin/busybox sha256sum -c -s -', 'sha256sum -c - >/dev/null 2>&1')
                    .replace('/bin/busybox rm', 'rm')
                    .replace('/newdata', str(temp / 'newdata')).replace('/oldroot', str(temp / 'oldroot')))
        shell = ('set -u\ny2_find_partition() { echo /dev/fixture; }\nmount() { :; }\numount() { :; }\n'
                 'recovery_log() { echo "LOG $*"; }\n' + body + 'recovery_keep_radio_firmware\n')
        result = subprocess.run(['sh', '-c', shell], capture_output=True, text=True, timeout=30)
        return temp, result

    def test_reviewed_files_move_to_y2data(self):
        names = ['WIFI_RAM_CODE_MT6582', 'WMT_SOC.cfg', 'mt6572_82_patch_e1_0_hdr.bin',
                 'mt6572_82_patch_e1_1_hdr.bin', 'modem_1_2g_n.img', 'WIFI.defaults', 'BT.defaults']
        files = {n: os.urandom(100) for n in names}
        temp, result = self.run_keep(files)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('LOG radio firmware kept on Y2DATA', result.stdout)
        kept = temp / 'newdata/firmware/mt6582'
        self.assertEqual({p.name: p.read_bytes() for p in kept.iterdir()}, files)
        self.assertFalse((temp / 'newdata/firmware/.mt6582.keep').exists())

    def test_files_not_matching_the_reviewed_hashes_are_left_alone(self):
        temp, result = self.run_keep({'WIFI_RAM_CODE_MT6582': b'other'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((temp / 'newdata/firmware/mt6582').exists())


if __name__ == '__main__':
    unittest.main()
