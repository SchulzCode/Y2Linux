"""Exercise the patched controller dispatch, below the APT32F input driver."""
from pathlib import Path
import json, subprocess, tempfile, unittest
ROOT=Path(__file__).resolve().parents[1]
class CombinedI2C(unittest.TestCase):
    def test_old_controller_uses_one_wrrd_without_retries(self):
        spec=next(s for s in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays'] if s['path']=='drivers/i2c/busses/i2c-mt65xx.c')
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);source=d/'driver.c'
            source.write_bytes((ROOT/'.cache/sources/linux-6.18'/spec['path']).read_bytes())
            subprocess.run(['patch','--batch','--fuzz=0',str(source)],input=(ROOT/'kernel/patches'/spec['patch']).read_bytes(),check=True,stdout=subprocess.PIPE)
            s=source.read_text();body=s[s.index('static int mtk_i2c_transfer('):s.index('static irqreturn_t mtk_i2c_irq(')]
            code=r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
#define I2C_M_RD 1
#define I2C_MT65XX_CLK_MAX 3
#define I2C_MAX_FAST_MODE_PLUS_FREQ 1000000
#define EINVAL 22
#define dev_dbg(...) ((void)0)
enum { I2C_MASTER_WR=1, I2C_MASTER_RD, I2C_MASTER_WRRD };
struct device {bool of_node;};
struct compat {int auto_restart;};
struct mtk_i2c {struct compat *dev_comp;struct device *dev;int clocks[3],auto_restart,ignore_restart_irq,op,speed_hz;};
struct i2c_adapter {struct mtk_i2c *i2c;};
struct i2c_msg {int flags,addr,len;unsigned char *buf;};
static int enabled,disabled,calls,ops[4],lefts[4],failure;
static struct mtk_i2c *i2c_get_adapdata(struct i2c_adapter *a){return a->i2c;}
static int of_device_is_compatible(bool node,const char *name){assert(!strcmp(name,"mediatek,mt6582-i2c"));return node;}
static int clk_bulk_enable(int n,int *c){(void)c;assert(n==3);enabled++;return 0;}
static void clk_bulk_disable(int n,int *c){(void)c;assert(n==3);disabled++;}
static int mtk_i2c_do_transfer(struct mtk_i2c *i,struct i2c_msg *m,int n,int left){assert(m->buf && n>=1);ops[calls]=i->op;lefts[calls++]=left;return failure;}
'''+body+r'''
int main(void){
 struct compat data={0};struct device dev={true};
 struct mtk_i2c i={.dev_comp=&data,.dev=&dev,.speed_hz=100000};struct i2c_adapter a={&i};
 unsigned char reg=0,reply[9]={0};struct i2c_msg m[2]={{0,0x51,1,&reg},{I2C_M_RD,0x51,9,reply}};
 assert(mtk_i2c_transfer(&a,m,2)==2);
 assert(calls==1 && ops[0]==I2C_MASTER_WRRD && lefts[0]==0 && !i.auto_restart);
 assert(enabled==1 && disabled==1 && reg==0);
 calls=0;failure=-110;
 assert(mtk_i2c_transfer(&a,m,2)==-110 && calls==1 && ops[0]==I2C_MASTER_WRRD);
 assert(enabled==2 && disabled==2); /* no retries; clocks balanced on failure */
 calls=0;failure=0;
 assert(mtk_i2c_transfer(&a,m,1)==1 && calls==1 && ops[0]==I2C_MASTER_WR);
 calls=0;dev.of_node=false;data.auto_restart=1;
 assert(mtk_i2c_transfer(&a,m,2)==2 && calls==1 && ops[0]==I2C_MASTER_WRRD);
 calls=0;data.auto_restart=0;
 assert(mtk_i2c_transfer(&a,m,2)==2 && calls==2); /* unrelated compatibles unchanged */
 calls=0;dev.of_node=true;m[0].flags=I2C_M_RD;
 assert(mtk_i2c_transfer(&a,m,2)==2 && calls==2 && ops[0]==I2C_MASTER_RD);
 calls=0;m[0].buf=NULL;
 assert(mtk_i2c_transfer(&a,m,2)==-EINVAL && calls==0 && enabled==disabled);
}
'''
            (d/'test.c').write_text(code)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror',str(d/'test.c'),'-o',str(d/'test')],check=True)
            subprocess.run([str(d/'test')],check=True)
