"""Stock Y2 main-disk capacity and independently observed native coordinates."""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from tools.build.run import prepare_overlay
from tools.production.layout import addressing_contract, make_readback_plan
from tools.production.validate import validate_addressing
from tests.test_mmc_transport import function

ROOT = Path(__file__).resolve().parents[1]


class StockWindow(unittest.TestCase):
    def test_manifest_and_readback_modes_match_physical_evidence(self):
        contract = addressing_contract()
        self.assertEqual(contract['logical_to_native_user_offset_sectors'], 23552)
        self.assertEqual(contract['logical_disk_sectors'], 15203328)
        self.assertEqual(contract['native_user_sectors'], 15269888)
        evidence = json.loads((ROOT/'docs/hardware-evidence/2026-09-13-storage05-owner/readback-corrected-analysis.json').read_text())
        classification = json.loads((ROOT/'docs/architecture/production-partitions.json').read_text())
        plan = make_readback_plan(classification, ROOT/'tests/fixtures/production')
        by_name = {x['partition']: x for x in plan['preservation_ranges']+plan['target_readbacks']}
        for name, row in zip(('BOOTIMG','MBR','EBR1','EBR2','ANDROID','USRDATA'), evidence['rows']):
            item = by_name[name]
            self.assertEqual(item['stock_logical_start_bytes'], row['stock_logical_start_bytes'])
            self.assertEqual(item['physical_start_bytes'], row['native_user_start_bytes'])
            self.assertEqual(item['legacy_global_da_start_bytes'], row['spft_global_start_bytes'])
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory); (out/'metadata').mkdir()
            for name, obj in [('partitions', classification), ('storage-addressing', contract), ('readback-plan', plan)]:
                (out/'metadata'/f'{name}.json').write_text(json.dumps(obj))
            manifest = {'kernel_version':'6.18.0-y2linux-storage06', 'storage_addressing':contract,
                        'hardware_compatibility':{'accepted_linux_user_sector_counts':[15203328]}}
            self.assertTrue(validate_addressing(out, manifest))
            for field in ('logical_to_native_user_offset_sectors','logical_disk_sectors','excluded_native_tail_sectors'):
                saved = contract[field]; contract[field] += 1
                with self.assertRaises(ValueError): validate_addressing(out, manifest)
                contract[field] = saved
            manifest.pop('storage_addressing')
            with self.assertRaises(ValueError): validate_addressing(out, manifest)
            manifest['storage_addressing'] = contract
            by_name['ANDROID']['physical_start_bytes'] -= 23552*512
            (out/'metadata/readback-plan.json').write_text(json.dumps(plan))
            with self.assertRaises(ValueError): validate_addressing(out, manifest)

    def test_actual_block_allocation_and_native_bounds(self):
        spec = next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays']
                    if s['path'] == 'drivers/mmc/core/block.c')
        body = function(prepare_overlay(ROOT, spec).read_text(), 'mmc_blk_alloc')
        code = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>
#include "storage-policy.h"
typedef uint64_t sector_t;
struct device {const char *of_node;};
struct mmc_host {struct device *parent;};
struct mmc_card {
    struct mmc_host *host;struct device dev;
    struct {unsigned sectors;} ext_csd;
    struct {unsigned capacity,read_blkbits;} csd;
    bool sd,mmc,blockaddr;
};
struct mmc_blk_data {sector_t size;};
#define MMC_BLK_DATA_AREA_MAIN 1
#define ERR_PTR(e) ((struct mmc_blk_data *)(intptr_t)(e))
#define mmc_dev(h) ((h)->parent)
#define mmc_card_sd(c) ((c)->sd)
#define mmc_card_mmc(c) ((c)->mmc)
#define mmc_card_blockaddr(c) ((c)->blockaddr)
static bool of_device_is_compatible(const char *node,const char *compat) {return node && !strcmp(node,compat);}
static struct mmc_blk_data result;
static struct mmc_blk_data *mmc_blk_alloc_req(struct mmc_card *card,struct device *dev,
    sector_t size,bool ro,const char *name,int area,int part) {
    assert(dev==&card->dev && !ro && !name && area==MMC_BLK_DATA_AREA_MAIN && part==0);
    result.size=size;return &result;
}
''' + body + r'''
int main(void) {
    struct device device={"innioasis,y2-mmc"};struct mmc_host host={&device};
    struct mmc_card card={.host=&host,.ext_csd={15269888},.csd={200,10},.mmc=true,.blockaddr=true};
    assert(mmc_blk_alloc(&card)->size==15203328);
    assert(card.ext_csd.sectors==15269888); /* physical identity remains true */
    card.ext_csd.sectors--;assert(mmc_blk_alloc(&card)==ERR_PTR(-ENODEV));card.ext_csd.sectors++;
    card.blockaddr=false;assert(mmc_blk_alloc(&card)==ERR_PTR(-ENODEV));card.blockaddr=true;
    card.mmc=false;card.sd=true;assert(mmc_blk_alloc(&card)==ERR_PTR(-ENODEV));
    device.of_node="innioasis,y2-sd";assert(mmc_blk_alloc(&card)->size==400);
    device.of_node="mediatek,mt8135-mmc";card.mmc=true;card.sd=false;
    assert(mmc_blk_alloc(&card)->size==15269888);
    device.of_node=NULL;card.ext_csd.sectors=12345;assert(mmc_blk_alloc(&card)->size==12345);
    /* Independent observed DA coordinates minus boot1/boot2/RPMB, then sectors. */
    const unsigned logical[]={0,1024,145408,166912,2104320};
    const unsigned global[]={0x1400000,0x1480000,0x5b00000,0x6580000,0x41780000};
    for(unsigned op=0;op<64;op++)for(unsigned i=0;i<5;i++) {
        unsigned wire=y2_emmc_wire_arg(op,logical[i]);
        assert(wire==((op==17||op==18||op==24||op==25)?(global[i]-0x880000)/512:logical[i]));
    }
    assert(Y2_EMMC_USER_OFFSET+Y2_EMMC_DISK_SECTORS+Y2_EMMC_TAIL_SECTORS==15269888);
    /* Every native sector reachable by an allowed write is in the two
     * independent physical extents. Nothing reaches the prefix or tail. */
    struct y2_emmc_request r={.opcode=24,.blocks=1,.blksz=512,.direction=Y2_MMC_WRITE,.user_area=1,.identified=1};
    for(r.arg=0;r.arg<15269888;r.arg++) {
        unsigned wire=y2_emmc_wire_arg(24,r.arg);
        bool expected=(wire>=190464 && wire<1869824)||(wire>=2127872 && wire<3766272);
        assert(!!y2_emmc_request_allowed(&r)==expected);
    }
    return 0;
}
'''
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            (p/'test.c').write_text(code)
            subprocess.run(['cc', '-O2', '-Wall', '-Wextra', '-Werror', '-I'+str(ROOT/'kernel/platform'),
                            str(p/'test.c'), '-o', str(p/'test')], check=True)
            subprocess.run([str(p/'test')], check=True)
