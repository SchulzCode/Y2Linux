"""Exercise the production USB worker/status failure path without hardware."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class UsbFailure(unittest.TestCase):
    def test_poll_failure_survives_teardown(self):
        adapter = (ROOT / 'kernel/usb/y2_musb.c').read_text()
        worker = adapter[adapter.index('static void y2_usb_worker(struct work_struct *work)\n{'):
                         adapter.index('static void y2_usb_begin(void)\n{')]
        status = adapter[adapter.index('static ssize_t y2_usb_status(char __user *buf)\n{'):]
        source = r'''
#include <assert.h>
#include <string.h>
#include <stddef.h>
#include "live.h"
#define __user
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x,v) ((x)=(v))
#define ENODEV 19
#define EFAULT 14
#define MUSB_DEVCTL 0x60
#define USB_STATE_CONFIGURED 7
typedef long ssize_t;
struct work_struct { int unused; };
struct musb { unsigned char *mregs; struct {unsigned state;} g; };
static struct musb *y2_musb;
static struct y2_usb_live y2_live;
static struct y2_pwrap_snapshot next_power,y2_usb_supply;
static void *y2_usb_pmic;
static int y2_usb_child,y2_usb_finished,y2_usb_failure_lock,y2_usb_work;
static unsigned y2_usb_detached,y2_usb_detaches,detached,reconnected;
static int reconnect_rc;
static unsigned jiffies,y2_usb_deadline=50000,probes,scheduled,finished,registered;
static int time_after_eq(unsigned a,unsigned b) {return a>=b;}
static unsigned msecs_to_jiffies(unsigned ms) {return ms;}
static void spin_lock(int *lock) {assert(!*lock);*lock=1;}
static void spin_unlock(int *lock) {assert(*lock);*lock=0;}
static unsigned y2_power_read(void *p,unsigned r) {(void)p;(void)r;assert(0);return 0;}
static void y2_power_write(void *p,unsigned r,unsigned v) {(void)p;(void)r;(void)v;assert(0);}
static void y2_power_delay(void *p) {(void)p;assert(0);}
/* Protocol faults themselves are covered by test_pwrap; inject its result here. */
static void fake_probe(const struct y2_pwrap_io *io,struct y2_pwrap_snapshot *out)
{assert(io->context==y2_usb_pmic);++probes;*out=next_power;}
#define y2_pwrap_probe fake_probe
static void y2_usb_fail(int rc) {if(!y2_live.result)y2_live.result=rc;}
static void y2_usb_phase(unsigned stage) {y2_live.stage=stage;}
static int y2_usb_register(void) {++registered;return 0;}
static unsigned readb(void *p) {(void)p;return 0x98;}
static void y2_usb_detach(void) {++detached;y2_usb_detached=1;y2_usb_phase(Y2_USB_DETACHED);}
static int y2_usb_reconnect(void) {++reconnected;if(!reconnect_rc)y2_usb_detached=0;return reconnect_rc;}
static void schedule_delayed_work(int *work,unsigned delay)
{assert(work==&y2_usb_work && delay==250);++scheduled;}
static void y2_usb_finish(void) {++finished;y2_usb_finished=1;y2_usb_pmic=NULL;}
static int copy_to_user(void *dest,const void *src,unsigned n)
{assert(!y2_usb_failure_lock);memcpy(dest,src,n);return 0;}
'''
        source += worker + status + r'''
int main(void) {
    assert(sizeof(struct y2_usb_live)==88);
    for(unsigned fault=0;fault<5;++fault) {
        y2_live=(struct y2_usb_live){.magic=Y2_USB_LIVE_MAGIC,.stage=Y2_USB_ATTACH,.devctl=0x100};
        next_power=(struct y2_pwrap_snapshot){.magic=Y2_PWRAP_MAGIC,.valid=7,
            .wrap=1,.arb=0x1ff,.channel=1,.init=1,.before=0x00300001,.after=0x00300001,
            .cid=0x2023,.vusb=0xc000,.chrdet=1};
        y2_usb_pmic=&next_power;y2_usb_finished=0;
        probes=scheduled=finished=registered=0;
        struct y2_usb_live status;
        assert(y2_usb_status((char*)&status)==sizeof(status));
        assert(!status.power_failure.magic); /* no invented failure before polling */
        if(fault==1) {next_power.result=-16;next_power.valid=0;next_power.before=next_power.after=0x00200001;}
        if(fault==2) {next_power.result=-16;next_power.valid=0;next_power.before=next_power.after=0x00360001;}
        if(fault==3) {next_power.result=-110;next_power.valid=3;next_power.after=0x00340023;}
        if(fault==4) next_power.vusb=0x4000;
        y2_usb_worker(NULL);
        assert(probes==1 && !registered);
        assert(y2_usb_status((char*)&status)==sizeof(status));
        assert(status.magic==Y2_USB_LIVE_MAGIC && status.polls==1 && status.stage==Y2_USB_ATTACH);
        if(!fault) {
            assert(!finished && scheduled==1 && !status.result && status.chrdet==1);
            assert(!status.power_failure.magic);
        } else {
            assert(finished==1 && !scheduled && !y2_usb_pmic);
            assert(status.chrdet==0x10000 && status.result==(fault==4 ? -ENODEV : next_power.result));
            assert(!memcmp(&status.power_failure,&next_power,sizeof(next_power)));
            y2_usb_worker(NULL); /* teardown is terminal; do not probe/retry */
            assert(probes==1 && finished==1);
            assert(y2_usb_status((char*)&status)==sizeof(status));
            assert(!memcmp(&status.power_failure,&next_power,sizeof(next_power)));
        }
    }
    /* Execute the production worker through late attach, one detach, waiting,
     * reconnect, second detach, deadline and failed re-entry. */
    for(unsigned fault=0;fault<3;++fault) {
        unsigned char regs[256]={0};struct musb musb={.mregs=regs,.g={.state=7}};
        y2_live=(struct y2_usb_live){.magic=Y2_USB_LIVE_MAGIC};
        next_power=(struct y2_pwrap_snapshot){.magic=Y2_PWRAP_MAGIC,.valid=7,.vusb=0xc000,.chrdet=0x21};
        y2_usb_pmic=&next_power;y2_usb_child=1;y2_musb=&musb;y2_usb_finished=0;
        y2_usb_detached=y2_usb_detaches=detached=reconnected=0;jiffies=0;finished=0;
        reconnect_rc=fault==2 ? -19 : 0;
        y2_usb_worker(NULL);assert(y2_live.stage==Y2_USB_CONFIGURED);
        next_power.chrdet=1;y2_usb_worker(NULL);
        assert(detached==1 && y2_usb_detaches==1 && !finished && y2_live.stage==Y2_USB_DETACHED);
        for(unsigned i=0;i<20;++i) y2_usb_worker(NULL);
        assert(detached==1 && !reconnected && !finished); /* no repeated teardown */
        if(fault==1) jiffies=y2_usb_deadline;
        next_power.chrdet=0x21;y2_usb_worker(NULL);
        if(fault) {assert(finished==1 && reconnected==(fault==2));continue;}
        assert(reconnected==1 && !y2_usb_detached && !finished && y2_live.stage==Y2_USB_CONFIGURED);
        next_power.chrdet=1;y2_usb_worker(NULL);
        assert(finished==1 && detached==1 && reconnected==1);
    }
}
'''
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            (path / 'test.c').write_text(source)
            subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror', '-Wno-unused-parameter',
                            '-I' + str(ROOT / 'kernel/usb'), str(path / 'test.c'),
                            '-o', str(path / 'test')], check=True)
            subprocess.run([str(path / 'test')], check=True)
