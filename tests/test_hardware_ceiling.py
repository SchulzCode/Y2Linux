"""Exercise request ownership and fault cleanup in the actual patched drivers."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]


def source(path):
    spec = next(item for item in json.loads(
        (ROOT / 'kernel/patches/manifest.json').read_text())['overlays']
        if item['path'] == path)
    return prepare_overlay(ROOT, spec).read_text()


def run_c(program):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory)
        (path / 'test.c').write_text(program)
        subprocess.run([shutil.which('cc'), '-std=gnu11', '-Wall', '-Werror',
                        str(path / 'test.c'), '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test')], check=True)


class HardwareCeiling(unittest.TestCase):
    def test_clock_cap_requires_idle_ownership_and_a_present_negotiated_card(self):
        text = source('drivers/mmc/host/mtk-sd.c')
        body = text[text.index('static ssize_t y2_clock_limit_hz_store'):]
        body = body[:body.index('static DEVICE_ATTR_RW')]
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <sys/types.h>
#define MMC_POWER_ON 2
#define WRITE_ONCE(x,v) ((x)=(v))
#define dev_info(...) ((void)0)
struct msdc_host { unsigned y2_clock_limit; };
struct card { bool removed; };
struct mmc_host { unsigned f_max,actual_clock; struct card *card;
 struct { unsigned timing,clock,power_mode; } ios; struct msdc_host host; };
struct device { struct mmc_host *mmc; };
struct device_attribute {};
static int claimed,programs,fail_change;
static void *dev_get_drvdata(struct device *d) { return d->mmc; }
static void *mmc_priv(struct mmc_host *m) { return &m->host; }
static int kstrtouint(const char *s,int base,unsigned *v) { return sscanf(s,"%u",v)==1?0:-EINVAL; }
static void mmc_claim_host(struct mmc_host *m) { assert(!claimed);claimed=1; }
static void mmc_release_host(struct mmc_host *m) { assert(claimed);claimed=0; }
static bool mmc_card_removed(struct card *c) { return c->removed; }
static int msdc_set_mclk(struct msdc_host *h,unsigned timing,unsigned hz) {
 assert(claimed); assert(hz==25000000); /* card negotiated only 25 MHz */
 programs++;return fail_change;
}
''' + body + r'''
int main(void) {
 struct card c={false};struct mmc_host m={.f_max=50000000,.card=&c,
 .ios={1,25000000,MMC_POWER_ON},.host={25000000}};struct device d={&m};
 assert(y2_clock_limit_hz_store(&d,0,"50000000",8)==8);
 assert(programs==1 && !claimed && m.host.y2_clock_limit==50000000);
 assert(y2_clock_limit_hz_store(&d,0,"13000000",8)==8);
 assert(programs==2 && !claimed && m.host.y2_clock_limit==13000000);
 assert(y2_clock_limit_hz_store(&d,0,"52000000",8)==-EINVAL);
 assert(y2_clock_limit_hz_store(&d,0,"invalid",7)==-EINVAL);
 c.removed=true;
 assert(y2_clock_limit_hz_store(&d,0,"50000000",8)==-ENODEV);
 c.removed=false;m.card=0;
 assert(y2_clock_limit_hz_store(&d,0,"50000000",8)==-ENODEV);
 m.card=&c;m.ios.power_mode=0;
 assert(y2_clock_limit_hz_store(&d,0,"50000000",8)==-ENODEV);
 m.ios.power_mode=MMC_POWER_ON;m.f_max=13000000;
 assert(y2_clock_limit_hz_store(&d,0,"25000000",8)==-ERANGE);
 assert(programs==2 && !claimed && m.host.y2_clock_limit==13000000);
 m.f_max=50000000;fail_change=-ETIMEDOUT;
 assert(y2_clock_limit_hz_store(&d,0,"50000000",8)==-ETIMEDOUT);
 assert(programs==3 && !claimed && m.host.y2_clock_limit==13000000);
}
''')

    def test_clock_timeout_restores_previous_mode_or_blocks_future_io(self):
        text = source('drivers/mmc/host/mtk-sd.c')
        body = text[text.index('static int y2_msdc_clock_ready('):]
        body = body[:body.index('static int msdc_set_mclk(')]
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdint.h>
typedef uint32_t u32;
#define MSDC_CFG 0
#define MSDC_INTEN 4
#define MSDC_CFG_CKPDN 16
#define MSDC_CFG_CKSTB 128
#define dev_err(...) ((void)0)
struct mmc_host { unsigned actual_clock; };
struct msdc_host { struct mmc_host *mmc; void *base,*src_clk_cg,*dev;
 unsigned mclk;int y2_clock_error; };
static unsigned registers[8];
static int polls, poll_results[2], disables, enables, enable_result;
static struct mmc_host *mmc_from_priv(struct msdc_host *h) { return h->mmc; }
static int poll_clock(unsigned wait,unsigned timeout) {
 assert(wait==1 && timeout==20000 && polls<2);
 return poll_results[polls++];
}
#define readl_poll_timeout(addr,val,condition,wait,timeout) \
 ((val)=0,(void)(condition),poll_clock(wait,timeout))
static void clk_disable_unprepare(void *c) { disables++; }
static int clk_prepare_enable(void *c) { enables++;return enable_result; }
static void writel(unsigned v,void *p) { *(unsigned *)p=v; }
static void sdr_set_bits(void *p,unsigned v) { *(unsigned *)p|=v; }
''' + body + r'''
int main(void) {
 struct mmc_host m={13000000};struct msdc_host h={.mmc=&m,
  .base=registers,.mclk=13000000,.y2_clock_error=-EIO};
 unsigned previous=0x5000|MSDC_CFG_CKPDN;
 assert(y2_msdc_clock_ready(&h,previous,7,0)==0);
 assert(polls==1 && !disables && !enables && !h.y2_clock_error);
 polls=0;poll_results[0]=-ETIMEDOUT;poll_results[1]=0;
 assert(y2_msdc_clock_ready(&h,previous,7,0)==-ETIMEDOUT);
 assert(polls==2 && disables==1 && enables==1 && !h.y2_clock_error);
 assert(registers[0]==previous && registers[1]==7);
 assert(h.mclk==13000000 && m.actual_clock==13000000);
 polls=0;poll_results[1]=-ETIMEDOUT;
 assert(y2_msdc_clock_ready(&h,previous,7,0)==-ETIMEDOUT);
 assert(polls==2 && h.y2_clock_error==-EIO && !h.mclk && !m.actual_clock);
 polls=disables=enables=0;poll_results[0]=0;
 assert(y2_msdc_clock_ready(&h,previous,7,-EIO)==-EIO);
 assert(polls==1 && !disables && enables==1 && h.y2_clock_error==-EIO);
 polls=0;poll_results[0]=-ETIMEDOUT;enable_result=-EIO;
 h.mclk=m.actual_clock=13000000;
 assert(y2_msdc_clock_ready(&h,previous,7,0)==-ETIMEDOUT);
 assert(polls==1 && h.y2_clock_error==-EIO && !h.mclk && !m.actual_clock);
}
''')

    def test_dma_bus_fault_is_quiesced_before_channel_release(self):
        text = source('drivers/usb/musb/musbhsdma.c')
        body = text[text.index('static int dma_channel_abort('):]
        body = body[:body.index('irqreturn_t dma_controller_irq')]
        run_c(r'''
#include <assert.h>
#include <stdint.h>
#define __iomem
typedef uint8_t u8;typedef uint16_t u16;
enum { MUSB_DMA_STATUS_BUSY=1,MUSB_DMA_STATUS_FREE=2,MUSB_DMA_STATUS_BUS_ABORT=3 };
enum { MUSB_TXCSR=1,MUSB_RXCSR=2,MUSB_HSDMA_CONTROL=3 };
enum { MUSB_TXCSR_AUTOSET=1,MUSB_TXCSR_DMAENAB=2,MUSB_TXCSR_DMAMODE=4,
 MUSB_RXCSR_AUTOCLEAR=1,MUSB_RXCSR_DMAENAB=2,MUSB_RXCSR_DMAMODE=4 };
#define MUSB_HSDMA_CHANNEL_OFFSET(ch,reg) 3
struct musb { struct { int (*ep_offset)(u8,u16); } io; };
struct musb_dma_controller { void *base; struct musb *private_data; };
struct musb_dma_channel { struct musb_dma_controller *controller; u8 idx,epnum; int transmit; };
struct dma_channel { struct musb_dma_channel *private_data; int status; };
static unsigned writes,csr=7,addr=99,count=99,control=1;
static int offset(u8 ep,u16 reg) { return reg; }
static u16 musb_readw(void *b,int off) { return csr; }
static void musb_writew(void *b,int off,u16 value) {
 if (off==MUSB_TXCSR) {
  /* DMAENAB must be removed in an earlier write than DMAMODE. */
  if ((csr&4) && !(value&4)) assert(!(csr&2));
  csr=value;
 } else if(off==MUSB_RXCSR) csr=value;
 else control=value;
 writes++;
}
static void musb_write_hsdma_addr(void *b,u8 ch,unsigned value) { addr=value;writes++; }
static void musb_write_hsdma_count(void *b,u8 ch,unsigned value) { count=value;writes++; }
''' + body + r'''
int main(void) {
 struct musb m={{offset}};struct musb_dma_controller c={0,&m};
 struct musb_dma_channel p={&c,0,1,1};struct dma_channel d={&p,0};
 for(int state=1;state<=3;state+=2)for(int tx=0;tx<2;tx++) {
  writes=0;csr=7;addr=count=99;control=1;p.transmit=tx;d.status=state;
  assert(dma_channel_abort(&d)==0);
  assert(writes>=4 && !csr && !addr && !count && !control);
  assert(d.status==MUSB_DMA_STATUS_FREE);
 }
 writes=0;assert(dma_channel_abort(&d)==0);assert(!writes);
}
''')
