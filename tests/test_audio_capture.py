"""I2S capture analysis rejects a hidden S16 conversion and wrong LRCLK."""
from pathlib import Path
import csv
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from audio_precision import PATTERN, verify_capture


class Capture(unittest.TestCase):
    def test_exact_low_bits_at_all_supported_fixture_rates(self):
        for rate in (44100, 48000, 88200, 96000):
            for truncate, clock_factor in ((False, 1), (True, 1), (False, 2)):
                with tempfile.TemporaryDirectory() as directory:
                    path = Path(directory) / 'capture.csv'
                    with path.open('w', newline='') as stream:
                        writer = csv.writer(stream)
                        writer.writerow(('time_s', 'left_s24', 'right_s24'))
                        for frame in range(128):
                            values = [PATTERN[frame % 16], PATTERN[(frame + 5) % 16]]
                            if truncate: values = [(value >> 8) << 8 for value in values]
                            writer.writerow((frame / (rate * clock_factor), *values))
                    value = verify_capture(path, rate)
                    self.assertEqual(value['state'], 'MISMATCH' if truncate or clock_factor != 1 else 'MATCH')
                    self.assertFalse(value['physical_qualification'])
