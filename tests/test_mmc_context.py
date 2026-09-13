"""Execute the production receive-side MMC context tracking with DMA/status faults."""
import json,re,subprocess,tempfile,unittest
from pathlib import Path
from tools.build.run import prepare_overlay
ROOT=Path(__file__).resolve().parents[1]
class Context(unittest.TestCase):
    def test_initialization_before_card_registration_and_switch_confirmation(self):
        spec=next(x for x in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays'] if x['path']=='drivers/mmc/host/mtk-sd.c')
        source=prepare_overlay(ROOT,spec).read_text();start=source.index('\tif (host->y2_emmc && mrq->cmd->error &&');end=source.index('\tmsdc_track_cmd_data(host, mrq->cmd);',start)
        dispatch_start=source.index('\tif (host->y2_emmc && mrq->cmd->opcode == MMC_GO_IDLE_STATE)');dispatch_end=source.index('\thost->error = 0;',dispatch_start)
        hdr=(ROOT/'.cache/sources/linux-6.18/include/linux/mmc/mmc.h').read_text()
        names=('MMC_GO_IDLE_STATE','MMC_SEND_OP_COND','MMC_SWITCH','MMC_SEND_STATUS','MMC_SEND_EXT_CSD','MMC_READ_SINGLE_BLOCK','MMC_READ_MULTIPLE_BLOCK','EXT_CSD_PART_CONFIG','R1_STATUS','R1_CURRENT_STATE','R1_READY_FOR_DATA','R1_SWITCH_ERROR','R1_STATE_TRAN')
        constants='\n'.join(line for line in hdr.splitlines() if any(re.match(r'#define '+n+r'(\s|\()',line) for n in names))+'\n'
        pre=r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <stdbool.h>
#include <stddef.h>
typedef uint32_t u32;typedef uint8_t u8;
#define BIT(n) (1U<<(n))
#define MMC_DATA_READ BIT(9)
#define DMA_FROM_DEVICE 2
#define dev_info_ratelimited(...) (++logs)
#define dev_info(...) ((void)0)
struct mmc_command {unsigned opcode,arg,resp[4];int error;};
struct mmc_data {int error;unsigned flags,bytes_xfered,sg_len;void *sg;};
struct mmc_request {struct mmc_command *cmd;struct mmc_data *data;};
struct msdc_host {bool y2_emmc,y2_blockaddr,y2_identified,y2_switch_pending;unsigned y2_partition,y2_pending_partition,y2_sectors;void *dev;};
static unsigned owned,logs,copies;
static void dma_sync_sg_for_cpu(void *d,void *s,unsigned n,unsigned dir){(void)d;(void)s;assert(n==1&&dir==2&&!owned);owned=1;}
static void dma_sync_sg_for_device(void *d,void *s,unsigned n,unsigned dir){(void)d;(void)s;assert(n==1&&dir==2&&owned);owned=0;}
static unsigned sg_copy_to_buffer(void *s,unsigned n,void *b,size_t len){assert(owned&&n==1&&len==512);memcpy(b,s,len);copies++;return len;}
static void complete(struct msdc_host *host,struct mmc_request *mrq) {
'''
        tail=r'''
}
int main(void) {
 struct msdc_host h={.y2_emmc=true};struct mmc_command c={.opcode=MMC_SEND_OP_COND,.resp={BIT(30)}};
 struct mmc_request r={&c,NULL};complete(&h,&r);assert(h.y2_blockaddr&&!h.y2_identified);
 unsigned char ext[512]={0};unsigned sectors=15269888;
 memcpy(ext+212,&sectors,4);ext[179]=0x49; /* boot enable/ack + boot0 access */
 struct mmc_data data={.flags=MMC_DATA_READ,.bytes_xfered=512,.sg_len=1,.sg=ext};
 r.data=&data;c.opcode=MMC_SEND_EXT_CSD;data.error=-5;complete(&h,&r);assert(!h.y2_identified&&!copies);
 data.error=0;c.error=-5;complete(&h,&r);assert(!h.y2_identified&&!copies);
 c.error=0;complete(&h,&r);assert(h.y2_identified&&h.y2_sectors==15269888&&h.y2_partition==0x49&&copies==1&&!owned);
 r.data=NULL;c.opcode=MMC_SWITCH;c.arg=0x03b34801;dispatch(&h,&r);complete(&h,&r);
 assert(h.y2_switch_pending&&h.y2_partition==0x49); /* not trusted on transport success */
 c.opcode=MMC_SEND_STATUS;c.resp[0]=7<<9;complete(&h,&r);assert(h.y2_switch_pending);
 c.resp[0]=(4<<9)|R1_READY_FOR_DATA|R1_SWITCH_ERROR;complete(&h,&r);
 assert(!h.y2_identified&&!h.y2_switch_pending&&h.y2_partition==0x49);
 r.data=&data;c.opcode=MMC_SEND_EXT_CSD;complete(&h,&r);assert(h.y2_identified);
 r.data=NULL;c.opcode=MMC_SWITCH;c.arg=0x03b34801;dispatch(&h,&r);complete(&h,&r);
 c.opcode=MMC_SEND_STATUS;c.resp[0]=(4<<9)|R1_READY_FOR_DATA;complete(&h,&r);
 assert(!h.y2_switch_pending&&h.y2_partition==0x48&&h.y2_identified);
 r.data=&data;c.opcode=MMC_READ_MULTIPLE_BLOCK;c.arg=0;complete(&h,&r);assert(logs==1&&!owned);
 c.arg=166912;complete(&h,&r);assert(logs==1); /* normal file contents never observed */
 r.data=NULL;c.opcode=MMC_SWITCH;c.arg=0x03b34801;dispatch(&h,&r);
 assert(h.y2_switch_pending);c.error=-5;complete(&h,&r);
 assert(!h.y2_identified&&!h.y2_switch_pending);
 c.error=0;c.opcode=MMC_SEND_STATUS;c.resp[0]=(4<<9)|R1_READY_FOR_DATA;complete(&h,&r);
 assert(!h.y2_identified); /* status alone cannot bless a failed switch */
 h.y2_identified=true;c.opcode=MMC_GO_IDLE_STATE;dispatch(&h,&r);
 assert(!h.y2_identified&&!h.y2_blockaddr&&!h.y2_switch_pending);
 return 0;
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text(constants+pre+source[start:end]+'}\nstatic void dispatch(struct msdc_host *host,struct mmc_request *mrq) {\n'+source[dispatch_start:dispatch_end]+tail)
            subprocess.run(['cc','-Wall','-Wextra','-Werror',str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
