"""Execute the real APT32F handler against transport and malformed-frame fixtures."""
import re, subprocess, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class WheelRegression(unittest.TestCase):
    def test_register_zero_restart_and_bounded_failure(self):
        source=(ROOT/'kernel/platform/apt32f-wheel.c').read_text()
        core=source[source.index('#define APT32F_FRAME_LEN'):source.index('static int apt32f_probe')]
        shim=r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
typedef unsigned char u8;
typedef int irqreturn_t;
#define IRQ_HANDLED 1
#define I2C_M_RD 1
#define KEY_UP 103
#define KEY_PAGEUP 104
#define KEY_DOWN 108
#define KEY_PAGEDOWN 109
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define dev_warn_ratelimited(...) ((void)0)
struct i2c_client {unsigned addr;void *adapter;int dev;};
struct input_dev {int unused;};
struct i2c_msg {unsigned addr,flags,len;u8 *buf;};
static unsigned calls,events,syncs,last_key;static int result=2;
static u8 packet[9]={0xaa,0x55,3,0,1,0,0,0,0};
static int i2c_transfer(void *adap,struct i2c_msg *m,int count) {
 (void)adap;calls++;assert(count==2 && m[0].addr==0x51 && m[1].addr==0x51);
 assert(!m[0].flags && m[0].len==1 && m[0].buf[0]==0);
 assert(m[1].flags==I2C_M_RD && m[1].len==9);
 memcpy(m[1].buf,packet,9);return result;
}
static void input_report_key(struct input_dev *dev,unsigned key,int value) {
 (void)dev;assert(value==(events%2==0));events++;last_key=key;
}
static void input_sync(struct input_dev *dev) {(void)dev;syncs++;}
'''
        test=r'''
int main(void) {
 struct i2c_client client={.addr=0x51};struct input_dev input={0};
 struct apt32f_wheel wheel={&client,&input};
 result=-ETIMEDOUT;
 for(unsigned i=0;i<8;i++) assert(apt32f_irq(55,&wheel)==IRQ_HANDLED);
 assert(calls==8 && !events); /* no handler retries or false scroll */
 result=1;apt32f_irq(55,&wheel);assert(!events);
 result=2;
 for(unsigned code=1;code<=4;code++){packet[4]=code;apt32f_irq(55,&wheel);assert(last_key==apt32f_scroll_keys[code]);}
 assert(events==8 && syncs==8);
 packet[0]=0;apt32f_irq(55,&wheel);assert(events==8);packet[0]=0xaa;
 packet[1]=0;apt32f_irq(55,&wheel);assert(events==8);packet[1]=0x55;
 packet[2]=1;apt32f_irq(55,&wheel);assert(events==8);packet[2]=3;
 packet[4]=9;apt32f_irq(55,&wheel);assert(events==8);
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'test.c').write_text(shim+core+test)
            subprocess.run(['clang','-Wall','-Wextra','-Werror','-Wno-unused-parameter','-O2',str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
