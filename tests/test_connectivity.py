"""Bounded radio wire formats and explicit owner firmware provisioning."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from tools.connectivity.provision import checked, regular, defaults
from test_power import run_c
from test_audio import function

ROOT = Path(__file__).resolve().parents[1]


class Connectivity(unittest.TestCase):
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
