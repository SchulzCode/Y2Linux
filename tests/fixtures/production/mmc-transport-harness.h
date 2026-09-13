/* SPDX-License-Identifier: GPL-2.0-only */
#include <assert.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include "storage-policy.h"
#define GENMASK(h,l) ((~0U << (l)) & (~0U >> (31-(h))))
typedef uint32_t u32;
#define MMC_DATA_WRITE BIT(8)
#define MMC_DATA_READ BIT(9)
#define MMC_RSP_PRESENT BIT(0)
#define MMC_RSP_136 BIT(1)
#define MMC_RSP_CRC BIT(2)
#define MMC_RSP_BUSY BIT(3)
#define MMC_RSP_OPCODE BIT(4)
#define MMC_RSP_R1 (MMC_RSP_PRESENT | MMC_RSP_CRC | MMC_RSP_OPCODE)
#define MMC_RSP_R1B (MMC_RSP_R1 | MMC_RSP_BUSY)
#define MMC_RSP_R1B_NO_CRC (MMC_RSP_PRESENT | MMC_RSP_BUSY | MMC_RSP_OPCODE)
#define MMC_CMD_AC 0
#define MMC_CMD_ADTC BIT(5)
#define mmc_resp_type(c) ((c)->flags & 31)
#define mmc_cmd_type(c) ((c)->flags & 96)
#define SD_IO_RW_DIRECT 52
#define SD_SWITCH_VOLTAGE 11
#define SD_APP_SEND_SCR 51
#define SD_APP_SEND_NUM_WR_BLKS 22
#define SD_SWITCH 6
#define SD_APP_SD_STATUS 13
#define WARN_ON(x) assert(!(x))
#define spin_lock_irqsave(l,f) ((void)(l))
#define spin_unlock_irqrestore(l,f) ((void)(l))
#define dev_warn_ratelimited(...) ((void)0)
#define dev_err(...) ((void)0)
#define dev_dbg(...) ((void)0)
#define mod_delayed_work(...) ((void)0)
struct mmc_data;
struct mmc_command {unsigned opcode,arg,flags,resp[4];int error;struct mmc_data *data;};
struct mmc_data {unsigned flags,blocks,blksz,bytes_xfered;int error;
    unsigned timeout_ns,timeout_clks;struct mmc_command *stop;unsigned char *buffer;};
struct mmc_request {struct mmc_command *cmd,*sbc,*stop;struct mmc_data *data;};
struct msdc_host;
struct mmc_card {bool is_mmc;};
struct mmc_host {struct mmc_card *card;struct msdc_host *priv;};
struct msdc_host {bool y2_emmc,hsq_en,y2_identified,y2_blockaddr,y2_switch_pending,hs400_tuning;
    unsigned y2_sectors,y2_partition,y2_pending_partition,error,timeout_ns,timeout_clks;
    int lock,cmd_rsp;struct mmc_host *mmc;struct mmc_request *mrq;
    struct mmc_command *cmd;struct mmc_data *data;unsigned char *base;};
static unsigned char registers[4096];
static unsigned commands[16],arguments[16],encodings[16],ncommands,prepared,started,done,resets;
static unsigned poll_fault;
static unsigned char factory_mbr[512];
static struct mmc_host *mmc_from_priv(struct msdc_host *h) {return h->mmc;}
static struct msdc_host *mmc_priv(struct mmc_host *m) {return m->priv;}
static bool mmc_card_mmc(struct mmc_card *c) {assert(c);return c->is_mmc;}
static bool mmc_op_multi(unsigned op) {return op==18 || op==25;}
static bool mmc_op_tuning(unsigned op) {return op==19 || op==21;}
static u32 readl(void *address) {u32 v;memcpy(&v,address,4);return v;}
static void writel(u32 v,void *address) {
    memcpy(address,&v,4);
    if(address==registers+SDC_CMD) {
        assert(ncommands<16);commands[ncommands]=v&63;
        arguments[ncommands]=readl(registers+SDC_ARG);encodings[ncommands++]=v;
    }
}
static void sdr_clr_bits(void *p,u32 bits) {writel(readl(p)&~bits,p);}
static void sdr_set_bits(void *p,u32 bits) {writel(readl(p)|bits,p);}
static void sdr_set_field(void *p,u32 mask,u32 v) {writel((readl(p)&~mask)|((v<<__builtin_ctz(mask))&mask),p);}
static int poll_register(void *p) {
    if(poll_fault==1 && p==registers+MSDC_DMA_CTRL)return -ETIMEDOUT;
    if(poll_fault==2 && p==registers+MSDC_DMA_CFG)return -ETIMEDOUT;
    if(p==registers+MSDC_DMA_CTRL)sdr_clr_bits(p,MSDC_DMA_CTRL_STOP);
    return 0;
}
#define readl_poll_timeout_atomic(p,val,condition,delay,timeout) poll_register(p)
static void msdc_set_timeout(struct msdc_host *h,unsigned ns,unsigned clks) {h->timeout_ns=ns;h->timeout_clks=clks;}
static u32 msdc_cmd_find_resp(struct msdc_host *h,struct mmc_command *c) {return 1;}
static void msdc_prepare_data(struct msdc_host *h,struct mmc_data *d) {prepared++;}
static bool msdc_data_prepared(struct mmc_data *d) {return true;}
static bool mmc_hsq_finalize_request(struct mmc_host *h,struct mmc_request *r) {return false;}
static void mmc_request_done(struct mmc_host *h,struct mmc_request *r) {done++;}
static void msdc_request_done(struct msdc_host *h,struct mmc_request *r) {h->mrq=NULL;done++;}
static void msdc_reset_hw(struct msdc_host *h) {resets++;}
static bool msdc_cmd_is_ready(struct msdc_host *h,struct mmc_request *r,struct mmc_command *c) {return true;}
static int msdc_auto_cmd_done(struct msdc_host *h,int events,struct mmc_command *c) {assert(!h->y2_emmc);return 0;}
static void msdc_start_data(struct msdc_host *h,struct mmc_command *c,struct mmc_data *d) {
    h->data=d;started++;
    if(c->arg==0 && (d->flags&MMC_DATA_READ))memcpy(d->buffer,factory_mbr,512);
}
static const u32 cmd_ints_mask=MSDC_INT_CMDRDY|MSDC_INT_CMDTMO|MSDC_INT_RSPCRCERR;
static const u32 data_ints_mask=MSDC_INT_XFER_COMPL|MSDC_INT_DATTMO|MSDC_INT_DATCRCERR;
static void msdc_cmd_next(struct msdc_host *,struct mmc_request *,struct mmc_command *);
