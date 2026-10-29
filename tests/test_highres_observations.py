"""Check discriminating fixtures against retained physical DMA/codec evidence."""
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import unittest
import wave

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/host'))
from audio_precision import generate_live


class WiredPrecisionObservations(unittest.TestCase):
    def test_live_fixtures_distinguish_low_bits_and_pack_signed_channels(self):
        with tempfile.TemporaryDirectory() as tmp:
            rows = generate_live(Path(tmp) / 'fixtures', 1)
            self.assertEqual(len(rows), 6)
            for row in rows:
                with wave.open(str(Path(tmp) / 'fixtures' / row['file']), 'rb') as stream:
                    width = stream.getsampwidth()
                    self.assertEqual(stream.getnchannels(), 2)
                    self.assertEqual(stream.getnframes(), row['rate'])
                    data = stream.readframes(8)
                values = [int.from_bytes(data[i:i+width], 'little', signed=True)
                          for i in range(0, len(data), width)]
                left, right = values[::2], values[1::2]
                self.assertTrue(all(x > 0 for x in left[:4]))
                self.assertTrue(all(x < 0 for x in right[:4]))
                self.assertEqual(right, left[4:] + left[:4])
                self.assertLess(row['max_normalized_amplitude'], 1 / 1024)
                self.assertEqual(len(set(left[:4])), 4)
                if row['bits'] > 16:
                    self.assertEqual(len({x >> 8 for x in left[:4]}), 1)
            with self.assertRaises(ValueError):
                generate_live(Path(tmp) / 'unsafe', 0)

    def test_live_fixture_cli_succeeds_without_playing(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run([sys.executable, str(ROOT / 'tools/host/audio_precision.py'),
                                     str(Path(tmp) / 'fixtures'), '--live-safe',
                                     '--duration-seconds', '1'], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(json.loads(result.stdout)), 6)

    def test_physical_dma_equals_final_membrane_and_dac_is_16_bit(self):
        evidence = json.loads((ROOT / 'docs/validation/evidence/highres-audio-completion/physical.json').read_text())
        self.assertFalse(evidence['physical_serial_capture'])
        self.assertTrue(evidence['diagnostic_modules_removed'])
        self.assertEqual(len(evidence['reborn_tests']), 6)
        for row in evidence['reborn_tests']:
            self.assertEqual(row['source_precision_reported'], row['source_bits'])
            self.assertEqual(row['alsa']['format'], 'S16_LE')
            self.assertEqual((row['alsa']['channels'], row['alsa']['period'], row['alsa']['buffer']),
                             (2, 512, 4096))
            ring = bytes.fromhex(re.search(r'dma_hex=([0-9a-f]+)', row['observer'])[1])
            self.assertEqual(ring, bytes.fromhex(row['final_sink_probe_hex']))
            self.assertEqual(len(ring), 256)
            regs = {name: int(value, 16) for name, value in
                    re.findall(r'^(\w+)=(0x[0-9a-f]+)$', row['observer'], re.M)}
            self.assertEqual(regs['SP_BITSIZE'] & 3, 2)  # ASP 16-bit receiver
            self.assertEqual(regs['ASP_CH_1_SZ_EN'] & 3, 1)
            self.assertEqual(regs['ASP_CH_2_SZ_EN'] & 3, 1)
            self.assertEqual(regs['AFE_CONN0'], (1 << 5) | (1 << 22))
            self.assertEqual(regs['AFE_I2S_CON3'] & 0xb, 0xb)
            self.assertEqual(regs['ASP_LRCK_PERIOD_1'], 63)
            self.assertTrue(all(value == 0 for value in row['errors'].values()))
            words = struct.unpack('<128h', ring)
            if row['source_bits'] > 16:
                self.assertEqual(len(set(words[0:8:2])), 1)

    def test_neutral_software_24_bit_outputs_keep_exact_signed_low_bits(self):
        evidence = json.loads((ROOT / 'docs/validation/evidence/highres-audio-completion/physical.json').read_text())
        expected = [4608, -4864, 4609, -4863, 4735, -4737, 4863, -4609,
                    -4864, 4608, -4863, 4609, -4737, 4735, -4609, 4863]
        for row in evidence['software_24bit_probes']:
            data = bytes.fromhex(row['probe_hex'])
            words = list(struct.unpack('<64i', data))
            if row['sink_format'] == 'S32_LE':
                self.assertTrue(all(word & 255 == 0 for word in words))
                words = [word >> 8 for word in words]
            else:
                words = [(word & 0xffffff) - (1 << 24) if word & 0x800000
                         else word & 0xffffff for word in words]
            self.assertEqual(words[:16], expected)
            self.assertFalse(row['physical_output'])


if __name__ == '__main__':
    unittest.main()
