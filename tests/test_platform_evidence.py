"""Real readonly ALSA query, precision bytes and bounded previous-boot retention."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import wave
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools/platform'))
from audio_precision import generate
from y2_platform.common import Context
from y2_platform.boot import capture_previous, evidence_status, mark


class EvidenceTests(unittest.TestCase):
    def test_precision_files_preserve_low_bits_and_independent_channels(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)/'fixtures'
            manifest = generate(path)
            raw = (path/'reference-s32le.raw').read_bytes()
            low = (path/'reference-s24le.raw').read_bytes()
            words = struct.unpack('<'+'i'*(len(raw)//4), raw)
            self.assertEqual(words, tuple(v*256 for v in struct.unpack('<'+'i'*(len(low)//4), low)))
            self.assertTrue(any(v & 0xffff for v in words))
            self.assertNotEqual(words[::2], words[1::2])
            for rate in (44100,48000,88200,96000):
                with wave.open(str(path/f'precision-s24-{rate}.wav')) as stream:
                    self.assertEqual((stream.getsampwidth(),stream.getnchannels(),stream.getframerate()), (3,2,rate))
                    packed = stream.readframes(stream.getnframes())
                self.assertEqual(tuple(int.from_bytes(packed[i:i+3],'little',signed=True)*256
                                       for i in range(0,len(packed),3)), words)
            self.assertFalse(manifest['physical_playback'])
            with self.assertRaises(FileExistsError): generate(path)

    def test_actual_alsa_null_constraints_do_not_claim_hardware(self):
        if not shutil.which('pkg-config') or subprocess.run(['pkg-config','--exists','alsa']).returncode:
            self.skipTest('native ALSA development dependency; separate host/ARM query receipt required')
        with tempfile.TemporaryDirectory() as root:
            executable = Path(root)/'query'
            flags = subprocess.check_output(['pkg-config','--cflags','--libs','alsa'],text=True).split()
            subprocess.run(['cc','-Wall','-Wextra','-Werror',str(ROOT/'tools/connectivity/audio-contract.c'),
                            *flags,'-o',str(executable)],check=True)
            result = json.loads(subprocess.check_output([str(executable),'--device','null'],timeout=3))
            self.assertEqual(len(result['combinations']),12)
            self.assertTrue(all(c['accepted_constraints'] for c in result['combinations']))
            self.assertFalse(result['physical_qualification'])
            self.assertFalse(result['pcm_started'])
            self.assertEqual(subprocess.run([str(executable),'--device','default'],capture_output=True).returncode,2)

    def test_previous_boot_logs_are_private_bounded_once_only_and_not_reset_diagnosis(self):
        with tempfile.TemporaryDirectory() as root:
            ctx = Context(root)
            def put(name,value):
                path=ctx.path(name);path.parent.mkdir(parents=True,exist_ok=True);path.write_text(value);return path
            put('/data/.y2data-schema','1')
            put('/proc/self/mountinfo','3 1 179:2 / /data rw - ext4 /dev/data rw\n')
            put('/run/y2-data-device','/dev/data')
            put('/proc/sys/kernel/random/boot_id','new-boot')
            put('/data/system/platform/boot.json',json.dumps({'schema':1,'boot_id':'previous','last_stage':'application_ready','orderly_shutdown':False}))
            log=put('/data/logs/system.log', ('private previous message '+'x'*600+'\n')*1000)
            result=capture_previous(ctx)
            self.assertEqual(result['previous_boot_id'],'previous')
            self.assertIsNone(result['reset_cause'])
            self.assertFalse(result['kernel_panic_retention_guaranteed'])
            self.assertLessEqual(len(result['logs'][0]['tail_lines']),32)
            path=ctx.path('/data/system/platform/previous-boot-evidence.json')
            self.assertEqual(path.stat().st_mode&0o777,0o600)
            self.assertLessEqual(path.stat().st_size,96000)
            self.assertNotIn('private previous message',json.dumps(evidence_status(ctx)))
            log.write_text('new boot message')
            self.assertEqual(capture_previous(ctx),result)
            mark(ctx,'platform_start')
            path.unlink()
            with self.assertRaisesRegex(ValueError,'missed_early_boundary'): capture_previous(ctx)
            put('/proc/sys/kernel/random/boot_id','next-boot')
            log.unlink();log.symlink_to('/etc/passwd')
            with self.assertRaises(OSError): capture_previous(ctx)


if __name__ == '__main__': unittest.main()
