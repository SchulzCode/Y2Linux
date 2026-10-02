import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('campaign', ROOT/'tools/development/qualify-feature-completion.py')
h = importlib.util.module_from_spec(spec);spec.loader.exec_module(h)


class Qualification(unittest.TestCase):
    def campaign(self, directory):
        package = Path(directory)/'package';package.mkdir()
        manifest = dict(build_git_commit='a'*40, reborn_source_commit='b'*40, kernel_version='kernel', rootfs_version='root')
        (package/'manifest.json').write_text(json.dumps(manifest))
        return h.Campaign(argparse.Namespace(package=package, output=Path(directory)/'run',
                                             host='usb', wifi_host='wifi', sleep=False))

    def test_legacy_mutating_operation_is_never_replayed_on_wifi(self):
        with tempfile.TemporaryDirectory() as directory:
            campaign = self.campaign(directory)
            attempts = []
            def fail(host, code, timeout):
                attempts.append(host)
                raise h.subprocess.TimeoutExpired('ssh', timeout)
            with patch.object(campaign.platform,'live_host',return_value='usb'), patch.object(campaign.platform,'ssh',side_effect=fail):
                with self.assertRaises(RuntimeError): campaign.platform.remote('write("fixture","value")')
            self.assertEqual(attempts, ['usb'])

    def test_optional_poll_tolerates_both_transports_asleep(self):
        with tempfile.TemporaryDirectory() as directory:
            campaign = self.campaign(directory)
            with patch.object(campaign.platform, 'live_host', side_effect=RuntimeError('asleep')):
                self.assertIsNone(campaign.platform.remote('read("fixture")', required=False))
                with self.assertRaises(RuntimeError):
                    campaign.platform.remote('write("fixture","value")')

    def test_identity_taint_and_boot_change_stop_exercise(self):
        with tempfile.TemporaryDirectory() as directory:
            campaign = self.campaign(directory)
            sample = {'record':dict(y2linux_commit='a'*40,reborn_commit='b'*40,kernel='kernel',rootfs_release='root',boot_id='boot1'),
                      'system':{'kernel_taint':0}}
            with patch.object(campaign, 'command', return_value=sample):
                campaign.identity()
                sample['system']['kernel_taint'] = 1
                with self.assertRaisesRegex(ValueError,'taint'):campaign.identity()
                sample['system']['kernel_taint'] = 0
                sample['record']['boot_id'] = 'boot2'
                with self.assertRaisesRegex(ValueError,'boot_change'):campaign.identity()


if __name__ == '__main__': unittest.main()
