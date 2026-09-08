import dataclasses
import unittest
from tools.validation.d08 import Inputs, validate, u32

# Synthetic layout: not a claim about an actual compiled kernel.
GOOD = Inputs(0x200000, 0x800, 0x1000, 0x500000, 0x100000, 0x600000,
              0x100, 0x400, 0x200000, 0x600, 0x204000, 0x200000, 0x201000, 0x2000)


class Layout(unittest.TestCase):
    def test_bounded_layout(self):
        r = validate(GOOD)
        self.assertEqual(r['initramfs'], [0x84000000, 0x84001000])
        self.assertEqual(r['ram'], ((0x80000000, 0x81800000), (0x84000000, 0x84080000)))
        self.assertLess(r['workspace_end'], 0x81800000)
        self.assertGreaterEqual(r['relocated_dtb'][0], r['resident_kernel'][1])

    def test_independent_rejections(self):
        cases = [dict(z=0x600000, edata=0x600000), dict(dtb=0x10001),
                 dict(initramfs=0x80001), dict(image=0xc00001),
                 dict(kernel_bss=0x200001), dict(kernel_span=0xe00001),
                 dict(regular_file_bytes=0x200001), dict(z=0x200001),
                 dict(edata=0x1ffff8), dict(restart=0), dict(reloc_code_end=0x20000),
                 dict(stack_top=0x250000), dict(compressed_bss_end=0x205000),
                 dict(kernel_span=0x800000), dict(image=0x300, kernel_bss=1, kernel_span=0x301),
                 dict(z=-1), dict(z=2**32)]
        for change in cases:
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate(dataclasses.replace(GOOD, **change))

    def test_checked_arithmetic(self):
        for n in [-1, 2**32, True, 3.5]:
            with self.subTest(n=n), self.assertRaises(ValueError):
                u32(n)

    def test_small_adjustment_preserves_dtb(self):
        r = validate(dataclasses.replace(GOOD, z=0x80000, edata=0x80000,
                                        compressed_bss_start=0x80000,
                                        compressed_bss_end=0x81000, stack_top=0x84000))
        self.assertGreater(r['adjusted_image_size'], GOOD.image)
        self.assertGreaterEqual(r['relocated_dtb'][0], r['resident_kernel'][1])

if __name__ == '__main__':
    unittest.main()
