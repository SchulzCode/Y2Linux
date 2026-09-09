"""Production detach/re-entry functions with MMIO and upstream callback fixtures."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Reconnect(unittest.TestCase):
    def test_owned_controller_lifecycle_and_guards(self):
        adapter = (ROOT / 'kernel/usb/y2_musb.c').read_text()
        lifecycle = adapter[adapter.index('static void y2_usb_detach(void)\n{'):
                            adapter.index('static void y2_usb_worker(struct work_struct *work)\n{')]
        write_callback = adapter[adapter.index('static void y2_musb_writeb('):
                                 adapter.index('static void y2_musb_writew(')]
        source = r'''
#include <assert.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include "gate.h"
#include "session.h"
#include "live.h"
#include "../diagnostic/usb_wake.h"
#define __iomem
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define EIO 5
#define ENODEV 19
#define MUSB_POWER 1
#define MUSB_DEVCTL 0x60
#define MUSB_POWER_SOFTCONN 0x40
#define MUSB_DEVCTL_SESSION 1
#define MUSB_DEVCTL_HR 2
#define MUSB_INTRUSB 0x0a
#define MUSB_INTRTX 2
#define MUSB_INTRRX 4
typedef uint8_t u8;typedef uint32_t u32;
struct musb { unsigned char *mregs;int lock,softconnect; };
static unsigned char mac[1024],phy[256];
static void *y2_usb_phy=phy;
static struct musb controller={.mregs=mac,.softconnect=1},*y2_musb=&controller;
static struct y2_usb_live y2_live;
static struct y2_platform_snapshot y2_power;
static struct y2_pwrap_snapshot y2_usb_supply;
static struct y2_session y2_session;
static bool y2_usb_detached;
static unsigned disconnected,stopped,started,clock_bad,session_bad,fifo_bad,cleared;
#define spin_lock_irqsave(lock,flags) do {(flags)=0;assert(!*(lock));*(lock)=1;} while(0)
#define spin_unlock_irqrestore(lock,flags) do {(void)(flags);assert(*(lock));*(lock)=0;} while(0)
#define pr_info(...) ((void)0)
static void y2_usb_phase(unsigned stage) {y2_live.stage=stage;}
static unsigned readb(const void *p) {return *(const uint8_t*)p;}
static unsigned readw(const void *p) {uint16_t v;memcpy(&v,p,2);return v;}
static void writeb(unsigned v,void *p) {*(uint8_t*)p=v;}
static void writel(unsigned v,void *p) {memcpy(p,&v,4);}
static int y2_clock_read(void *context,unsigned addr,unsigned *v) {
    (void)context;
    switch(addr) {case 0x10003018:*v=clock_bad?1024:0;break;
    case 0x10000060:*v=0x01010100;break;case 0x10209220:*v=0xfd000001;break;
    case 0x1020922c:*v=0x80000001;break;default:assert(0);}
    return 0;
}
static unsigned session_read(void *p,unsigned r) {
    assert(p==y2_musb);
    if(r==0x100) return session_bad ? 0x80 : 0x98;
    return phy[r];
}
static void session_write(void *p,unsigned r,unsigned v) {
    assert(p==y2_musb && (r==0x6c || r==0x6d));phy[r]=v;
}
static void session_delay(void *p) {assert(p==y2_musb);}
static struct y2_session_io y2_session_io(struct musb *p)
{return (struct y2_session_io){p,session_read,session_write,session_delay};}
static void musb_g_disconnect(struct musb *p) {
    assert(p->lock && y2_usb_detached && !(mac[1]&0x40) && y2_live.stage==Y2_USB_DETACHED);
    ++disconnected;
}
static void musb_stop(struct musb *p) {assert(p->lock && disconnected);++stopped;}
static void musb_start(struct musb *p) {
    assert(p->lock && y2_usb_detached && phy[0x6c]==0x2e && phy[0x6d]==0x3e);
    ++started;if(fifo_bad)y2_live.result=-5;
}
static unsigned y2_musb_clearb(void *p,unsigned r) {(void)p;(void)r;++cleared;return 0;}
static unsigned y2_musb_clearw(void *p,unsigned r) {return y2_musb_clearb(p,r);}
'''
        source += write_callback + lifecycle + r'''
static void reset(void) {
    memset(mac,0,sizeof(mac));memset(phy,0,sizeof(phy));
    y2_live=(struct y2_usb_live){0};y2_power=(struct y2_platform_snapshot){0};
    y2_usb_supply=(struct y2_pwrap_snapshot){.magic=Y2_PWRAP_MAGIC,.valid=7,.cid=0x2023,.vusb=0xc000,.chrdet=0x21};
    y2_session=(struct y2_session){.before_c=0x12,.before_d=0,.written=1};
    phy[0x6c]=0x2e;phy[0x6d]=0x3e;phy[0x6b]=2;
    y2_power.wake.after[3]=2;mac[0x60]=0x80;mac[1]=0x60;
    disconnected=stopped=started=clock_bad=session_bad=fifo_bad=cleared=0;y2_usb_detached=false;
    y2_usb_detach();
    assert(disconnected==1 && stopped==1 && y2_usb_detached && !y2_session.written);
    assert(phy[0x6c]==0x12 && !phy[0x6d] && !(mac[1]&0x40));
    /* A delayed upstream gadget pullup cannot reconnect while absent. */
    y2_musb_writeb(mac,1,0x60);assert(mac[1]==0x20);
    y2_musb_writeb(mac,0x60,3);assert(!mac[0x60]);mac[0x60]=0x80;
}
int main(void) {
    for(unsigned fault=0;fault<29;++fault) {
        reset();
        if(fault==1)clock_bad=1;
        if(fault==2)y2_usb_supply.chrdet=1;
        if(fault==3)phy[0x6c]^=1;
        if(fault==4)phy[0x6d]=1;
        if(fault==5)mac[0x60]=0x81;
        if(fault>=6 && fault<13)phy[y2_usb_mode_offsets[fault-6]]^=1;
        if(fault>=13 && fault<21)mac[0x204+16*(fault-13)]=1;
        if(fault==21)session_bad=1;
        if(fault==22)fifo_bad=1;
        if(fault==23)y2_usb_supply.valid=3;
        if(fault>=24) {unsigned offsets[]={0,1,2,3,6};phy[0x68+offsets[fault-24]]^=1;}
        int rc=y2_usb_reconnect();
        if(!fault) {
            assert(!rc && started==1 && cleared==3 && !y2_usb_detached);
            assert(mac[1]&0x40);assert(y2_live.stage==Y2_USB_READY);
        } else {
            assert(rc<0 && y2_usb_detached && !(mac[1]&0x40));
            assert(started==(fault==22));
            if(fault!=21 && fault!=22)assert(!y2_session.written);
        }
    }
}
'''
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            (p / 'test.c').write_text(source)
            subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror',
                            '-I' + str(ROOT / 'kernel/usb'), str(p / 'test.c'),
                            '-o', str(p / 'test')], check=True)
            subprocess.run([str(p / 'test')], check=True)
