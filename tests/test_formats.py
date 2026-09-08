import gzip
import unittest
from tools.build.initramfs import archive
from tools.validation.formats import cpio, gunzip

class Formats(unittest.TestCase):
    def test_roundtrip_and_bounds(self):
        raw = archive(b'bounded init fixture')
        packed = gzip.compress(raw, mtime=0)
        self.assertEqual(cpio(gunzip(packed, 0x210000)), b'bounded init fixture')
        for bad, cap in [(packed[:-1], 0x210000), (packed + packed, 0x210000), (packed, 10)]:
            with self.subTest(cap=cap), self.assertRaises(ValueError): gunzip(bad, cap)

    def test_cpio_rejects_changes(self):
        raw = archive(b'bounded init fixture')
        for bad in [raw.replace(b'dev/console', b'dev/consolX'), raw[:-512],
                    raw + bytes(512), raw.replace(b'000081ed', b'0000a1ed')]:
            with self.assertRaises(ValueError): cpio(bad)

if __name__ == '__main__': unittest.main()
