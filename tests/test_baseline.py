"""Exercise shared M2 safety decisions and the actual DT contract offline."""
from pathlib import Path
import os
import struct
import subprocess
import tempfile
import unittest

PROJECT=Path(__file__).resolve().parents[1]

class SharedPolicy(unittest.TestCase):
    def test_clock_decode_and_write_firewalls(self):
        source=r'''
#include <assert.h>
#include "policy.h"
int main(void) {
    assert(y2_pll_decode(0, 0x134000, 0)==0);
    assert(y2_pll_decode(1, 0x134000, 0)==2002000000U);
    assert(y2_pll_decode(1, 0x99fff, 0)==1000998413U);
    assert(y2_pll_decode(0x11, 0x134000, 0)==1001000000U);
    assert(y2_pll_decode(0x21, 0x134000, 0)==500500000U);
    assert(y2_pll_decode(0x31, 0x134000, 0)==500500000U);
    assert(y2_pll_decode(1, 0x01134000, 1)==1001000000U);
    assert(y2_pll_decode(0x80001, 0x1fffff, 0)==0); /* overflow is unknown */
    for(unsigned r=0;r<65536;++r) {
        unsigned mask=y2_pmic_write_mask(r);
        if(r==0x160||r==0x166||r==0x172||r==0x174) assert(mask==65535);
        else if(r==0x16c) assert(mask==16);
        else if(r==0x330||r==0x338||r==0x340||r==0x348) assert(mask==0x1f00);
        else if(r==0x356) assert(mask==15);
        else if(r==0x50c) assert(mask==0x8000);
        else if(r==0x532) assert(mask==0x00e0);
        else assert(mask==0); /* every rail/charger/reset and alias is blocked */
    }
    for(unsigned op=0;op<64;++op) for(unsigned embedded=0;embedded<2;++embedded) {
        assert(!y2_mmc_read_allowed(op,embedded,1,0));
        assert(!y2_mmc_read_allowed(op,embedded,0,1));
        if(op==6) assert(y2_mmc_read_allowed(op,embedded,0,0)==!embedded);
        if(op==24||op==25||op==28||op==29||op==35||op==36||op==38||op==42||op==56)
            assert(!y2_mmc_read_allowed(op,embedded,0,0));
    }
    assert(y2_mmc_read_allowed(8,1,0,0)); /* read-only eMMC EXT_CSD */
    assert(y2_mmc_read_allowed(51,0,0,0)); /* SD SCR */
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'test.c').write_text(source)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',
                            '-I'+str(PROJECT/'kernel/platform'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'),'needs actual build')
    def test_actual_dt_rejects_unsafe_integration(self):
        from tools.validation.baseline_dtb import check
        root=Path(os.environ['Y2_ARTIFACT_TEST_ROOT'])
        data=(root/'y2.dtb').read_bytes();size=(root/'initramfs.cpio.gz').stat().st_size
        check(data,size)
        changes=[(struct.pack('>II',400000,0),struct.pack('>II',50000000,0)),
                 (b'innioasis,y2-msdc-readonly\0',b'mediatek,mt8135-mmc'+b'\0'*8),
                 (struct.pack('>II',25,4),struct.pack('>II',25,8)),
                 (struct.pack('>II',55,2),struct.pack('>II',55,1)),
                 (b'innioasis,lk-powered\0',b'innioasis,xx-powered\0')]
        # Scalar frequency is tested independently of neighbouring FDT tokens.
        changes[0]=(struct.pack('>I',400000),struct.pack('>I',50000000))
        for old,new in changes:
            with self.subTest(old=old):
                self.assertIn(old,data)
                with self.assertRaises((ValueError,KeyError,struct.error)):
                    check(data.replace(old,new,1),size)
