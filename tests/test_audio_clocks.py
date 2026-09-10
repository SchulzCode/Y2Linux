"""Exercise actual CCF audio callbacks: preserve SD mux and inherited gates."""
from pathlib import Path
import subprocess, tempfile, unittest
from test_audio import function
ROOT=Path(__file__).resolve().parents[1]

class AudioClocks(unittest.TestCase):
    def test_audio_mux_masks(self):
        source=(ROOT/'kernel/platform/clocks.c').read_text()
        fixture=r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include "clocks.h"
#define BIT(n) (1U<<(n))
struct clk_hw {int unused;};
struct y2_clock {struct clk_hw hw; void *top; unsigned id; bool inherited;};
#define container_of(p,t,m) ((t *)((char *)(p)-offsetof(t,m)))
#define spin_lock_irqsave(p,f) do{(void)p;f=0;}while(0)
#define spin_unlock_irqrestore(p,f) do{(void)p;(void)f;}while(0)
static int y2_clk_lock;
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){*(unsigned *)p=v;}
'''
        fixture+=''.join(function(source,n) for n in ('y2_audio_enable','y2_audio_disable','y2_audio_enabled'))
        fixture+=r'''
int main(void) {
 unsigned regs[64];struct y2_clock c={.top=regs};
 for(unsigned id=Y2_CLK_AUDINTBUS;id<=Y2_CLK_AUDIO;id++)for(unsigned v=0;v<256;v++) {
  unsigned old=0xa580abcdU^(v<<24)^(v<<16),mask=id==Y2_CLK_AUDIO?0x00810000:0x87000000;
  c.id=id;c.inherited=false;regs[0x70/4]=old;
  assert(!y2_audio_enable(&c.hw));assert(regs[0x70/4]==(old&~mask));
  assert((regs[0x70/4]&0xffff)==(old&0xffff)); /* both adjacent SD mux fields */
  assert(y2_audio_enabled(&c.hw));
  y2_audio_disable(&c.hw);assert(!y2_audio_enabled(&c.hw));
  assert(!y2_audio_enable(&c.hw));c.inherited=true;y2_audio_disable(&c.hw);
  assert(y2_audio_enabled(&c.hw));
 }
}
'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.c').write_text(fixture)
            subprocess.run(['clang','-O2','-Wall','-Wextra','-Werror','-I'+str(ROOT/'kernel/platform'),str(p/'test.c'),'-o',str(p/'test')],check=True)
            subprocess.run([str(p/'test')],check=True)
