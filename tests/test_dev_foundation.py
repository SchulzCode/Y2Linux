"""DEV memory/rescue/DT fault checks using actual emitted artifacts."""
import json, os, struct, subprocess, tempfile, unittest
from pathlib import Path
from tools.validation.d08 import Inputs
from tools.validation.dev_memory import check_layout
from tools.validation.dev_dtb import check as check_dtb
from tools.validation.dev_artifacts import rescue_entries
from tools.validation.formats import gunzip
ROOT=Path(__file__).resolve().parents[1]
BUILD=Path(os.environ.get('Y2_ARTIFACT_TEST_ROOT',ROOT/'out/y2linux-dev-02'))
class DevFoundation(unittest.TestCase):
    def test_large_bank_and_packaging_limits(self):
        x=json.loads((BUILD/'layout.json').read_text())['inputs']
        result=check_layout(Inputs(**x))
        self.assertEqual(result['allocator_before_kernel_bytes'],951*1024*1024-16384)
        for key,value in [('initramfs',0x400001),('regular_file_bytes',0x800001),('z',0x600008),('kernel_span',0xe00001)]:
            with self.subTest(key=key), self.assertRaises(ValueError):check_layout(Inputs(**(x|{key:value})))
    def test_dt_protects_emmc_and_reservations(self):
        data=(BUILD/'y2.dtb').read_bytes();size=(BUILD/'initramfs.cpio.gz').stat().st_size
        check_dtb(data,size)
        for old,new in [(b'disabled\0',b'okay\0\0\0\0\0'),
                        (struct.pack('>II',0x81800000,0x2800000),struct.pack('>II',0x81800000,0x1800000)),
                        (struct.pack('>II',0xbdf00000,0x2100000),struct.pack('>II',0xbe000000,0x2000000)),
                        (struct.pack('>I',13000000),struct.pack('>I',52000000)),
                        (b'innioasis,y2-msdc-sd\0',b'innioasis,y2-msdc-xx\0')]:
            self.assertIn(old,data)
            with self.subTest(old=old), self.assertRaises((ValueError,KeyError)):check_dtb(data.replace(old,new),size)
    def test_rescue_archive_and_bad_data(self):
        rd=(BUILD/'initramfs.cpio.gz').read_bytes();raw=gunzip(rd,0x810000);entries=rescue_entries(raw)
        self.assertEqual(entries['init'][1],(ROOT/'initramfs/rescue/init').read_bytes())
        for path in ('sbin/blkid','bin/busybox','lib/ld-linux-armhf.so.3','sbin/y2-observer','sbin/y2-fbtest','sbin/y2-abi-check'):self.assertIn(path,entries)
        for bad in (raw[:-512],raw.replace(b'bin/busybox\0',b'../busyboxx\0',1)):
            with self.assertRaises((ValueError,UnicodeDecodeError)):rescue_entries(bad)
    def test_rescue_only_selects_one_removable_ext4_label(self):
        script=(ROOT/'initramfs/rescue/init').read_text()
        selection=script[script.index('rootdev='):script.index('if [ "$abi_ok" = 1 ]')]
        with tempfile.TemporaryDirectory() as tmp:
            t=Path(tmp);(t/'block').mkdir();(t/'bin').mkdir();(t/'dev').mkdir()
            blkid=t/'bin/blkid';blkid.write_text('#!/bin/sh\ncat "$1.info"\n');blkid.chmod(0o755)
            def disk(name,host,info):
                real=t/'devices'/host/name;real.mkdir(parents=True);(t/'block'/name).symlink_to(real)
                (t/'dev'/f'{name}.info').write_text(info)
            disk('mmcblk0','11230000.mmc','LABEL="Y2ROOT" TYPE="ext4"')
            script=selection.replace('/sys/class/block/*',str(t/'block')+'/*').replace('dev=/dev/','dev='+str(t/'dev')+'/')
            script+='\nprintf "%s|%s" "$rootdev" "$ambiguous"\n'
            def run():return subprocess.check_output(['sh','-c',script],env=dict(os.environ,PATH=str(t/'bin')+':/usr/bin:/bin'),text=True)
            self.assertEqual(run(),'|0')
            disk('mmcblk1p1','11240000.mmc','LABEL="Y2ROOT" TYPE="vfat"');self.assertEqual(run(),'|0')
            (t/'dev/mmcblk1p1.info').write_text('LABEL="Y2ROOT" TYPE="ext4"')
            self.assertEqual(run(),str(t/'dev/mmcblk1p1')+'|0')
            disk('mmcblk1p2','11240000.mmc','LABEL="Y2ROOT" TYPE="ext4"')
            self.assertTrue(run().endswith('|1'))
