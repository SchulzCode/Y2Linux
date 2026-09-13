"""Adversarial coverage of the active production MMC request policy."""
import ctypes, subprocess, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class Request(ctypes.Structure):
    _fields_=[(n,ctypes.c_uint) for n in ('opcode','arg','direction','blocks','blksz','has_sbc','sbc_opcode','sbc_arg','has_stop','stop_opcode','stop_arg','user_area','part_config','identified')]
class ProductionMMC(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory();p=Path(cls.tmp.name)
        (p/'test.c').write_text('#include "storage-policy.h"\nint allow(const struct y2_emmc_request *r){return y2_emmc_request_allowed(r);}\n')
        subprocess.run(['cc','-Wall','-Wextra','-Werror','-shared','-fPIC','-I'+str(ROOT/'kernel/platform'),str(p/'test.c'),'-o',str(p/'test.so')],check=True)
        cls.lib=ctypes.CDLL(str(p/'test.so'));cls.lib.allow.argtypes=[ctypes.POINTER(Request)]
    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()
    def allowed(self,**kwargs):return bool(self.lib.allow(ctypes.byref(Request(**kwargs))))
    def io(self,write=False,**changes):
        d=dict(opcode=25 if write else 18,arg=166912,direction=256 if write else 512,blocks=8,blksz=512,user_area=1,identified=1)
        d.update(changes);return self.allowed(**d)
    def test_read_write_and_companions(self):
        for write in (False,True):
            self.assertTrue(self.io(write))
            self.assertTrue(self.io(write,opcode=24 if write else 17,blocks=1))
            self.assertFalse(self.io(write,opcode=24 if write else 17,blocks=2))
            self.assertTrue(self.io(write,has_sbc=1,sbc_opcode=23,sbc_arg=8,has_stop=1,stop_opcode=12))
            for arg in (7,9,0x40000008,0xffffffff):self.assertFalse(self.io(write,has_sbc=1,sbc_opcode=23,sbc_arg=arg))
            for arg in (0x80000008,0x20000008,0xa0000008):self.assertEqual(self.io(write,has_sbc=1,sbc_opcode=23,sbc_arg=arg),write)
            self.assertFalse(self.io(write,has_stop=1,stop_opcode=38))
            self.assertFalse(self.io(write,has_stop=1,stop_opcode=12,stop_arg=1))
            for direction in (0,768):self.assertFalse(self.io(write,direction=direction))
            self.assertFalse(self.io(write,blksz=1024))
            self.assertFalse(self.io(write,user_area=0))
    def test_all_protected_boundaries_and_overflow(self):
        for start,end in ((166912,1846272),(2104320,3742720)):
            for blocks in (1,8,128,65535,end-start):
                self.assertTrue(self.io(True,arg=start,blocks=blocks))
                self.assertTrue(self.io(True,arg=end-blocks,blocks=blocks))
                self.assertFalse(self.io(True,arg=start-1,blocks=blocks))
                self.assertFalse(self.io(True,arg=end-blocks+1,blocks=blocks))
        for sector in (0,1024,18432,38912,125952,145408,1846272,3742720):
            self.assertTrue(self.io(arg=sector));self.assertFalse(self.io(True,arg=sector))
        for sector,count in ((0xffffffff,2),(166912,0xffffffff),(15269888,1),(0,0)):
            self.assertFalse(self.io(arg=sector,blocks=count));self.assertFalse(self.io(True,arg=sector,blocks=count))
    def test_switch_fields_and_hardware_areas(self):
        for index in range(256):
            for value in (0,1,2,3,0x40,0x48,0xff):
                allowed={32:{1},33:{0,1},34:{1,2,3},161:{0,1},175:{1},183:{0},185:{0}}.get(index,set())
                self.assertEqual(self.allowed(opcode=6,arg=0x03000001|(index<<16)|(value<<8)),value in allowed,(index,value))
        for before in range(256):
            self.assertTrue(self.allowed(opcode=6,arg=0x03b30001|((before&~7)<<8),part_config=before,identified=1))
            for region in range(1,8):
                self.assertFalse(self.allowed(opcode=6,arg=0x03b30001|(((before&~7)|region)<<8),part_config=before,identified=1))
        self.assertFalse(self.allowed(opcode=6,arg=0x03b30001,identified=0))
    def test_standard_initialization_flush_shutdown(self):
        self.assertTrue(self.allowed(opcode=8,direction=512,blocks=1,blksz=512))
        self.assertFalse(self.allowed(opcode=8,direction=256,blocks=1,blksz=512))
        for op in (0,1,2,3,7,9,10,12,13):self.assertTrue(self.allowed(opcode=op))
        self.assertTrue(self.allowed(opcode=16,arg=512))
        self.assertTrue(self.allowed(opcode=5,user_area=1))
        self.assertTrue(self.allowed(opcode=6,arg=0x03200101)) # FLUSH_CACHE used by block fsync
        self.assertFalse(self.allowed(opcode=0,arg=0xf0f0f0f0))
        for op in (23,24,25,26,27,28,29,35,36,38,42,56,60):self.assertFalse(self.allowed(opcode=op,user_area=1))
