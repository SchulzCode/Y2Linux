"""Bounded radio wire formats and explicit owner firmware provisioning."""
import hashlib
import copy
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from tools.connectivity.provision import checked, regular, defaults
from test_power import run_c
from test_audio import function

ROOT = Path(__file__).resolve().parents[1]


class Connectivity(unittest.TestCase):
    def test_factory_disk_follows_mounted_emmc_and_rejects_other_media(self):
        source=(ROOT/'tools/connectivity/factory.c').read_text()
        body='\n'.join(function(source,n) for n in ('factory_parent','open_factory_disk'))
        run_c(r'''
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#define BLKGETSIZE64 123
#define DISK_BYTES 7784103936ULL
static unsigned index_number,bad,opens,closes;
static int mock_stat(const char *path,struct stat *st){
 memset(st,0,sizeof(*st));
 if(bad==1)return -1;
 st->st_dev=makedev(179,(!strcmp(path,"/") || bad==2)?5:7);return 0;
}
static char *mock_realpath(const char *path,char *out){
 unsigned part;if(!strcmp(path,"/sys/dev/block/179:5"))part=5;
 else {assert(!strcmp(path,"/sys/dev/block/179:7"));part=7;}
 if(bad==3)return NULL;
 unsigned n=index_number+((bad==4 && part==7)?1:0);
 snprintf(out,PATH_MAX,"/sys/devices/platform/emmc/block/mmcblk%u/mmcblk%up%u",n,n,part);return out;
}
static int factory_sysfs(const char *parent,const char *name,char *out,size_t capacity){
 if(!strcmp(name,"partition"))snprintf(out,capacity,"%u\n",bad==5?8:parent[strlen(parent)-1]-'0');
 else if(!strcmp(name,"device/type"))snprintf(out,capacity,"%s\n",bad==6?"SD":"MMC");
 else {assert(!strcmp(name,"dev"));snprintf(out,capacity,"179:0%s",bad==7?" trailing":"\n");}
 return 0;
}
static int mock_open(const char *path,int flags){
 char expected[64];snprintf(expected,sizeof(expected),"/dev/mmcblk%u",index_number);
 assert(!strcmp(path,expected) && flags==(O_RDONLY|O_CLOEXEC|O_NOFOLLOW));
 opens++;return bad==8?-1:42;
}
static int mock_fstat(int fd,struct stat *st){
 assert(fd==42);memset(st,0,sizeof(*st));st->st_mode=bad==9?S_IFREG:S_IFBLK;
 st->st_rdev=makedev(179,bad==10?8:0);return 0;
}
static int mock_ioctl(int fd,int op,uint64_t *value){
 assert(fd==42 && op==BLKGETSIZE64);*value=bad==11?7818182656ULL:DISK_BYTES;return bad==12?-1:0;
}
static int mock_close(int fd){assert(fd==42);closes++;return 0;}
#define stat(p,s) mock_stat(p,s)
#define realpath(p,b) mock_realpath(p,b)
#define open(p,f) mock_open(p,f)
#define fstat(f,s) mock_fstat(f,s)
#define ioctl(f,o,v) mock_ioctl(f,o,v)
#define close(f) mock_close(f)
''' + body + r'''
int main(void){
 for(index_number=0;index_number<8;index_number++){
  opens=closes=bad=0;assert(open_factory_disk()==42 && opens==1 && !closes);
  for(bad=1;bad<=12;bad++){
   opens=closes=0;assert(open_factory_disk()==-1);
   assert(opens==(bad>=8));assert(closes==(bad>=9));
  }
 }
}
''')

    def test_actual_btif_rearms_tail_irq_for_every_transfer(self):
        source=(ROOT/'kernel/platform/connectivity/btif.c').read_text()
        source=source.replace('int y2_btif_send(', 'static int y2_btif_send(')
        body='\n'.join(function(source,n) for n in ('advance','y2_btif_irq','y2_btif_send'))
        defines='\n'.join(l for l in source.splitlines() if l.startswith('#define '))
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define Y2_CONN_DMA_SIZE 8192
#define READ_ONCE(x) (x)
#define min(a,b) ((a)<(b)?(a):(b))
#define dma_wmb() ((void)0)
#define dma_rmb() ((void)0)
#define IRQ_NONE 0
#define IRQ_HANDLED 1
typedef int irqreturn_t;
struct y2_conn {
 bool transport_on;int dma_tx,dma_rx,tx_wait,irq[4],failure;
 unsigned txptr,rxptr;void *txdma,*rxdma,*btif;unsigned char *txbuf,*rxbuf;
};
static uint32_t regs[32];
static unsigned char fifo[8192],wire[8192];
static unsigned wire_size,writes,bad_enable_order,stuck;
static void mutex_lock(int *l){assert(!*l);*l=1;}
static void mutex_unlock(int *l){assert(*l);*l=0;}
static void wake_up_all(int *w){(void)w;}
static void y2_conn_failed(struct y2_conn *c,int e){c->failure=e;}
static void y2_stp_receive(struct y2_conn *c,const unsigned char *p,unsigned n){assert(0);}
''' + defines + r'''
#define R(offset) regs[(offset)/4]
static void update_irq(void){
 /* MediaTek APDMA one-shot: HW clears INT_EN at the free-space threshold. */
 if(R(INT_EN) && R(LEFT)>=R(THRESHOLD)){R(INT_FLAG)=1;R(INT_EN)=0;}
}
static unsigned readl(void *p){
 assert((uintptr_t)p>=(uintptr_t)regs && (uintptr_t)p<(uintptr_t)(regs+32));return *(uint32_t *)p;
}
static void writel(unsigned v,void *p){
 unsigned offset=(unsigned)((uintptr_t)p-(uintptr_t)regs);assert(offset<sizeof(regs));writes++;
 if(offset==WRITE_PTR){
  if(!R(ENABLE))bad_enable_order++;
  unsigned n=(v&0xffff)-(R(READ_PTR)&0xffff);
  if((v^R(READ_PTR))&0x10000)n+=8192;
  assert(n<=8192);R(VALID)=n;R(LEFT)=8192-n;
 }
 if(offset==FLUSH)assert(!R(STOP));
 R(offset)=v;update_irq();
}
static void hardware_step(void){
 if(stuck || !R(ENABLE))return;
 unsigned n=R(FLUSH)?R(VALID):(R(VALID)&~7U);
 unsigned p=R(READ_PTR)&0xffff;assert(wire_size+n<=sizeof(wire));
 for(unsigned i=0;i<n;i++)wire[wire_size++]=fifo[(p+i)%8192];
 unsigned next=p+n;R(READ_PTR)=(next%8192)|((R(READ_PTR)^(next>=8192?0x10000:0))&0x10000);
 R(VALID)-=n;R(LEFT)+=n;
 if(R(FLUSH)){R(FLUSH)=0;R(ENABLE)=0;}
 update_irq();
}
#define readl_poll_timeout(p,v,condition,delay,timeout) ({ \
 int rc=-ETIMEDOUT;for(unsigned polls=0;polls<40;polls++){ \
 (v)=readl(p);if(condition){rc=0;break;}hardware_step();}rc;})
''' + body + r'''
static void reset(struct y2_conn *c,unsigned pointer){
 memset(c,0,sizeof(*c));memset(regs,0,sizeof(regs));memset(fifo,0,sizeof(fifo));
 c->transport_on=true;c->txdma=regs;c->txbuf=fifo;c->txptr=pointer;c->irq[1]=71;
 R(READ_PTR)=R(WRITE_PTR)=pointer;R(LEFT)=8192;R(THRESHOLD)=8192-7;
 wire_size=writes=bad_enable_order=stuck=0;
}
static void transfer(struct y2_conn *c,unsigned n){
 unsigned char input[2048];assert(n<=sizeof(input));for(unsigned i=0;i<n;i++)input[i]=(i*31+n)&255;
 wire_size=0;
 assert(!y2_btif_send(c,input,n));
 hardware_step(); /* The initial VALID read saw all n bytes, before DMA ran. */
 if(R(INT_FLAG))assert(y2_btif_irq(71,c)==IRQ_HANDLED);
 hardware_step();
 assert(!R(VALID) && !R(FLUSH) && wire_size==n && !memcmp(input,wire,n));
 assert(!bad_enable_order && !c->failure && !c->dma_tx);
}
int main(void){
 struct y2_conn c;
 reset(&c,0);transfer(&c,26); /* observed failure: 24 sent, two trailer bytes left */
 transfer(&c,26); /* Interrupt is one-shot, so the second transfer must rearm too. */
 for(unsigned n=1;n<=37;n++)transfer(&c,n); /* every short-tail residue and exact multiples */
 for(unsigned i=0;i<40;i++)transfer(&c,1005); /* wrap the coherent FIFO repeatedly */
 reset(&c,8190);transfer(&c,26);transfer(&c,2048);
 /* A rejected/blocked transfer cannot publish a new pointer or combine STOP/FLUSH. */
 unsigned char byte=0;
 reset(&c,0);c.transport_on=false;assert(y2_btif_send(&c,&byte,1)==-ESHUTDOWN && !writes);
 reset(&c,0);R(STOP)=1;assert(y2_btif_send(&c,&byte,1)==-ESHUTDOWN && !writes);
 reset(&c,0);R(FLUSH)=1;stuck=1;
 assert(y2_btif_send(&c,&byte,1)==-ETIMEDOUT && !writes && !c.txptr);
 reset(&c,0);R(LEFT)=0;stuck=1;
 assert(y2_btif_send(&c,&byte,1)==-ETIMEDOUT && !writes && !c.txptr);
}
''')

    def test_actual_stp_mandatory_bootstrap_and_full_mode_transition(self):
        source=(ROOT/'kernel/platform/connectivity/stp.c').read_text()
        source=source.replace('int y2_stp_send(', 'static int y2_stp_send(')
        source=source.replace('void y2_stp_receive(', 'static void y2_stp_receive(')
        body='\n'.join(function(source,n) for n in
                       ('y2_stp_send','y2_stp_acknowledge','deliver','y2_stp_receive'))
        body+='\n'+function((ROOT/'kernel/platform/connectivity/wmt.c').read_text().replace('int y2_conn_wmt(', 'static int y2_conn_wmt('), 'y2_conn_wmt')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#include "connectivity/protocol.h"
#define Y2_CONN_WMT 4
#define Y2_CONN_BT 0
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define min(x,y) ((x)<(y)?(x):(y))
#define dev_err(...) ((void)0)
#define msecs_to_jiffies(x) (x)
#define wait_event_timeout(q,condition,timeout) ((void)(q),(void)(timeout),!!(condition))
struct y2_stp_frame {unsigned size;unsigned char data[Y2_STP_MAX_PAYLOAD+6];};
struct y2_conn {
 int stp_lock,tx_wait,retry_work,response,failure,command;
 bool transport_on,full_stp,wmt_reg_read,wmt_rf_calibrate;
 unsigned chip,hvr,fvr;
 unsigned stp_pending,stp_tx,stp_oldest,stp_rx,stp_retries,rx_used,rx_needed,response_size;
 struct y2_stp_frame window[8];
 unsigned char rx_frame[Y2_STP_MAX_PAYLOAD+6],response_data[256];
};
static struct y2_stp_frame sent[16];
static unsigned sent_count,timer,bt_packets,completions;
static int system_wq,send_error;
static struct y2_conn *active;
static unsigned char inbound[700];
static unsigned inbound_size;
static void reinit_completion(int *p){*p=0;}
static int wait_for_completion_timeout(int *p,unsigned timeout);
static void y2_btif_report_timeout(struct y2_conn *c){(void)c;}
static void mutex_lock(int *p){assert(!*p);*p=1;}
static void mutex_unlock(int *p){assert(*p);*p=0;}
static void wake_up_all(int *p){(void)p;}
static void mod_delayed_work(int q,int *w,unsigned time){assert(time==250);timer=1;}
static void cancel_delayed_work(int *w){timer=0;}
static void memzero_explicit(void *p,unsigned n){memset(p,0,n);}
static int completion_done(int *p){return *p;}
static void complete(int *p){assert(!*p);*p=1;completions++;}
static void y2_conn_failed(struct y2_conn *c,int error){assert(error<0);c->failure=error;}
static void y2_hci_receive(struct y2_conn *c,const unsigned char *p,unsigned n){bt_packets++;}
static int y2_btif_send(struct y2_conn *c,const unsigned char *p,unsigned n){
 assert(c->stp_lock && n<=sizeof(sent[0].data) && sent_count<16);
 if(send_error)return send_error;
 sent[sent_count].size=n;memcpy(sent[sent_count++].data,p,n);return 0;
}
''' + body + r'''
static int wait_for_completion_timeout(int *p,unsigned timeout){
 assert(timeout==4000 && active);
 if(inbound_size)y2_stp_receive(active,inbound,inbound_size);
 return *p || active->failure;
}
static void reset(struct y2_conn *c){
 memset(c,0,sizeof(*c));c->transport_on=true;c->wmt_reg_read=true;
 sent_count=timer=bt_packets=completions=0;send_error=0;
 c->chip=0x6582;c->hvr=0x8a01;c->fvr=0x8a00;
 active=c;inbound_size=0;
}
/* Synthetic RF data: only the length/header/sequence comes from own hardware.
 * Never put calibration results or their identifying hash in the repository. */
static void rf_event(void){
 inbound_size=636;memset(inbound,0,sizeof(inbound));
 const unsigned char header[]={0x9b,0x42,0x76,0x53,2,0x14,0x72,2,0,1};
 memcpy(inbound,header,sizeof(header));
 for(unsigned i=10;i<634;i++)inbound[i]=(unsigned char)(i*17+31);
 unsigned crc=y2_stp_crc(inbound+4,630);inbound[634]=crc;inbound[635]=crc>>8;
}
static void rf_ready(struct y2_conn *c){
 reset(c);c->full_stp=true;c->wmt_reg_read=false;c->wmt_rf_calibrate=true;
 c->stp_rx=c->stp_tx=c->stp_oldest=3;
 rf_event();
}
int main(void){
 struct y2_conn c;
 /* Stock wmt_core_reg_rw_raw uses bRawFlag=false. Its first chip read is
  * a 26-byte mandatory STP packet, never a bare 20-byte WMT command. */
 const unsigned char request[]={1,8,16,0,2,1,0,1,8,0,0,0x80,0,0,0,0,0xff,0xff,0,0};
 const unsigned char wire[]={0x80,0x40,20,0,1,8,16,0,2,1,0,1,8,0,0,0x80,0,0,0,0,0xff,0xff,0,0,0,0};
 reset(&c);assert(!y2_stp_send(&c,Y2_CONN_WMT,request,sizeof(request)));
 assert(sent_count==1 && sent[0].size==sizeof(wire) && !memcmp(sent[0].data,wire,sizeof(wire)));
 assert(!c.stp_pending && !c.stp_tx && !c.stp_rx && !timer);
 assert(y2_stp_send(&c,Y2_CONN_BT,request,sizeof(request))==-EHOSTDOWN);
 assert(sent_count==1);
 send_error=-EIO;assert(y2_stp_send(&c,Y2_CONN_WMT,request,sizeof(request))==-EIO);
 assert(!c.stp_pending && !timer);
 /* Outer length=16, inner WMT length=4 (stock register-read quirk).
  * Exercise every DMA split, especially header and trailer boundaries. */
 const unsigned char reply[]={0x80,0x40,16,0,2,8,4,0,0,0,0,1,8,0,0,0x80,0x82,0x65,0,0,0,0};
 for(unsigned split=0;split<=sizeof(reply);split++){
  reset(&c);y2_stp_receive(&c,reply,split);
  assert(c.response==(split==sizeof(reply)));
  y2_stp_receive(&c,reply+split,sizeof(reply)-split);
  assert(!c.failure && c.response && completions==1 && c.response_size==16);
  assert(!memcmp(c.response_data,reply+4,16) && !c.rx_used && !sent_count && !timer);
 }
 reset(&c);for(unsigned i=0;i<sizeof(reply);i++)y2_stp_receive(&c,reply+i,1);
 assert(c.response && !c.failure);
 /* Checksums are disabled only during mandatory bootstrap. */
 unsigned char modified[sizeof(reply)];memcpy(modified,reply,sizeof(reply));
 modified[3]=0xa5;modified[sizeof(reply)-1]=0x5a;
 reset(&c);y2_stp_receive(&c,modified,sizeof(modified));assert(c.response && !c.failure);
 reset(&c);c.wmt_reg_read=false;y2_stp_receive(&c,reply,sizeof(reply));assert(c.failure==-EPROTO);
 for(unsigned bad=0;bad<5;bad++){
  reset(&c);memcpy(modified,reply,sizeof(reply));
  if(bad==0)modified[0]=2; /* Former raw parser must no longer accept this. */
  if(bad==1)modified[1]=0; /* BT cannot speak before full mode. */
  if(bad==2)modified[2]=0;
  if(bad==3){modified[1]=0x41;modified[2]=1;} /* >256-byte WMT reply */
  if(bad==4)modified[1]|=0x80;
  y2_stp_receive(&c,modified,sizeof(modified));assert(c.failure && !c.response);
 }
 reset(&c);
 const unsigned char set_reply[]={0x80,0x40,6,0,2,4,2,0,0,3,0,0};
 y2_stp_receive(&c,set_reply,sizeof(set_reply));assert(c.response && !c.failure);
 c.full_stp=true;c.response=0;c.wmt_reg_read=false;
 const unsigned char query[]={1,4,1,0,4};
 assert(!y2_stp_send(&c,Y2_CONN_WMT,query,sizeof(query)));
 assert(sent[0].data[0]==0x87 && sent[0].data[3]==0xcc && c.stp_pending==1 && timer);
 unsigned char full[]={0x80,0x40,10,0xca,2,4,6,0,0,4,0xdf,0x0e,0x68,1,0,0};
 unsigned crc=y2_stp_crc(full+4,10);full[14]=crc;full[15]=crc>>8;
 y2_stp_receive(&c,full,sizeof(full));
 assert(!c.failure && c.response && c.stp_rx==1 && !c.stp_pending && !timer);
 assert(sent_count==2 && sent[1].size==4 && sent[1].data[0]==0x80);
 unsigned before=completions;y2_stp_receive(&c,full,sizeof(full));
 assert(!c.failure && completions==before && sent_count==3); /* duplicate ACK, no duplicate data */
 reset(&c);c.full_stp=true;full[15]^=1;
 y2_stp_receive(&c,full,sizeof(full));assert(c.failure==-EBADMSG && !c.response);
 assert(!bt_packets);
 /* All DMA split points, then a byte-at-a-time RF event. */
 for(unsigned split=0;split<=636;split++){
  rf_ready(&c);y2_stp_receive(&c,inbound,split);
  assert(c.response==(split==636));
  y2_stp_receive(&c,inbound+split,636-split);
  assert(!c.failure && c.response_size==630 && c.stp_rx==4 && !c.rx_used);
  assert(!memcmp(c.response_data,inbound+4,6) && c.response_data[6]==0);
  assert(sent_count==1 && sent[0].size==4 && sent[0].data[0]==0x83);
 }
 rf_ready(&c);for(unsigned i=0;i<636;i++)y2_stp_receive(&c,inbound+i,1);
 assert(!c.failure && c.response_size==630 && completions==1);
 y2_stp_receive(&c,inbound,636);assert(completions==1 && sent_count==2);
 /* The complete production WMT path must accept this event and retire its
  * window entry. The next ordinary command proves trailer/state alignment. */
 const unsigned char rf_cmd[]={1,0x14,1,0,1},rf_short[]={2,0x14,2,0,0,1};
 rf_ready(&c);c.wmt_rf_calibrate=false;
 assert(!y2_conn_wmt(&c,rf_cmd,sizeof(rf_cmd),rf_short,sizeof(rf_short)));
 assert(c.response_size==630 && !c.wmt_rf_calibrate && !c.stp_pending && !timer);
 const unsigned char coex_cmd[]={1,0x10,2,0,1,1},coex_reply[]={2,0x10,1,0,0};
 unsigned char next[]={0xa4,0x40,5,0xe9,2,0x10,1,0,0,0,0};
 crc=y2_stp_crc(next+4,5);next[9]=crc;next[10]=crc>>8;
 memcpy(inbound,next,sizeof(next));inbound_size=sizeof(next);
 assert(!y2_conn_wmt(&c,coex_cmd,sizeof(coex_cmd),coex_reply,sizeof(coex_reply)));
 assert(c.response_size==5 && c.stp_rx==5 && !c.stp_pending && !c.rx_used);
 /* Six-byte RF replies keep their existing strict semantics. */
 rf_ready(&c);memcpy(inbound+4,rf_short,6);inbound[1]=0x40;inbound[2]=6;
 inbound[3]=inbound[0]+inbound[1]+inbound[2];
 crc=y2_stp_crc(inbound+4,6);inbound[10]=crc;inbound[11]=crc>>8;inbound_size=12;
 assert(!y2_conn_wmt(&c,rf_cmd,sizeof(rf_cmd),rf_short,sizeof(rf_short)));
 /* Reject an unrequested result, other silicon, bad length/status/opcode,
  * CRC failure, truncation and duplicate new-sequence WMT events. */
 for(unsigned bad=0;bad<10;bad++){
  rf_ready(&c);
  if(bad==0)c.chip=0x6572;
  if(bad==1)c.hvr=0x8a00;
  if(bad==2)c.fvr=0x8a01;
  if(bad==3)inbound[8]=1;
  if(bad==4)inbound[9]=2;
  if(bad==5)inbound[5]=0x10;
  if(bad==6)inbound[6]--;
  if(bad==7)inbound_size--;
  crc=y2_stp_crc(inbound+4,630);inbound[634]=crc;inbound[635]=crc>>8;
  if(bad==8)inbound[635]^=1;
  int ret=y2_conn_wmt(&c,bad==9?coex_cmd:rf_cmd,bad==9?sizeof(coex_cmd):sizeof(rf_cmd),
                      bad==9?coex_reply:rf_short,bad==9?sizeof(coex_reply):sizeof(rf_short));
  assert(ret==(bad==7?-ETIMEDOUT:bad==8?-EBADMSG:-EPROTO));
  assert(!c.wmt_rf_calibrate && !c.wmt_reg_read && !c.command);
 }
 for(unsigned length=629;length<=631;length+=2){
  rf_ready(&c);inbound[2]=length;inbound[3]=inbound[0]+inbound[1]+inbound[2];
  inbound[6]=length-4;inbound[7]=(length-4)>>8;
  crc=y2_stp_crc(inbound+4,length);inbound[4+length]=crc;inbound[5+length]=crc>>8;
  inbound_size=length+6;
  assert(y2_conn_wmt(&c,rf_cmd,sizeof(rf_cmd),rf_short,sizeof(rf_short))==-EPROTO);
 }
 rf_ready(&c);c.wmt_rf_calibrate=false;y2_stp_receive(&c,inbound,636);
 assert(c.failure==-EPROTO && !completions);
 rf_ready(&c);c.response=1;y2_stp_receive(&c,inbound,636);
 assert(c.failure==-EPROTO && !completions);
 rf_ready(&c);send_error=-EIO;
 assert(y2_conn_wmt(&c,rf_cmd,sizeof(rf_cmd),rf_short,sizeof(rf_short))==-EIO);
 assert(!c.wmt_rf_calibrate && !c.command);
}
''')

    def test_stock_conn_remap_address_and_failed_write(self):
        source=(ROOT/'kernel/platform/clocks.c').read_text()
        body=function(source.replace('int y2_ccf_radio_remap(', 'static int y2_ccf_radio_remap('), 'y2_ccf_radio_remap')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define __iomem
#define EPROBE_DEFER 517
static unsigned char regs[0x2000],before[0x2000];
static void *y2_clock_bases[4];
static int y2_clk_lock,locked,drop_write,writes;
#define spin_lock_irqsave(lock,flags) do { (void)(lock);(flags)=0;assert(!locked);locked=1; } while(0)
#define spin_unlock_irqrestore(lock,flags) do { (void)(lock);(void)(flags);assert(locked);locked=0; } while(0)
#define mb() ((void)0)
static unsigned readl(void *p) {
 unsigned value;assert(locked && (unsigned char *)p>=regs && (unsigned char *)p<=regs+sizeof(regs)-4);
 memcpy(&value,p,4);return value;
}
static void writel(unsigned value,void *p) {
 assert(locked && (unsigned char *)p>=regs && (unsigned char *)p<=regs+sizeof(regs)-4);
 writes++;if(!drop_write)memcpy(p,&value,4);
}
static unsigned word(unsigned offset) {unsigned v;memcpy(&v,regs+offset,4);return v;}
''' + body + r'''
int main(void) {
 assert(y2_ccf_radio_remap(0)==-EPROBE_DEFER && !writes);
 y2_clock_bases[2]=regs;
 assert(y2_ccf_radio_remap(2)==-EINVAL && !writes);
 /* Stock FM code uses 0xf0001000 + 0x310, physical 0x10001310.
  * Test stale loader mappings, all high bits and the formerly wrong offset. */
 const unsigned old[]={0,0x1800,0x1fff,0xbeef1234,0xfffffbdf};
 for(unsigned i=0;i<sizeof(old)/sizeof(old[0]);i++) {
  memset(regs,0xa5,sizeof(regs));memcpy(regs+0x310,&old[i],4);memcpy(before,regs,sizeof(regs));
  writes=0;assert(!y2_ccf_radio_remap(0) && writes==1 && !locked);
  assert(word(0x310)==((old[i]&~0x1fffU)|0x1bdf));
  assert(word(0x1310)==0xa5a5a5a5);
  assert(!memcmp(regs,before,0x310));
  assert(!memcmp(regs+0x314,before+0x314,sizeof(regs)-0x314));
 }
 /* An ignored/rejected remap must prevent a successful power-on result. */
 memset(regs,0,sizeof(regs));drop_write=1;
 assert(y2_ccf_radio_remap(0)==-EIO && !locked && !word(0x310));
 drop_write=0;
 assert(!y2_ccf_radio_remap(1)); /* Existing MD mapping remains distinct. */
 assert(word(0x300)==0x53514f3f && word(0x304)==0x5b595755);
 assert(word(0x308)==0x4543413f && word(0x30c)==0x4d4b4947);
 assert(!word(0x310) && !word(0x1310));
}
''')

    def test_factory_open_with_repeated_separators_preserves_seed(self):
        run_c(r'''
#include <assert.h>
#include "../../tools/connectivity/fs-store.h"
static unsigned request(unsigned char *b,unsigned op,const void *a,unsigned n,unsigned flags) {
 struct y2_reply r={b,8,0};memset(b,0,Y2_FS_STRIDE);y2_conn_put32(b,op);
 y2_fs_arg(&r,a,n);if(op==0x1001)y2_fs_int(&r,flags);return r.size;
}
int main(void) {
 struct y2_store *s=calloc(1,sizeof(*s));assert(s);
 assert(y2_fs_new(s,"X/",1));assert(y2_fs_new(s,"Z/",1));
 const unsigned char seed[4]={0x11,0x22,0x33,0x44}; /* synthetic, not factory data */
 assert(!y2_fs_seed(s,"X/MP0D_000",seed,sizeof(seed)));
 struct y2_file *factory=y2_fs_lookup(s,"X/MP0D_000");assert(factory);
 unsigned char in[Y2_FS_STRIDE],out[Y2_FS_STRIDE],word[4],path[256];
 for(unsigned separators=1;separators<32;separators++) {
  memset(path,0,sizeof(path));path[0]='X';path[2]=':';
  unsigned at=4;for(unsigned i=0;i<separators;i++){path[at]='\\';at+=2;}
  const char *base="MP0D_000";for(unsigned i=0;base[i];i++){path[at]=base[i];at+=2;}at+=2;
  /* Request 53's operation/flags and doubled separator, with synthetic seed. */
  unsigned n=request(in,0x1001,path,at,0x1010400);
  assert(y2_fs_dispatch(s,in,n,out)==16);
  int h=(int)y2_conn_le32(out+12);assert(h>0 && h<Y2_FS_HANDLES);
  assert(s->handles[h].file==factory && factory->size==4);
  assert(!memcmp(factory->data,seed,4) && !memcmp(factory->factory,seed,4));
  y2_conn_put32(word,h);n=request(in,0x1005,word,4,0);
  assert(y2_fs_dispatch(s,in,n,out)==16 && !y2_conn_le32(out+12));
 }
 /* Canonicalization also applies inside directories, without adding files. */
 assert(y2_fs_new(s,"Z/NVRAM",1));assert(y2_fs_new(s,"Z/NVRAM/NVD_CORE",1));
 const char nested[]="Z:\\\\NVRAM\\\\NVD_CORE\\\\TEST";
 memset(path,0,sizeof(path));for(unsigned i=0;i<sizeof(nested);i++)path[2*i]=nested[i];
 unsigned n=request(in,0x1001,path,2*sizeof(nested),0x10000);
 assert(y2_fs_dispatch(s,in,n,out)==16 && (int)y2_conn_le32(out+12)>0);
 assert(y2_fs_lookup(s,"Z/NVRAM/NVD_CORE/TEST"));
 /* Empty components are harmless; traversal/host paths/embedded NUL aren't. */
 const char *bad[]={"X:\\\\..\\MP0D_000","X:\\A\\\\..\\MP0D_000",
  "X:\\MP0D_000/../BAD","/run/y2/factory/MP0D_000","C:\\MP0D_000", "X:\\.\\MP0D_000"};
 for(unsigned i=0;i<sizeof(bad)/sizeof(bad[0]);i++) {
  memset(path,0,sizeof(path));unsigned len=strlen(bad[i])+1;
  for(unsigned j=0;j<len;j++)path[2*j]=bad[i][j];
  n=request(in,0x1001,path,2*len,0x1010400);
  assert(y2_fs_dispatch(s,in,n,out)==16 && (int)y2_conn_le32(out+12)<0);
 }
 const char suffix[]="X:\\\\MP0D_000";
 memset(path,0,sizeof(path));for(unsigned i=0;i<sizeof(suffix);i++)path[2*i]=suffix[i];
 path[2*sizeof(suffix)]='Q'; /* extra component after the terminating NUL */
 n=request(in,0x1001,path,2*(sizeof(suffix)+2),0x1010400);
 assert(y2_fs_dispatch(s,in,n,out)==16 && (int)y2_conn_le32(out+12)<0);
 assert(factory->factory_size==4 && !memcmp(factory->factory,seed,4));
 y2_fs_clear(s);free(s);
}
''')

    def test_md_request_transport_padding_keeps_service_abi_strict(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "connectivity/protocol.h"
int main(void) {
 unsigned char data[Y2_FS_STRIDE]={0}; struct y2_fs_packet p;
 /* Synthetic GetDiskInfo: UTF-16 drive and flags. The mailbox can report
  * 32 bytes while counted arguments end at 28; replies have no trailer. */
 y2_conn_put32(data,0x100e);y2_conn_put32(data+4,2);
 y2_conn_put32(data+8,8);memcpy(data+12,"Z\0:\0\\\0\0",8);
 y2_conn_put32(data+20,4);y2_conn_put32(data+24,0);
 y2_conn_put32(data+28,0xa5a5a5a5);
 assert(y2_fs_request_size(data,32,&p)==28);
 assert(p.op==0x100e && p.count==2 && p.args[0].size==8 && p.args[1].size==4);
 assert(y2_fs_parse(data,32,&p));
 assert(!y2_fs_parse(data,28,&p));
 for(unsigned n=0;n<28;n++)assert(y2_fs_request_size(data,n,&p)<0);
 for(unsigned n=29;n<40;n++)if(n!=32)assert(y2_fs_request_size(data,n,&p)<0);
 /* A zero-argument restore query, ordinary close and a short argument. */
 for(unsigned count=0;count<2;count++) for(unsigned bytes=1;bytes<=4;bytes++) {
  memset(data,0,sizeof(data));y2_conn_put32(data,count?0x1005:0x101c);
  y2_conn_put32(data+4,count);y2_conn_put32(data+8,bytes);
  unsigned end=count?16:8;
  for(unsigned pad=0;pad<256;pad++) {
   memset(data+end,pad,4);
   assert(y2_fs_request_size(data,end+4,&p)==(int)end);
   assert(!y2_fs_parse(data,end,&p));
   assert(y2_fs_parse(data,end+4,&p));
  }
 }
 y2_conn_put32(data+4,17);assert(y2_fs_request_size(data,20,&p)<0);
 y2_conn_put32(data+4,1);y2_conn_put32(data+8,0xffffffff);
 assert(y2_fs_request_size(data,20,&p)<0);
 assert(y2_fs_request_size(data,Y2_FS_STRIDE+4,&p)<0);
 /* Exhaust the length bound, including the final legal mailbox word. */
 for(unsigned bytes=0;bytes<=Y2_FS_STRIDE;bytes++) {
  y2_conn_put32(data+8,bytes);unsigned end=12+((bytes+3)&~3U);
  if(end<=Y2_FS_STRIDE)assert(y2_fs_request_size(data,end,&p)==(int)end);
  if(end+4<=Y2_FS_STRIDE)assert(y2_fs_request_size(data,end+4,&p)==(int)end);
  if(end>Y2_FS_STRIDE)assert(y2_fs_request_size(data,Y2_FS_STRIDE,&p)<0);
 }
}
''')

    def test_early_regulatory_database_is_pinned_and_signed(self):
        from tools.build import regulatory
        files=regulatory.load(ROOT)
        self.assertEqual(set(files),set(regulatory.FILES.values()))
        self.assertGreater(len(files['lib/firmware/regulatory.db']),1000)
        self.assertGreater(len(files['lib/firmware/regulatory.db.p7s']),100)
        with patch.object(Path,'read_bytes',return_value=b'changed archive'), self.assertRaises(ValueError):
            regulatory.load(ROOT)

    @unittest.skipUnless(os.environ.get('Y2_ARTIFACT_TEST_ROOT'), 'requires emitted integrated DT')
    def test_emitted_dt_rejects_radio_ownership_and_power_regressions(self):
        from tools.validation.dev_dtb import check, CONN
        from tools.validation.formats import fdt
        from tools.validation.dtb import cells
        build=Path(os.environ['Y2_ARTIFACT_TEST_ROOT'])
        raw=(build/'y2.dtb').read_bytes();size=(build/'initramfs.cpio.gz').stat().st_size
        check(raw,size)
        nodes,reserved=fdt(raw)
        if CONN not in nodes:self.skipTest('historical pre-connectivity artifact')
        rail='/pwrap@1000d000/pmic/regulators/ldo_vcn33_bt'
        cases=[
            ('/reserved-memory/high-owned@bdf00000','reg',cells(0xbe000000,0x2000000)),
            (CONN,'memory-region',nodes['/clock-controller@10000000']['phandle']),
            (CONN,'reg',nodes[CONN]['reg'].replace(cells(0x11000780),cells(0x11000200))),
            (CONN,'interrupts',nodes[CONN]['interrupts'].replace(cells(71),cells(44))),
            (CONN,'clocks',nodes[CONN]['clocks'][:-4]+cells(0)),
            (CONN,'resets',nodes[CONN]['resets'][:-4]+cells(0)),
            (rail,'regulator-always-on',b''),
            (rail,'regulator-max-microvolt',cells(3400000)),
        ]
        for path,key,value in cases:
            changed=copy.deepcopy(nodes);changed[path][key]=value
            with self.subTest(path=path,key=key), patch('tools.validation.dev_dtb.fdt',return_value=(changed,reserved)), self.assertRaises((ValueError,KeyError)):
                check(raw,size)

    def test_actual_hci_fragmentation_and_corrupt_lengths(self):
        source=(ROOT/'kernel/platform/connectivity/hci.c').read_text()
        body=function(source.replace('void y2_hci_receive(', 'static void y2_hci_receive('), 'y2_hci_receive')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include "connectivity/protocol.h"
typedef unsigned char u8;
#define HCI_EVENT_PKT 4
#define HCI_ACLDATA_PKT 2
#define HCI_SCODATA_PKT 3
#define HCI_MAX_FRAME_SIZE 1028
#define GFP_ATOMIC 0
#define READ_ONCE(x) (x)
#define min(a,b) ((a)<(b)?(a):(b))
struct sk_buff {unsigned len,type; unsigned char data[1028];};
struct hci_dev {struct {unsigned byte_rx,err_rx;} stat;};
struct y2_conn {bool hci_open,hci_paused;struct hci_dev *hdev;struct sk_buff *hci_rx;unsigned hci_needed;int failure;};
static unsigned delivered,bytes;
#define hci_skb_pkt_type(s) ((s)->type)
static struct sk_buff *bt_skb_alloc(unsigned n,int flags) {assert(n==1028);return calloc(1,sizeof(struct sk_buff));}
static void kfree_skb(struct sk_buff *s) {free(s);}
static void skb_put_data(struct sk_buff *s,const void *data,unsigned n) {assert(s->len+n<=1028);memcpy(s->data+s->len,data,n);s->len+=n;}
static int hci_recv_frame(struct hci_dev *dev,struct sk_buff *s) {delivered++;bytes+=s->len;free(s);return 0;}
static void y2_conn_failed(struct y2_conn *c,int error) {c->failure=error;}
''' + body + r'''
int main(void) {
 unsigned char frames[]={4,0x0e,4,1,0,0,0, 2,1,0,3,0,1,2,3, 3,1,0,2,4,5};
 for(unsigned split=1;split<=sizeof(frames);split++) {
  struct hci_dev dev={0}; struct y2_conn c={.hci_open=true,.hdev=&dev};
  delivered=bytes=0;
  for(unsigned at=0;at<sizeof(frames);) {
   unsigned n=sizeof(frames)-at;if(n>split)n=split;
   y2_hci_receive(&c,frames+at,n);at+=n;
  }
  assert(delivered==3 && bytes==sizeof(frames)-3 && !c.hci_rx && !c.failure);
 }
 struct hci_dev dev={0};struct y2_conn c={.hci_open=true,.hdev=&dev};
 const unsigned char corrupt[]={2,0,0,0xff,0xff};
 y2_hci_receive(&c,corrupt,sizeof(corrupt));assert(c.failure==-EPROTO && !c.hci_rx);
 c.failure=0;const unsigned char invalid[]={0xff};
 y2_hci_receive(&c,invalid,1);assert(c.failure==-EPROTO && dev.stat.err_rx==2);
 c.failure=0;c.hci_paused=true;unsigned before=delivered;
 y2_hci_receive(&c,frames,sizeof(frames));assert(delivered==before && !c.hci_rx);
}
''')

    def test_actual_domain_ack_failure_and_dma_pointer_rollover(self):
        body=function((ROOT/'kernel/platform/connectivity/btif.c').read_text(),'advance')
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include <string.h>
#include "connectivity/domain.h"
#define Y2_CONN_DMA_SIZE 8192
''' + body + r'''
static unsigned regs[0x800/4],failed,delays;
static unsigned read_reg(void *c,unsigned r) {return regs[r/4];}
static void write_reg(void *c,unsigned r,unsigned v) {
 regs[r/4]=v;
 if(r==0x280 || r==0x284) {
  unsigned bit=r==0x280?2:1;
  if(!failed) {
   if((v&12)==12) {regs[0x60c/4]|=bit;regs[0x610/4]|=bit;}
   else if(!(v&12)) {regs[0x60c/4]&=~bit;regs[0x610/4]&=~bit;}
  }
 }
}
static void delay(unsigned us) {delays+=us;}
int main(void) {
 struct y2_spm_io io={NULL,read_reg,write_reg,delay};
 for(unsigned domain=0;domain<2;domain++) {
  unsigned reg=domain?0x284:0x280,bit=domain?1:2;
  memset(regs,0,sizeof(regs));regs[reg/4]=0x112;
  assert(!y2_radio_domain_sequence(&io,domain,1));
  assert((regs[reg/4]&0x11f)==13 && (regs[0x60c/4]&bit) && (regs[0x610/4]&bit));
  assert(!y2_radio_domain_sequence(&io,domain,0));
  assert((regs[reg/4]&0x11f)==0x112 && !(regs[0x60c/4]&bit) && !(regs[0x610/4]&bit));
  failed=1;delays=0;assert(y2_radio_domain_sequence(&io,domain,1)==-ETIMEDOUT);
  assert(delays==10000 && !(regs[reg/4]&1));failed=0;
 }
 assert(y2_radio_domain_sequence(&io,2,1)==-EINVAL);
 for(unsigned offset=0;offset<8192;offset++)for(unsigned wrap=0;wrap<2;wrap++) {
  unsigned p=offset|(wrap<<16);
  assert(advance(p,8192)==(p^0x10000));
  unsigned q=advance(p,8191);q=advance(q,1);assert(q==(p^0x10000));
  assert((advance(p,1)&0xffff)==(offset+1)%8192);
 }
}
''')

    def test_system_update_never_selects_or_maps_data(self):
        from tools.production.system_update import preserving_scatter
        from tools.production.layout import scatter_rows
        stock=(ROOT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
        rows=scatter_rows(preserving_scatter(stock))
        self.assertEqual([x['partition_name'] for x in rows if x['is_download']=='true'], ['BOOTIMG','ANDROID'])
        for row in rows:
            if row['partition_name'] not in ('BOOTIMG','ANDROID'):
                self.assertEqual((row['file_name'],row['is_download']),('NONE','false'))

    def test_calibration_runtime_layout_and_ram_filesystem(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "connectivity/md-runtime.h"
#include "../../tools/connectivity/fs-store.h"
static unsigned packet(unsigned char *b,unsigned op,const void **args,unsigned *sizes,unsigned n) {
 struct y2_reply r={b,8,0}; memset(b,0,Y2_FS_STRIDE); y2_conn_put32(b,op);
 for(unsigned i=0;i<n;i++) y2_fs_arg(&r,args[i],sizes[i]);
 return r.size;
}
static int status(const unsigned char *b) {return (int)y2_conn_le32(b+12);}
int main(void) {
 unsigned char r[280],m[288];
 for(int xtal=0;xtal<2;xtal++) {
  y2_md_runtime(r,m,xtal);
  assert(y2_conn_le32(r)==Y2_MD_MAGIC && y2_conn_le32(r+276)==Y2_MD_MAGIC);
  assert(y2_conn_le32(r+0x74)==Y2_MD_VIEW+Y2_MD_FS_OFFSET);
  assert(y2_conn_le32(r+0x78)==5*Y2_FS_STRIDE);
  assert(y2_conn_le32(m+4)==(xtal?6:10));
  for(unsigned i=0;i<70;i++) {
   unsigned a=y2_conn_le32(r+4*i);
   if(a>=Y2_MD_VIEW && a<Y2_MD_VIEW+0x200000) assert(a<Y2_MD_VIEW+Y2_MD_SMEM_SIZE);
  }
 }
 struct y2_store *s=calloc(1,sizeof(*s)); assert(s);
 assert(y2_fs_new(s,"Z/",1)); assert(y2_fs_new(s,"X/",1));
 unsigned char in[Y2_FS_STRIDE],out[Y2_FS_STRIDE];
 const unsigned char seed[]={0x10,0x20,0x30};
 assert(!y2_fs_seed(s,"X/SEED",seed,sizeof(seed)));
 const unsigned char path[]="Z\0:\0\\\0N\0V\0R\0A\0M\0\0";
 const unsigned char file[]="Z\0:\0\\\0N\0V\0R\0A\0M\0\\\0T\0E\0S\0T\0\0";
 unsigned char word[4]={0},length[4]={0},flags[4]={0};
 const void *args[4]={path,flags,length,NULL}; unsigned sizes[4]={sizeof(path),4,4,0};
 unsigned n=packet(in,0x1007,args,sizes,1);
 assert(y2_fs_dispatch(s,in,n,out)==16 && status(out)==0);
 args[0]=file;sizes[0]=sizeof(file);y2_conn_put32(flags,0x10000);
 n=packet(in,0x1001,args,sizes,2);assert(y2_fs_dispatch(s,in,n,out)==16);
 int h=status(out);assert(h>0 && h<Y2_FS_HANDLES);y2_conn_put32(word,h);
 args[0]=word;sizes[0]=4;args[1]=seed;sizes[1]=3;args[2]=length;y2_conn_put32(length,3);
 n=packet(in,0x1004,args,sizes,3);assert(y2_fs_dispatch(s,in,n,out)==24 && !status(out));
 /* Reject declared writes longer than their data; preserve position/data. */
 y2_conn_put32(length,4);n=packet(in,0x1004,args,sizes,3);
 assert(y2_fs_dispatch(s,in,n,out)==24 && status(out)<0);
 assert(s->handles[h].position==3 && s->handles[h].file->size==3);
 args[1]=flags;sizes[1]=4;args[2]=length;y2_conn_put32(flags,0);y2_conn_put32(length,0);
 n=packet(in,0x1002,args,sizes,3);assert(y2_fs_dispatch(s,in,n,out)==16 && !status(out));
 y2_conn_put32(flags,3);n=packet(in,0x1003,args,sizes,2);
 assert(y2_fs_dispatch(s,in,n,out)==32 && !status(out));
 struct y2_fs_packet parsed;assert(!y2_fs_parse(out,32,&parsed));
 assert(parsed.count==3 && parsed.args[2].size==3 && !memcmp(parsed.args[2].data,seed,3));
 /* Protected data: a RAM shadow can change, original never changes. */
 struct y2_file *f=y2_fs_lookup(s,"X/SEED");assert(f);
 assert(!y2_fs_resize(s,f,100));f->data[0]=0x99;assert(f->factory[0]==0x10);
 assert(!y2_fs_resize(s,f,0) && !f->data);
 /* Actual allocated bytes, not only logical length, obey the quota. */
 assert(!y2_fs_resize(s,f,Y2_FS_FILE_LIMIT));
 assert(y2_fs_resize(s,f,Y2_FS_FILE_LIMIT+1)==-38);
 s->used=Y2_FS_QUOTA;assert(y2_fs_resize(s,s->handles[h].file,4)==-22);
 s->used=Y2_FS_FILE_LIMIT+3;
 /* Malformed packets/traversal never become host paths or valid names. */
 const unsigned char bad[]="Z\0:\0\\\0.\0.\0\\\0X\0\0";
 args[0]=bad;sizes[0]=sizeof(bad);n=packet(in,0x1007,args,sizes,1);
 assert(y2_fs_dispatch(s,in,n,out)==16 && status(out)<0);
 for(unsigned i=0;i<8;i++)assert(y2_fs_dispatch(s,in,i,out)<0);
 n=packet(in,0x101d,args,sizes,1);assert(y2_fs_dispatch(s,in,n,out)==16 && status(out)==-12);
 y2_fs_clear(s);assert(!s->used);free(s);
}
''')

    def test_stp_crc_headers_and_fs_bounds(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "connectivity/protocol.h"
int main(void) {
 unsigned char data[Y2_FS_STRIDE] = {0}, h[4];
 unsigned channel, length, seq, ack;
 struct y2_fs_packet p;
 assert(y2_stp_crc((const unsigned char *)"123456789", 9) == 0xbb3d);
 for(unsigned s=0;s<8;s++)for(unsigned a=0;a<8;a++)for(unsigned n=0;n<4096;n++) {
  h[0]=0x80|(s<<3)|a; h[1]=0x40|(n>>8); h[2]=n;
  h[3]=h[0]+h[1]+h[2];
  assert((y2_stp_header(h,&channel,&length,&seq,&ack)==0)==(n<=2048));
  if(n<=2048)assert(channel==4 && length==n && seq==s && ack==a);
  h[3]^=1; assert(y2_stp_header(h,&channel,&length,&seq,&ack));
 }
 y2_conn_put32(data,0x1003); y2_conn_put32(data+4,2);
 y2_conn_put32(data+8,4); y2_conn_put32(data+12,7);
 y2_conn_put32(data+16,3); memcpy(data+20,"abc",3);
 for(unsigned n=0;n<24;n++)assert(y2_fs_parse(data,n,&p));
 assert(!y2_fs_parse(data,24,&p));
 assert(p.op==0x1003 && p.count==2 && p.args[1].size==3);
 assert(!memcmp(p.args[1].data,"abc",3));
 assert(y2_fs_parse(data,25,&p));
 for(unsigned n=Y2_FS_STRIDE;n<Y2_FS_STRIDE+20;n++) {
  y2_conn_put32(data+8,n);assert(y2_fs_parse(data,24,&p));
 }
 y2_conn_put32(data+8,0xffffffff);assert(y2_fs_parse(data,24,&p));
 y2_conn_put32(data+4,17);assert(y2_fs_parse(data,24,&p));
 y2_conn_put32(data+4,0);assert(!y2_fs_parse(data,8,&p));
 char path[128];
 const unsigned char good[]="Z\0:\0\\\0N\0V\0R\0A\0M\0\\\0M\0P\0_\x00" "0\0\0";
 assert(!y2_fs_path(good,sizeof(good),path,sizeof(path)));
 assert(!strcmp(path,"NVRAM/MP_0"));
 assert(y2_fs_path(good,sizeof(good),path,5));
 for(unsigned n=0;n<sizeof(good);n++)assert(y2_fs_path(good,n,path,sizeof(path)));
 memcpy(data,good,sizeof(good)); data[6]='.';
 assert(y2_fs_path(data,sizeof(good),path,sizeof(path)));
 memcpy(data,good,sizeof(good)); data[7]=1;
 assert(y2_fs_path(data,sizeof(good),path,sizeof(path)));
 memcpy(data,good,sizeof(good)); data[8]=0;
 assert(y2_fs_path(data,sizeof(good),path,sizeof(path)));
}
''')

    def test_owner_files_reject_symlink_wrong_hash_and_size(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'input'; source.write_bytes(b'synthetic input')
            link = root/'link'; link.symlink_to(source)
            with self.assertRaises(OSError): regular(link, 15)
            with self.assertRaises(ValueError): regular(source, 14)
            with self.assertRaises(ValueError): checked(source, 15, '0'*64)
            self.assertEqual(checked(source, 15, hashlib.sha256(b'synthetic input').hexdigest()),
                             b'synthetic input')
            with self.assertRaises(ValueError): defaults(source)


if __name__ == '__main__':
    unittest.main()
