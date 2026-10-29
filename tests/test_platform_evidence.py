"""Real readonly ALSA query and precision bytes. The previous-boot evidence tests are Rust:
platform/crates/y2-platform/src/commands/observation/evidence_tests.rs."""
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import wave
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools/host'))  # audio_precision.py, a host tool
from audio_precision import generate


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
            self.assertEqual(len(result['combinations']),16)
            self.assertTrue(all(c['accepted_constraints'] for c in result['combinations']))
            self.assertFalse(result['physical_qualification'])
            self.assertFalse(result['pcm_started'])
            self.assertFalse(result['constraint_bits_are_hardware_precision'])
            self.assertTrue(all(c['meaningful_bits'] is None for c in result['combinations']))
            self.assertEqual({c['format'] for c in result['combinations']}, {'S16_LE', 'S24_LE', 'S32_LE', 'S24_3LE'})
            self.assertEqual(subprocess.run([str(executable),'--device','default'],capture_output=True).returncode,2)


if __name__ == '__main__': unittest.main()
