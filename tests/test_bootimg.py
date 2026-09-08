import unittest
from tools.build.package import pack
from tools.validation.bootimg import check
from tools.validation.d08 import up

class Bootimg(unittest.TestCase):
    def test_roundtrip_and_corruptions(self):
        kernel, ramdisk = b'kernel fixture'*40, b'ramdisk fixture'*20
        raw = pack(kernel,ramdisk)
        extent = 2048+up(len(kernel)+512,2048)+up(len(ramdisk)+512,2048)+4096
        layout = {'bootimg_read_end':extent}
        self.assertEqual(check(raw,kernel,ramdisk,layout)['bytes'],extent)
        for offset in [0,8,12,16,20,24,28,32,36,40,44,48,64,576,600,2048,2052,2056,2088,2560,len(raw)-1]:
            damaged = bytearray(raw); damaged[offset] ^= 1
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                check(damaged,kernel,ramdisk,layout)
        for damaged in [raw[:-2048],raw+bytes(2048)]:
            with self.assertRaises(ValueError): check(damaged,kernel,ramdisk,layout)

if __name__ == '__main__': unittest.main()
