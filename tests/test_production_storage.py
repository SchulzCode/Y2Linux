"""Adversarial tests for the production storage boundary, with no physical devices."""
import ctypes, json, os, shutil, subprocess, tempfile, unittest
from pathlib import Path
from tools.production.layout import TARGETS, make_scatter, scatter_rows, sparse_encode, sparse_identity
ROOT=Path(__file__).resolve().parents[1]

class StorageGuard(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory();p=Path(cls.tmp.name)
        (p/'guard.c').write_text('#include "'+str(ROOT/'kernel/platform/storage-policy.h')+'"\nint check(unsigned a,unsigned b,unsigned c,unsigned d,unsigned e,unsigned f,unsigned g) { return y2_emmc_command_allowed(a,b,c,d,e,f,g); }\n')
        subprocess.run([shutil.which('cc') or 'clang','-Wall','-Wextra','-Werror','-shared','-fPIC',str(p/'guard.c'),'-o',str(p/'guard.so')],check=True)
        cls.lib=ctypes.CDLL(str(p/'guard.so'));cls.fn=cls.lib.check
        cls.fn.argtypes=[ctypes.c_uint]*7;cls.fn.restype=ctypes.c_int
    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()
    def test_real_request_guard_completes_hsq_rejections(self):
        from tools.build.run import prepare_overlay
        spec=next(x for x in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays'] if x['path']=='drivers/mmc/host/mtk-sd.c')
        source=prepare_overlay(ROOT,spec).read_text()
        start=source.index('\tif (host->y2_production) {');end=source.index('\tif (IS_ENABLED(CONFIG_Y2_BOOT_DIAGNOSTIC) && host->y2_read_only)',start)
        fixture=r'''#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include "storage-policy.h"
#define MMC_DATA_WRITE 1
#define EXT_CSD_PART_CONFIG_ACC_MASK 7
#define EROFS 30
#define dev_warn_ratelimited(...) ((void)0)
struct mmc_card { struct { unsigned sectors,part_config; } ext_csd; bool blockaddr; };
struct mmc_host { struct mmc_card *card; };
struct mmc_command { unsigned opcode,arg; int error; };
struct mmc_data { unsigned flags,blocks,blksz; };
struct mmc_request { struct mmc_command *cmd; struct mmc_data *data; void *sbc; };
struct msdc_host { bool y2_production,hsq_en; };
static int done,queued,finalized,accepted;
static bool mmc_card_is_blockaddr(struct mmc_card *c) {return c->blockaddr;}
static bool mmc_hsq_finalize_request(struct mmc_host *m,struct mmc_request *r) {(void)m;(void)r;finalized++;return queued;}
static void mmc_request_done(struct mmc_host *m,struct mmc_request *r) {(void)m;(void)r;done++;}
static void guard(struct msdc_host *host,struct mmc_host *mmc,struct mmc_request *mrq) {
'''+source[start:end]+r'''
accepted++;
}
int main(void) {
 struct mmc_card card={{15269888,0},true};struct mmc_host mmc={&card};
 struct msdc_host host={true,true};struct mmc_command cmd={25,0,0};
 struct mmc_data data={MMC_DATA_WRITE,1,512};struct mmc_request mrq={&cmd,&data,NULL};
 queued=1;guard(&host,&mmc,&mrq);assert(cmd.error==-EROFS && finalized==1 && !done && !accepted);
 queued=0;guard(&host,&mmc,&mrq);assert(done==1 && finalized==2 && !accepted);
 host.hsq_en=false;guard(&host,&mmc,&mrq);assert(done==2 && finalized==2);
 cmd.arg=166912;guard(&host,&mmc,&mrq);assert(accepted==1);
 card.ext_csd.part_config=1;guard(&host,&mmc,&mrq);assert(accepted==1 && done==3);
 card.ext_csd.part_config=0;card.ext_csd.sectors--;guard(&host,&mmc,&mrq);assert(accepted==1 && done==4);
 card.ext_csd.sectors++;card.blockaddr=false;guard(&host,&mmc,&mrq);assert(accepted==1 && done==5);
 mmc.card=NULL;guard(&host,&mmc,&mrq);assert(accepted==1 && done==6);
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text(fixture)
            subprocess.run([shutil.which('cc') or 'clang','-Wall','-Wextra','-Werror','-I'+str(ROOT/'kernel/platform'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
    def test_write_bounds_and_protected_exclusions(self):
        for op in range(64):
            self.assertEqual(self.fn(op,166912,1,1,512,0,1),int(op in (24,25)),op)
        for t in TARGETS.values():
            first=t['start']//512;count=t['size']//512
            expected=t['type']!='kernel/boot'
            self.assertEqual(bool(self.fn(25,first,1,count,512,0,1)),expected)
            self.assertFalse(self.fn(25,first-1,1,2,512,0,1))
            self.assertFalse(self.fn(25,first+count-1,1,2,512,0,1))
        for sector in (0,1024,18432,38912,125952,1846272,3742720,0xffffffff):
            self.assertFalse(self.fn(24,sector,1,1,512,0,1))
        for blocks,size,sbc,card in [(0,512,0,1),(0xffffffff,512,0,1),(1,1024,0,1),(1,512,1,1),(1,512,0,0)]:
            self.assertFalse(self.fn(25,166912,1,blocks,size,sbc,card))
    def test_ext_csd_cannot_select_boot_rpmb_or_change_boot_config(self):
        for index in range(256):
            allowed={32:{1},33:{0,1},34:{1,2,3},161:{1},175:{1},183:{0},185:{0}}.get(index,set())
            for value in range(256):
                self.assertEqual(bool(self.fn(6,0x03000001|(index<<16)|(value<<8),0,0,0,0,0)),value in allowed,(index,value))
        for access in (0,1,2,4):self.assertFalse(self.fn(6,(access<<24)|(175<<16)|(1<<8)|1,0,0,0,0,0))
        self.assertFalse(self.fn(0,0xf0f0f0f0,0,0,0,0,1))
        self.assertTrue(self.fn(6,0x03000001|(175<<16)|(1<<8),0,0,0,0,0))
        for op in (23,24,25,26,27,28,29,35,36,38,42,56):self.assertFalse(self.fn(op,0,0,0,0,0,1))

class RootResolver(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.p=Path(self.tmp.name);self.blocks=self.p/'class/block';self.blocks.mkdir(parents=True)
        self.bin=self.p/'bin';self.bin.mkdir()
        stub=self.bin/'blkid';stub.write_text('#!/bin/sh\ncase "$3" in TYPE) echo "${MOCK_TYPE:-ext4}";; LABEL) echo "${MOCK_LABEL:-Y2ROOT}";; UUID) echo "${MOCK_UUID:-79324c69-6e75-4801-8000-000000000101}";; esac\n');stub.chmod(0o755)
        self.script=(ROOT/'initramfs/production/storage.sh').read_text().replace('/sys/class/block/',str(self.blocks)+'/')
    def tearDown(self):self.tmp.cleanup()
    def disk(self,name='mmcblk7',host='11230000',start=166912,size=1679360,capacity=15203328,media='MMC'):
        parent=self.p/f'devices/platform/{host}.mmc/mmc_host/mmc7/mmc7:0001/block/{name}'
        (parent/'device').mkdir(parents=True);(parent/'device/type').write_text(media);(parent/'removable').write_text('0');(parent/'size').write_text(str(capacity))
        part=parent/(name+'p5');part.mkdir();(part/'partition').write_text('5');(part/'start').write_text(str(start));(part/'size').write_text(str(size))
        (self.blocks/(name+'p5')).symlink_to(part)
    def resolve(self,**extra):
        env=os.environ|extra;env['PATH']=str(self.bin)+':'+env['PATH']
        return subprocess.run(['sh','-c',self.script+'\ny2_find_partition Y2ROOT 79324c69-6e75-4801-8000-000000000101 166912 1679360'],env=env,text=True,capture_output=True)
    def test_exact_internal_geometry_without_fixed_number(self):
        self.disk();r=self.resolve();self.assertEqual((r.returncode,r.stdout),(0,'/dev/mmcblk7p5\n'))
    def test_full_physical_disk_capacity(self):
        self.disk(capacity=15269888);self.assertEqual(self.resolve().returncode,0)
    def test_removable_clone_never_selected(self):
        self.disk(host='11240000',media='SD');self.assertNotEqual(self.resolve().returncode,0)
    def test_mismatched_capacity(self):
        self.disk(capacity=15203327);self.assertNotEqual(self.resolve().returncode,0)
    def test_shifted_partition(self):
        self.disk(start=166913);self.assertNotEqual(self.resolve().returncode,0)
    def test_wrong_size(self):
        self.disk(size=1679359);self.assertNotEqual(self.resolve().returncode,0)
    def test_wrong_identity_and_duplicates(self):
        self.disk()
        for key,value in [('MOCK_TYPE','vfat'),('MOCK_LABEL','Y2DATA'),('MOCK_UUID','other')]:self.assertNotEqual(self.resolve(**{key:value}).returncode,0)
        self.disk(name='mmcblk8');self.assertNotEqual(self.resolve().returncode,0)

class Transport(unittest.TestCase):
    def test_sparse_defined_bytes_and_truncation(self):
        import hashlib,struct
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);raw=p/'raw';sparse=p/'sparse';data=b'ABCD'*(1048576//4)+bytes(1048576)
            raw.write_bytes(data);sparse_encode(raw,sparse)
            self.assertEqual(sparse_identity(sparse,strict=True),{'expanded_bytes':len(data),'expanded_sha256':hashlib.sha256(data).hexdigest()})
            original=sparse.read_bytes();sparse.write_bytes(original[:-1])
            with self.assertRaises(ValueError):sparse_identity(sparse,strict=True)
            # A legal hole is still rejected for our fully defined release transport.
            sparse.write_bytes(struct.pack('<IHHHHIIII',0xed26ff3a,1,0,28,12,4096,1,1,0)+struct.pack('<HHII',0xcac3,0,1,12))
            with self.assertRaises(ValueError):sparse_identity(sparse,strict=True)
    def test_spft_scatter_maps_raw_images_only(self):
        stock=Path('/home/luca/Dokumente/Code/Y2Player/y2_v3.2.0_FM-20260813/MT6582_Android_scatter.txt').read_text()
        for first in (True,False):
            rows=scatter_rows(make_scatter(stock,first))
            for row in rows:
                name=row['partition_name']
                self.assertEqual(row['file_name'],TARGETS[name]['file'] if name in TARGETS else 'NONE')
                self.assertEqual(row['is_download']=='true',name in ('BOOTIMG','ANDROID') or (first and name=='USRDATA'))
    def test_classified_partitions_and_target_math(self):
        m=json.loads((ROOT/'docs/architecture/production-partitions.json').read_text());rows={r['name']:r for r in m['partitions']}
        self.assertEqual(len(rows),21)
        self.assertEqual({n for n,r in rows.items() if r['classification']=='REUSE'},set(TARGETS))
        for n,t in TARGETS.items():
            r=rows[n];self.assertEqual(int(r['scatter_physical_bytes'],16),t['start']);self.assertEqual(int(r['scatter_size_bytes'],16),t['size']);self.assertEqual(t['start']+0x1400000,t['linear'])

if __name__=='__main__':unittest.main()
