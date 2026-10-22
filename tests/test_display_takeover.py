"""Execute the first-only ownership transaction, including dropped stop writes."""
import json
from pathlib import Path
import unittest
from test_power import run_c
from tools.build.run import prepare_overlay

ROOT = Path(__file__).resolve().parents[1]


class DisplayTakeover(unittest.TestCase):
    def test_loader_stop_clock_failure_and_no_live_reset(self):
        manifest = json.loads((ROOT/'kernel/patches/manifest.json').read_text())
        entry = next(e for e in manifest['overlays'] if e['path'].endswith('/mtk_disp_ovl.c'))
        source = prepare_overlay(ROOT, entry).read_text()
        start = source.index('int mtk_ovl_clk_enable(')
        end = source.index('\nvoid mtk_ovl_clk_disable', start)
        real = source[start:end]
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <errno.h>
#define DISP_REG_OVL_INTEN 4
#define DISP_REG_OVL_EN 12
#define DISP_REG_OVL_SRC_CON 44
struct data {bool mt6582;};
struct mtk_disp_ovl {bool y2_owned;struct data *data;int clk;void *regs;};
struct device {struct mtk_disp_ovl *data;};
static unsigned r[16], writes, disabled;static int failure, drop;
static void *dev_get_drvdata(struct device *d){return d->data;}
static int clk_prepare_enable(int c){return failure;}
static void clk_disable_unprepare(int c){disabled++;}
static void writel(unsigned v,void *p){writes++;if(!drop)*(unsigned *)p=v;}
static unsigned readl(void *p){return *(unsigned *)p;}
''' + real + r'''
int main(void){
 struct data data={true};struct mtk_disp_ovl ovl={.data=&data,.regs=r};struct device d={&ovl};
 r[1]=2;r[3]=1;r[11]=15;
 failure=-EIO;assert(mtk_ovl_clk_enable(&d)==-EIO && !writes && !ovl.y2_owned);
 failure=0;drop=1;assert(mtk_ovl_clk_enable(&d)==-EIO && disabled==1 && !ovl.y2_owned);
 drop=0;assert(!mtk_ovl_clk_enable(&d) && ovl.y2_owned && !r[1] && !r[3] && !r[11]);
 unsigned count=writes;
 r[3]=1;r[11]=1;assert(!mtk_ovl_clk_enable(&d) && writes==count && r[3]==1 && r[11]==1);
 data.mt6582=false;ovl.y2_owned=false;
 assert(!mtk_ovl_clk_enable(&d) && writes==count);
}
''')


class ScanoutLog(unittest.TestCase):
    def test_scanout_line_only_on_layout_change_or_mismatch(self):
        # F30: a line per frame filled /data/logs within minutes and rotated reports' evidence away.
        manifest = json.loads((ROOT/'kernel/patches/manifest.json').read_text())
        entry = next(e for e in manifest['overlays'] if e['path'].endswith('/mtk_disp_ovl.c'))
        source = prepare_overlay(ROOT, entry).read_text()
        block = source[source.index('One line per new layout'):source.index('"Y2SCAN FB format=')]
        self.assertIn('if (live == addr && fmt == y2_last_fmt && size == y2_last_size &&\n'
                      '            pending->pitch == y2_last_pitch)\n            return;', block)
        self.assertEqual(source.count('Y2SCAN FB'), 1)
