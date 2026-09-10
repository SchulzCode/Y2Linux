"""Reject the actual old ABI configuration and exercise failed preflight isolation."""
import os,subprocess,tempfile,unittest
from pathlib import Path
from tools.build.check_config import parse
from tools.validation.dev_artifacts import userspace_config
ROOT=Path(__file__).resolve().parents[1]
BUILD=Path(os.environ.get('Y2_ARTIFACT_TEST_ROOT',ROOT/'out/y2linux-dev-02'))
class UserspaceABI(unittest.TestCase):
    def test_required_abi_and_framebuffer_support(self):
        config=parse(BUILD/'kernel/.config');userspace_config(config)
        for flag in ('CONFIG_ARM_THUMB','CONFIG_KUSER_HELPERS','CONFIG_FB_DEVICE','CONFIG_DEBUG_USER'):
            with self.subTest(flag=flag), self.assertRaisesRegex(ValueError,flag):userspace_config(config|{flag:'n'})
        old=ROOT/'out/y2linux-dev-01/kernel/.config'
        if old.exists():
            with self.assertRaisesRegex(ValueError,'CONFIG_ARM_THUMB'):userspace_config(parse(old))
    def test_failed_child_keeps_root_handoff_disabled(self):
        source=(ROOT/'initramfs/rescue/init').read_text()
        preflight=source[source.index('abi_ok=0'):source.index('rootdev=')]
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);probe=d/'probe';log=d/'kmsg'
            for action,expected in [('exit 0','1'),('exit 1','0'),('ulimit -c 0; kill -ILL $$','0')]:
                probe.write_text('#!/bin/sh\n'+action+'\n');probe.chmod(0o755)
                script=preflight.replace('/sbin/y2-abi-check',str(probe)).replace('/dev/kmsg',str(log))+'\nprintf "%s" "$abi_ok"\n'
                r=subprocess.run(['sh','-c',script],text=True,capture_output=True)
                self.assertEqual(r.returncode,0);self.assertEqual(r.stdout,expected)
                if expected=='0':self.assertIn('preserving rescue',log.read_text())
