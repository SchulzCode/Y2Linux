"""CPU Final Fix02 regressions. Synthetic evidence, never a device receipt."""
from pathlib import Path
import json
import re
import unittest
from test_audio import function
from test_power import run_c
from tools.build.run import prepare_overlay
ROOT = Path(__file__).resolve().parents[1]


def overlay(path):
    spec = next(x for x in json.loads((ROOT/'kernel/patches/manifest.json').read_text())['overlays'] if x['path'] == path)
    return prepare_overlay(ROOT, spec).read_text()


def public(source, *names):
    """Expose named non-static kernel functions to the host extractor."""
    for name in names:
        source = re.sub(r'^(int|void|bool|unsigned) ' + name + r'\(', r'static \1 ' + name + '(', source, flags=re.M)
    return source


PWRAP_FIXTURE = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
#include "pwrap-readiness-policy.h"
#define BIT(n) (1U<<(n))
#define EPROBE_DEFER 517
struct regmap { int unused; };
struct y2_wrap { unsigned char *base; struct regmap *map; };
static unsigned mmio[0x200/4], pmic[0x400/2], writes, spm_requests, faults, fail_read, fail_spm, feedback_lag;
static struct regmap map;
static struct y2_wrap wrap = { (unsigned char *)mmio, &map }, *y2_wrap = &wrap;
static int y2_wrap_lock, cpu_voltage_error, cpu_voltage_first_error;
static bool cpu_dvfs_prepared;
static unsigned cpu_selector_address;
static const char *cpu_voltage_stage = "not_attempted", *cpu_voltage_first_stage = "none";
static unsigned ready_mux, ready_wrap, ready_arb;
static const char *ready_reason = "not_read";
static void mutex_lock(int *l) { assert(!*l); *l = 1; }
static void mutex_unlock(int *l) { assert(*l); *l = 0; }
static unsigned readl(const void *p) { return *(const unsigned *)p; }
static void writel(unsigned v, void *p) { writes++; *(unsigned *)p = v; }
static void udelay(unsigned us) { (void)us; }
static void y2_cpu_dvfs_fault(void) { faults++; }
static int regmap_read(struct regmap *m, unsigned reg, unsigned *value)
{
	(void)m;
	if (fail_read == reg) return -ETIMEDOUT;
	*value = pmic[reg / 2];
	return 0;
}
static int y2_spm_cpu_voltage_request(unsigned slot)
{
	spm_requests++;
	if (fail_spm) return -ETIMEDOUT;
	/* SPM writes the programmed slot to the programmed PMIC bank. */
	unsigned address = mmio[(0xe4 + slot * 8) / 4], value = mmio[(0xe8 + slot * 8) / 4];
	pmic[address / 2] = value;
	if (!feedback_lag) pmic[0x224 / 2] = value;
	return 0;
}
static void board(unsigned arb)
{
	memset(mmio, 0, sizeof(mmio)); memset(pmic, 0, sizeof(pmic));
	mmio[0] = 0; mmio[1] = 1; mmio[0x50 / 4] = arb;
	pmic[0x216 / 2] = 0; pmic[0x21e / 2] = 0x48; pmic[0x224 / 2] = 0x48;
	cpu_dvfs_prepared = false; cpu_selector_address = 0; cpu_voltage_first_error = 0;
	writes = spm_requests = faults = fail_read = fail_spm = feedback_lag = 0;
}
'''


class PwrapReadiness(unittest.TestCase):
    def test_exact_operands_and_reasons(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "pwrap-readiness-policy.h"
int main(void){
 assert(!y2_pwrap_dvfs_unready(0,1,0x7f)); /* physical M2-PWRAP-01 readback */
 /* The preloader's written 0x1ff can never be read from the 7-bit field. */
 assert(!strcmp(y2_pwrap_dvfs_unready(0,1,0x1ff),"arbiter_unknown_bits"));
 assert(!strcmp(y2_pwrap_dvfs_unready(1,1,0x7f),"mux_not_wrapper"));
 assert(!strcmp(y2_pwrap_dvfs_unready(0,0,0x7f),"wrapper_disabled"));
 assert(!strcmp(y2_pwrap_dvfs_unready(0,1,0x6f),"arbiter_dvfs_channel_disabled"));
 assert(!strcmp(y2_pwrap_dvfs_unready(0,1,0x77),"arbiter_wacs2_disabled"));
 assert(!strcmp(y2_pwrap_dvfs_unready(0,1,0x78),"arbiter_not_preloader_complete"));
 assert(!strcmp(y2_pwrap_dvfs_unready(0,1,0x3f),"arbiter_not_preloader_complete"));
 for(unsigned arb=0;arb<0x400;arb++)
  assert(!y2_pwrap_dvfs_unready(0,1,arb)==(arb==0x7f));
}
''')

    def test_admission_voltage_feedback_and_faults_use_real_driver(self):
        s = public((ROOT/'kernel/platform/pwrap.c').read_text(), 'y2_pmic_cpu_dvfs_prepare', 'y2_pmic_cpu_voltage_set')
        assert 'readl(y2_wrap->base + 0x50) != 0x1ff' not in s
        run_c(PWRAP_FIXTURE + function(s, 'wrap_dvfs_ready') + function(s, 'y2_pmic_cpu_selector') +
              function(s, 'y2_pmic_cpu_dvfs_prepare') + function(s, 'y2_pmic_cpu_voltage_set') + r'''
int main(void){
 /* Real board state admits and programs slots 0/1/2 to the active 21e bank. */
 board(0x7f);
 assert(!y2_pmic_cpu_dvfs_prepare() && cpu_dvfs_prepared && cpu_selector_address==0x21e);
 assert(!strcmp(ready_reason,"ready") && ready_arb==0x7f && !faults);
 assert(mmio[0xe4/4]==0x21e && mmio[0xe8/4]==88 && mmio[0xec/4]==0x21e && mmio[0xf0/4]==80 && mmio[0xf4/4]==0x21e && mmio[0xf8/4]==72);
 assert(spm_requests==1 && pmic[0x21e/2]==72); /* baseline handshake at 1.15 V */
 /* Voltage-before-frequency request, with independent NI feedback. */
 assert(!y2_pmic_cpu_voltage_set(80) && pmic[0x21e/2]==80 && pmic[0x224/2]==80);
 assert(!y2_pmic_cpu_voltage_set(88) && pmic[0x224/2]==88);
 assert(!y2_pmic_cpu_voltage_set(72) && pmic[0x224/2]==72);
 assert(y2_pmic_cpu_voltage_set(73)==-EINVAL);
 /* Hardware VOSEL_ON bank is honored without switching mode. */
 board(0x7f); pmic[0x216/2]=2; pmic[0x220/2]=0x48;
 assert(!y2_pmic_cpu_dvfs_prepare() && cpu_selector_address==0x220 && mmio[0xe4/4]==0x220);
 /* Old strict predicate state and every invalid wrapper state refuse admission
  * before any slot write or SPM request. */
 const unsigned bad[]={0x1ff,0x6f,0x77,0};
 for(unsigned i=0;i<4;i++){
  board(bad[i]);
  assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !cpu_dvfs_prepared);
  assert(!spm_requests && !writes && !strcmp(cpu_voltage_first_stage,"pwrap_readiness"));
 }
 board(0x7f); mmio[0]=1; assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !strcmp(ready_reason,"mux_not_wrapper"));
 board(0x7f); mmio[1]=0; assert(y2_pmic_cpu_dvfs_prepare()==-EOPNOTSUPP && !strcmp(ready_reason,"wrapper_disabled"));
 /* Feedback mismatch: selector bank written but NI feedback does not follow. */
 board(0x7f); feedback_lag=1; pmic[0x224/2]=0x48;
 assert(!y2_pmic_cpu_dvfs_prepare());
 assert(y2_pmic_cpu_voltage_set(80)==-EIO && !strcmp(cpu_voltage_stage,"voltage_spm_handshake_and_readback"));
 /* Failed SPM voltage request is an error, never a silent success. */
 board(0x7f); assert(!y2_pmic_cpu_dvfs_prepare()); fail_spm=1;
 assert(y2_pmic_cpu_voltage_set(80)==-ETIMEDOUT && pmic[0x21e/2]==72);
 /* Failed baseline handshake latches the DVFS fault (high OPP off until reboot). */
 board(0x7f); fail_spm=1; assert(y2_pmic_cpu_dvfs_prepare()==-ETIMEDOUT && faults==1 && !cpu_dvfs_prepared);
 /* Wrapper state changing after admission refuses the request as uncertain. */
 board(0x7f); assert(!y2_pmic_cpu_dvfs_prepare()); mmio[0x50/4]=0x6f;
 unsigned before=spm_requests;
 assert(y2_pmic_cpu_voltage_set(80)==-EIO && spm_requests==before && !strcmp(ready_reason,"arbiter_dvfs_channel_disabled"));
 /* Slot tampering is detected before an SPM request. */
 board(0x7f); assert(!y2_pmic_cpu_dvfs_prepare()); mmio[0xe8/4]=0x5a; before=spm_requests;
 assert(y2_pmic_cpu_voltage_set(88)==-EIO && spm_requests==before);
 /* Unprepared requests and bank changes refuse. */
 board(0x7f); assert(y2_pmic_cpu_voltage_set(80)==-EACCES);
 board(0x7f); assert(!y2_pmic_cpu_dvfs_prepare()); pmic[0x216/2]=2; pmic[0x220/2]=0x48;
 assert(y2_pmic_cpu_voltage_set(80)==-EIO);
 /* Read failures propagate. */
 board(0x7f); fail_read=0x224; assert(y2_pmic_cpu_dvfs_prepare()==-ETIMEDOUT && !cpu_dvfs_prepared);
 /* Non-baseline inherited voltage is never admitted. */
 board(0x7f); pmic[0x21e/2]=80; pmic[0x224/2]=80; assert(y2_pmic_cpu_dvfs_prepare()==-EBUSY);
}
''')

    def test_transition_ordering_rollback_and_thermal_cap(self):
        run_c(r'''
#include <assert.h>
#include <errno.h>
#include "cpu-dvfs-policy.h"
static int volts=72, fail_up, fail_down, fail_clock, readback_low, faults, log_n;
static unsigned long hz=1040000000; static char log_[16];
static int vget(void*c){return readback_low?60:volts;}
static int vset(void*c,unsigned s){log_[log_n++]='V';if(s>(unsigned)volts&&fail_up)return -EIO;if(s<(unsigned)volts&&fail_down)return -EIO;volts=s;return 0;}
static int cset(void*c,unsigned long f){log_[log_n++]='C';if(fail_clock&&f!=598000000)return -EIO;/* voltage must already cover it */assert(y2_cpu_selector(f)<=(unsigned)volts||f==598000000);hz=f;return 0;}
static unsigned long cget(void*c){return hz;}
static void fault(void*c){faults++;}
static struct y2_dvfs_io io={0,vget,vset,cset,cget,fault};
int main(void){
 /* Increase: voltage then clock. Decrease: clock then voltage. */
 log_n=0;assert(!y2_dvfs_transition(&io,1300000000,1300000000)&&volts==88&&hz==1300000000&&log_[0]=='V'&&log_[1]=='C');
 log_n=0;assert(!y2_dvfs_transition(&io,1196000000,1300000000)&&volts==80&&log_[0]=='C'&&log_[1]=='V');
 log_n=0;assert(!y2_dvfs_transition(&io,598000000,1300000000)&&volts==72&&hz==598000000);
 /* Thermal/fault ceiling: requests above the admitted maximum are refused. */
 assert(y2_dvfs_transition(&io,1300000000,1040000000)==-EINVAL&&hz==598000000&&!faults);
 /* Failed voltage request: clock never raised, fault latched. */
 hz=1040000000;fail_up=1;assert(y2_dvfs_transition(&io,1300000000,1300000000)==-EIO&&hz==1040000000&&volts==72&&faults==1);
 fail_up=0;faults=0;
 /* Failed clock transition keeps the raised voltage (safe) and latches fault. */
 fail_clock=1;assert(y2_dvfs_transition(&io,1196000000,1300000000)==-EIO&&hz==1040000000&&volts==80&&faults==1);
 fail_clock=0;faults=0;volts=72;hz=1040000000;
 /* Failed decrease with unknown readback: restore old selector, contain. */
 assert(!y2_dvfs_transition(&io,1300000000,1300000000));
 fail_down=1;assert(!y2_dvfs_transition(&io,1040000000,1300000000)&&hz==1040000000&&volts==88&&faults==1);
 fail_down=0;faults=0;readback_low=1;volts=88;hz=1300000000;
 assert(y2_dvfs_transition(&io,1040000000,1300000000)==-ERANGE); /* unknown readback refuses */
}
''')


MSDC_FIXTURE = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <string.h>
typedef unsigned u32;
#define BIT(n) (1U<<(n))
#define READ_ONCE(x) (x)
#define MSDC_NR_CLOCKS 3
#include "msdc-rpm-policy.h"
struct device { int unused; };
struct clk { int count; };
struct clk_bulk_data { struct clk *clk; };
struct mmc_host { int unused; };
struct msdc_host {
	unsigned char *base; void *mrq; bool hsq_en; struct device *dev;
	struct clk *h_clk, *bus_clk, *src_clk, *src_clk_cg, *crypto_clk;
	struct clk_bulk_data bulk_clks[MSDC_NR_CLOCKS];
	u32 y2_rpm_saved[Y2_MSDC_RETAINED]; bool y2_rpm_gated;
	u32 y2_rpm_suspends, y2_rpm_resumes, y2_rpm_retained, y2_rpm_restored;
	u32 y2_rpm_refused[Y2_MSDC_BUSY_REASONS]; u32 y2_rpm_last_mismatch; int y2_rpm_error;
};
static unsigned regs[0x100 / 4], hsq_suspended, hsq_resumed, lose_state, stuck_state, no_ckstb, clk_fail, gated_access;
static struct clk gate;	/* hclk and source_cg are the same PERI MSDC CG */
static struct clk source;
static struct device dev;
static struct mmc_host mmc;
static struct msdc_host host = { .base = (unsigned char *)regs, .hsq_en = true, .dev = &dev, .h_clk = &gate, .src_clk = &source, .src_clk_cg = &gate };
static int clk_prepare_enable(struct clk *c) { if (!c) return 0; if (clk_fail && c == &source) return -EIO; c->count++; if (c == &gate && c->count == 1 && lose_state) { for (unsigned i = 1; i < Y2_MSDC_RETAINED; i++) regs[y2_msdc_retained[i] / 4] = 0; } return 0; }
static void clk_disable_unprepare(struct clk *c) { if (c) { assert(c->count > 0); c->count--; } }
static int clk_bulk_prepare_enable(int n, struct clk_bulk_data *b) { return 0; }
static void clk_bulk_disable_unprepare(int n, struct clk_bulk_data *b) { }
static unsigned readl(const void *p) { if (!gate.count) gated_access++; unsigned o = (const unsigned char *)p - host.base; if (o == 0 && !no_ckstb) return regs[0] | Y2_MSDC_CFG_CKSTB; return regs[o / 4]; }
static void writel(unsigned v, void *p) { if (!gate.count) gated_access++; unsigned o = (unsigned char *)p - host.base; if (stuck_state && o == 0xec) return; regs[o / 4] = v; }
static void udelay(int us) { }
static __attribute__((unused)) void mmc_hsq_suspend(struct mmc_host *m) { hsq_suspended++; }
static int mmc_hsq_resume(struct mmc_host *m) { hsq_resumed++; return 0; }
#define dev_err(...) ((void)0)
#define readl_poll_timeout(addr, val, cond, d, t) ({ int __r = 0; (val) = readl(addr); if (!(cond)) __r = -ETIMEDOUT; __r; })
static void msdc_gate_clock(struct msdc_host *host)
{
	clk_bulk_disable_unprepare(MSDC_NR_CLOCKS, host->bulk_clks);
	clk_disable_unprepare(host->crypto_clk);
	clk_disable_unprepare(host->src_clk_cg);
	clk_disable_unprepare(host->src_clk);
	clk_disable_unprepare(host->bus_clk);
	clk_disable_unprepare(host->h_clk);
}
static void reset_board(void)
{
	memset(regs, 0, sizeof(regs));
	regs[0] = 0x00000a01;	/* SD/MMC mode, divider 0x0a */
	regs[0x04/4] = 0x11; regs[0x10/4] = 0x80000f00; regs[0x30/4] = 0x00403f00;
	regs[0xb0/4] = 0x403c004f; regs[0xb4/4] = 0xffff0089; regs[0xec/4] = 0x00001234;
	gate.count = 2; source.count = 1; host.y2_rpm_gated = 0; host.mrq = 0;
	lose_state = stuck_state = no_ckstb = clk_fail = gated_access = 0;
}
'''


class MsdcRuntimeOwnership(unittest.TestCase):
    def source(self):
        return overlay('drivers/mmc/host/mtk-sd.c')

    def test_runtime_callbacks_no_longer_bypass_clock_ownership(self):
        s = self.source()
        runtime = function(s, 'msdc_runtime_suspend') + function(s, 'msdc_runtime_resume')
        self.assertIn('return y2_msdc_runtime_suspend(dev, mmc, host);', runtime)
        self.assertIn('return y2_msdc_runtime_resume(mmc, host);', runtime)
        self.assertNotIn('return 0;\n\t}\n\n\tmsdc_save_reg', runtime)
        # System sleep still reaches the same guarded owner through force_suspend.
        self.assertIn('return pm_runtime_force_suspend(dev);', function(s, 'msdc_suspend'))
        # Card-level retention (no CMD5/poweroff notify/reinit) remains in mmc core.
        core = overlay('drivers/mmc/core/mmc.c')
        self.assertIn('Y2 s2idle retains card power', core)
        # The gated path never commands, resets or re-powers the card/controller.
        owner = function(s, 'y2_msdc_runtime_suspend') + function(s, 'y2_msdc_runtime_resume')
        for forbidden in ('mmc_send', 'mmc_power', 'msdc_reset', 'MSDC_CFG_RST |', 'regulator', 'mmc_hw_reset', 'msdc_init_hw', 'pinctrl'):
            self.assertNotIn(forbidden, owner)
        # Sibling-SoC-only registers are not in the MT6582 image.
        policy = (ROOT/'kernel/platform/msdc-rpm-policy.h').read_text()
        table = policy[policy.index('y2_msdc_retained[Y2_MSDC_RETAINED] = {'):policy.index('};')]
        for absent in ('0xb8', '0x188', '0x18c', '0x208', '0x220', '0x228'):
            self.assertNotIn(absent, table)

    def test_busy_idle_gate_restore_and_failure(self):
        s = self.source()
        run_c(MSDC_FIXTURE + function(s, 'msdc_ungate_clock_only') + function(s, 'y2_msdc_runtime_suspend') +
              function(s, 'y2_msdc_runtime_resume') + r'''
int main(void){
 assert(y2_msdc_rpm_busy(0,0,0,0,0,0)==Y2_MSDC_IDLE);
 /* Active request, DMA, controller, FIFO and pending interrupt block the gate. */
 struct { unsigned off, value; void *mrq; enum y2_msdc_rpm_busy why; } busy[] = {
  {0,0,&host,Y2_MSDC_BUSY_REQUEST}, {0x9c,1,0,Y2_MSDC_BUSY_DMA}, {0x3c,1,0,Y2_MSDC_BUSY_CONTROLLER},
  {0x3c,2,0,Y2_MSDC_BUSY_CONTROLLER}, {0x14,0x00030000,0,Y2_MSDC_BUSY_FIFO}, {0x14,5,0,Y2_MSDC_BUSY_FIFO},
  {0x0c,0x100,0,Y2_MSDC_BUSY_INTERRUPT} };
 for (unsigned i=0;i<7;i++){
  reset_board(); host.mrq=busy[i].mrq; if(busy[i].off) regs[busy[i].off/4]|=busy[i].value;
  unsigned before=host.y2_rpm_refused[busy[i].why], resumed=hsq_resumed;
  assert(y2_msdc_runtime_suspend(&dev,&mmc,&host)==-EBUSY);
  assert(host.y2_rpm_refused[busy[i].why]==before+1 && gate.count==2 && !host.y2_rpm_gated);
  assert(regs[0]&Y2_MSDC_CFG_MODE && hsq_resumed==resumed+1); /* nothing changed, HSQ rolled back */
 }
 /* Idle: registers saved, MS mode, PERI CG released (both references). */
 reset_board(); unsigned image[Y2_MSDC_RETAINED];
 for(unsigned i=0;i<Y2_MSDC_RETAINED;i++) image[i]=readl(host.base+y2_msdc_retained[i]);
 unsigned suspends=host.y2_rpm_suspends;
 assert(!y2_msdc_runtime_suspend(&dev,&mmc,&host));
 assert(!gate.count && !source.count && host.y2_rpm_gated && host.y2_rpm_suspends==suspends+1);
 assert(!(regs[0]&Y2_MSDC_CFG_MODE) && (regs[0]&0xff00)==0x0a00 && !gated_access);
 assert(!memcmp(image,host.y2_rpm_saved,sizeof(image)));
 /* Resume with retained state: clocks back, SD/MMC mode and divider restored. */
 unsigned retained=host.y2_rpm_retained;
 assert(!y2_msdc_runtime_resume(&mmc,&host));
 assert(gate.count==2 && source.count==1 && !host.y2_rpm_gated && host.y2_rpm_retained==retained+1);
 assert(regs[0]==0x00000a01 && !host.y2_rpm_error);
 for(unsigned i=1;i<Y2_MSDC_RETAINED;i++) assert(regs[y2_msdc_retained[i]/4]==image[i]);
 /* A second resume while active is a no-op for clocks. */
 assert(!y2_msdc_runtime_resume(&mmc,&host) && gate.count==2);
 /* Lost register state is restored from the image and verified. */
 reset_board(); assert(!y2_msdc_runtime_suspend(&dev,&mmc,&host)); lose_state=1;
 unsigned restored=host.y2_rpm_restored;
 assert(!y2_msdc_runtime_resume(&mmc,&host) && host.y2_rpm_restored==restored+1 && host.y2_rpm_last_mismatch);
 for(unsigned i=1;i<Y2_MSDC_RETAINED;i++) assert(regs[y2_msdc_retained[i]/4]==image[i]);
 /* A register that cannot be restored fails the resume with clocks left on. */
 reset_board(); assert(!y2_msdc_runtime_suspend(&dev,&mmc,&host)); lose_state=1; stuck_state=1;
 assert(y2_msdc_runtime_resume(&mmc,&host)==-EIO && host.y2_rpm_error==-EIO && gate.count==2 && host.y2_rpm_gated);
 /* Missing clock stability fails; the clock is not gated behind a live host. */
 reset_board(); assert(!y2_msdc_runtime_suspend(&dev,&mmc,&host)); no_ckstb=1;
 assert(y2_msdc_runtime_resume(&mmc,&host)==-ETIMEDOUT && gate.count==2);
 /* Clock enable failure unwinds exactly. */
 reset_board(); assert(!y2_msdc_runtime_suspend(&dev,&mmc,&host)); clk_fail=1;
 assert(y2_msdc_runtime_resume(&mmc,&host)==-EIO && !gate.count && !source.count && host.y2_rpm_gated);
 /* The RST bit is never replayed from a saved image. */
 assert(!(y2_msdc_cfg_restore(0xffffffff)&(Y2_MSDC_CFG_RST|Y2_MSDC_CFG_CKSTB|Y2_MSDC_CFG_BV18PSS)));
}
''')


COORDINATOR = r'''
#include <assert.h>
#include <string.h>
#include "system-idle-policy.h"
static struct y2_idle_state st;
static unsigned t, online = 4, parks[4096], nparks, restores, pressure_restores_seen;
static int lease, dark = 1, demand;
static enum y2_idle_action step(unsigned busy, unsigned khz)
{
	struct y2_idle_sample in = { t, 250, busy, online, khz, dark, lease, 1, 1, demand };
	enum y2_idle_reason why;
	enum y2_idle_action a = y2_idle_decide(&st, &in, &why);
	demand = 0;
	if (a == Y2_IDLE_PARK) { parks[nparks++] = online - 1; online--; y2_idle_parked(&st, t); }
	if (a == Y2_IDLE_RESTORE) { online = 4; restores++; pressure_restores_seen += why == Y2_IDLE_BURST; y2_idle_restored(&st, t, why == Y2_IDLE_BURST); }
	t += 250;
	return a;
}
/* Measured screen-off background: mostly idle with a quarter-second burst
 * (up to 31%) and a transient 1040-MHz raise every three seconds. */
static __attribute__((unused)) void background(unsigned seconds)
{
	for (unsigned i = 0; i < seconds * 4; i++)
		step(i % 12 == 0 ? 310 : 30, i % 12 == 0 ? 1040000 : 598000);
}
'''


class Coordinator(unittest.TestCase):
    def test_sustained_idle_parks_despite_harmless_bursts_in_order(self):
        run_c(COORDINATOR + r'''
int main(void){
 y2_idle_init(&st, 0);
 background(59); assert(!nparks); /* initial restore hold */
 background(20); assert(!nparks); /* quiet window and averages still settling */
 background(120);
 assert(nparks == 3 && parks[0] == 3 && parks[1] == 2 && parks[2] == 1 && online == 1);
 assert(parks[0] && !restores && st.parked == 3);
 /* Steps are at least five seconds apart: one core at a time. */
 assert(st.longest_quiet_ms >= Y2_SYSTEM_QUIET_MS);
 /* Continued harmless background keeps CPU0 alone (no oscillation). */
 unsigned before = restores; background(300); assert(restores == before && online == 1);
}
''')

    def test_real_demand_restores_and_hysteresis(self):
        run_c(COORDINATOR + r'''
static void park_all(void){ y2_idle_init(&st, t); background(200); assert(online == 1); }
int main(void){
 /* Workload lease: immediate restore, reason lease, then hold. */
 park_all(); lease = 1; assert(step(10, 598000) == Y2_IDLE_RESTORE && online == 4 && st.last_reset == Y2_IDLE_LEASE);
 background(30); assert(online == 4); lease = 0;
 /* Display wake. */
 park_all(); dark = 0; assert(step(10, 598000) == Y2_IDLE_RESTORE && st.last_reset == Y2_IDLE_SCREEN); dark = 1;
 /* Explicit input/workload demand. */
 park_all(); demand = 1; assert(step(10, 598000) == Y2_IDLE_RESTORE && st.last_reset == Y2_IDLE_DEMAND);
 /* Hysteresis: nothing parks during the 60-s restore hold. */
 unsigned n = nparks; background(59); assert(nparks == n && online == 4);
 /* Short saturation (<1 s) on the lone CPU is harmless. */
 park_all(); step(1000, 1040000); step(1000, 1040000); step(1000, 1040000); step(20, 598000);
 assert(online == 1);
 /* One second of saturation is real demand: restore under pressure. */
 unsigned hold = st.hold_ms;
 for (int i = 0; i < 4; i++) step(1000, 1040000);
 assert(online == 4 && st.last_reset == Y2_IDLE_BURST && st.hold_ms == hold * 2 && st.pressure_restores == 1);
}
''')

    def test_no_oscillation_under_recurring_pressure(self):
        run_c(COORDINATOR + r'''
int main(void){
 y2_idle_init(&st, 0);
 unsigned cycles = 0;
 /* Every minute a real 1.5-s burst: parking may happen, but each pressure
  * restore doubles the hold so park/restore cycles become rare. */
 for (unsigned minute = 0; minute < 120; minute++) {
  background(58);
  for (int i = 0; i < 6; i++) step(1000, 1040000);
  cycles = pressure_restores_seen;
 }
 assert(st.hold_ms == Y2_SYSTEM_HOLD_MAX_MS);
 /* Four escalations, then at most one cycle per capped hold + quiet window. */
 assert(cycles <= 4 + (120 * 60000) / (Y2_SYSTEM_HOLD_MAX_MS + Y2_SYSTEM_QUIET_MS) + 1);
 /* A long stable parked period resets the hold on the next restore. */
 background(1200); assert(online == 1);
 dark = 0; step(10, 598000); dark = 1;
 assert(st.hold_ms == Y2_SYSTEM_RESTORE_HOLD_MS);
}
''')

    def test_sustained_load_never_parks(self):
        run_c(COORDINATOR + r'''
int main(void){
 y2_idle_init(&st, 0);
 /* 15% of four cores sustained: above the 10% sustained threshold. */
 for (unsigned i = 0; i < 4 * 600; i++) step(150, 598000);
 assert(!nparks && online == 4);
 /* Sustained high frequency without much load also blocks parking. */
 y2_idle_init(&st, t);
 for (unsigned i = 0; i < 4 * 600; i++) step(40, i % 2 ? 1040000 : 598000);
 assert(!nparks);
 /* Disabled/timer gates always win. */
 struct y2_idle_sample in = { t, 250, 0, 4, 598000, 1, 0, 0, 1, 0 }; enum y2_idle_reason why;
 assert(y2_idle_decide(&st, &in, &why) == Y2_IDLE_WAIT && why == Y2_IDLE_TIMER);
 in.timer = 1; in.allowed = 0; assert(y2_idle_decide(&st, &in, &why) == Y2_IDLE_WAIT && why == Y2_IDLE_DISABLED);
 /* Wrapping millisecond clock. */
 assert(y2_idle_after(5, 0xfffffff0U) && !y2_idle_after(0xfffffff0U, 5));
}
''')

    def test_driver_uses_policy_and_frequency_is_not_demand(self):
        s = (ROOT/'kernel/platform/system-idle.c').read_text()
        freq = function(s, 'frequency_event')
        self.assertNotIn('y2_system_idle_activity', freq)
        self.assertIn('atomic_inc(&frequency_raises)', freq)
        self.assertIn('DECLARE_DEFERRABLE_WORK(sample_work', s)
        self.assertIn('restore_locked(why == Y2_IDLE_BURST)', s)
        # Workload and display restores stay synchronous.
        self.assertIn('ret = y2_system_idle_restore();', (ROOT/'kernel/platform/workload.c').read_text())
        self.assertIn('y2_system_idle_restore();', (ROOT/'kernel/platform/backlight.c').read_text())


class ScreenOffBackground(unittest.TestCase):
    """Userspace sources found for the recurring screen-off bursts."""
    def setUp(self):
        import sys
        import tempfile
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.common import Context
        from y2_platform import service
        self.service = service
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.root = Path(self.temp.name)

    def card(self, present):
        block = self.root/'sys/class/block'
        block.mkdir(parents=True, exist_ok=True)
        target = self.root/'sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:0001/block/mmcblk1'
        target.mkdir(parents=True, exist_ok=True)
        link = block/'mmcblk1'
        if present and not link.is_symlink():
            link.symlink_to(target)
        if not present and link.is_symlink():
            link.unlink()

    def lifecycle(self, value):
        path = self.root/'run/y2/media-lifecycle.json'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))

    def test_media_reconcile_spawns_only_when_its_own_noop_is_false(self):
        from y2_platform.media import inventory
        self.assertTrue(self.service.media_reconcile_needed(self.ctx))  # first run
        self.card(True)
        current = [list(v) for v in inventory(self.ctx)]
        self.assertEqual(len(current), 1)
        self.lifecycle({'schema': 1, 'inventory': current, 'retry': False})
        self.assertFalse(self.service.media_reconcile_needed(self.ctx))
        self.lifecycle({'schema': 1, 'inventory': current, 'retry': True})
        self.assertTrue(self.service.media_reconcile_needed(self.ctx))  # bounded retry kept
        self.lifecycle({'schema': 1, 'inventory': current, 'retry': False})
        self.card(False)
        self.assertTrue(self.service.media_reconcile_needed(self.ctx))  # removal
        self.lifecycle({'schema': 1, 'inventory': [], 'retry': False})
        self.assertFalse(self.service.media_reconcile_needed(self.ctx))
        self.card(True)
        self.assertTrue(self.service.media_reconcile_needed(self.ctx))  # insertion

    def test_dark_cadence_keeps_network_fresh_and_health_deadline_safe(self):
        light = self.root/'sys/class/backlight/y2/brightness'
        power = light.parent/'bl_power'
        self.assertFalse(self.service.display_dark(self.ctx))  # unknown is not dark
        light.parent.mkdir(parents=True)
        light.write_text('7\n')
        power.write_text('0\n')
        self.assertFalse(self.service.display_dark(self.ctx))
        power.write_text('4\n')  # Reborn ScreenSleep: FB_BLANK_POWERDOWN, brightness kept
        self.assertTrue(self.service.display_dark(self.ctx))
        power.write_text('0\n')
        light.write_text('0\n')
        self.assertTrue(self.service.display_dark(self.ctx))
        reborn = (ROOT.parent/'Y2Reborn/crates/reborn-platform/src/power.rs')
        if reborn.exists():
            self.assertIn('bl_power', reborn.read_text())
        self.assertEqual(self.service.cadence(False, True), (3, 30))
        self.assertEqual(self.service.cadence(True, False), (10, 30))  # pending health keeps 30 s
        self.assertEqual(self.service.cadence(True, True), (10, 300))
        self.assertLess(self.service.DARK_INTERVAL_S + 3, 15)  # Reborn stale limit
        settled = self.service.health_settled
        self.assertFalse(settled(None))
        self.assertFalse(settled({'ok': False, 'output': '{"state":"Idle"}'}))
        self.assertFalse(settled({'ok': True, 'output': '{"state":"PendingHealth"}'}))
        self.assertFalse(settled({'ok': True, 'output': 'garbage'}))
        self.assertTrue(settled({'ok': True, 'output': '{"state":"Idle"}'}))

    def test_service_loop_uses_gated_reconcile_and_cadence(self):
        s = (ROOT/'tools/platform/y2_platform/service.py').read_text()
        loop = s[s.index('def serve('):]
        self.assertIn("if media_reconcile_needed(ctx):\n                ctx.command(['/usr/sbin/y2-platform', 'media', 'reconcile']", loop)
        self.assertIn('next_maintenance = before + maintenance', loop)
        self.assertIn('interval - (time.monotonic() - before)', loop)


JOURNAL_FIXTURE = r'''
#pragma clang diagnostic ignored "-Wunused-variable"
#pragma clang diagnostic ignored "-Wunused-function"
#pragma clang diagnostic ignored "-Wunused-const-variable"
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include "pm-journal-policy.h"
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define __iomem
#define READ_ONCE(x) (x)
#define WRITE_ONCE(x, v) ((x) = (v))
#define wmb() ((void)0)
#define raw_spin_lock_irqsave(l, f) ((void)(f), assert(!*(l)), *(l) = 1)
#define raw_spin_unlock_irqrestore(l, f) (assert(*(l)), *(l) = 0)
#undef static_assert
#define static_assert(x) _Static_assert(x, #x)
#define Y2_PM_PHYS 0x0010dc00
#define NSEC_PER_MSEC 1000000ULL
static unsigned long long clock_ns;
static unsigned long long local_clock(void) { return clock_ns += 1500000ULL; }
static unsigned long long div_u64(unsigned long long a, unsigned long long b) { return a / b; }
static unsigned xchg_u(unsigned *p, unsigned v) { unsigned o = *p; *p = v; return o; }
#define xchg(p, v) xchg_u(p, v)
enum { Y2_PM_NONE, Y2_PM_SUSPEND_REQUEST, Y2_PM_FILESYSTEM_SYNCED, Y2_PM_DEVICES_SUSPENDED,
 Y2_PM_SECONDARIES_OFF, Y2_PM_CIRQ_CLONED, Y2_PM_WAKE_MASK_PROGRAMMED, Y2_PM_RTC_ARMED,
 Y2_PM_PCM_INSTALLED, Y2_PM_CPU_CONTEXT_SAVING, Y2_PM_BEFORE_SPM_ENTRY, Y2_PM_AFTER_SPM_RETURN,
 Y2_PM_CPU_CONTEXT_RESTORED, Y2_PM_CIRQ_REPLAYED, Y2_PM_TIMER_RESTORED, Y2_PM_SECONDARIES_ON,
 Y2_PM_DEVICES_RESUMING, Y2_PM_RADIOS_RESTORING, Y2_PM_REBORN_READY, Y2_PM_COMPLETE,
 Y2_PM_UART_REQUEST, Y2_PM_UART_ACK, Y2_PM_NORMAL_PCM_RESTORED, Y2_PM_ABORTED,
 Y2_PM_HELPER_REQUEST, Y2_PM_TASKS_FROZEN, Y2_PM_PLATFORM_BEGIN, Y2_PM_DPM_PREPARE_BEGIN,
 Y2_PM_DPM_PREPARED, Y2_PM_LATE_SUSPENDED, Y2_PM_NOIRQ_SUSPENDED, Y2_PM_SECONDARIES_DISABLING,
 Y2_PM_SYSCORE_SUSPENDED, Y2_PM_PLATFORM_ENTER, Y2_PM_TEST_RETURN, Y2_PM_SELFTEST_A,
 Y2_PM_SELFTEST_B, Y2_PM_BACKSTOP_STARTED, Y2_PM_EXIT, Y2_PM_DEVICES_RESUMED, Y2_PM_CONSOLE_RESUMED,
 Y2_PM_PLATFORM_ENDED, Y2_PM_TASKS_THAWED, Y2_PM_FILESYSTEMS_THAWED, Y2_PM_POST_SUSPEND_NOTIFIED,
 Y2_PM_CONSOLE_RESTORED, Y2_PM_DORMANT_BEGIN, Y2_PM_DORMANT_CONTEXT, Y2_PM_DORMANT_FINISH, Y2_PM_DORMANT_RETURN, Y2_PM_DORMANT_COMPLETE, Y2_PM_DORMANT_ABORTED, Y2_PM_DORMANT_RESTORE_PCM, Y2_PM_DORMANT_RESTORE_CONTEXT, Y2_PM_DORMANT_RESTORE_CIRQ, Y2_PM_DORMANT_RESTORE_CLOCKS, Y2_PM_STAGE_COUNT };
struct y2_pm_backstop_ops { int (*start)(unsigned); void (*ping)(void); void (*stop)(void); };
struct kobject; struct kobj_attribute;
static unsigned sram[Y2_PM_REGION / 4], drop_offset = ~0U, stuck_offset = ~0U;
static void *journal = sram;
static int journal_lock;
static unsigned journal_record[Y2_PM_WORDS], previous[Y2_PM_WORDS], slot, previous_reset_entry;
static unsigned ring_sequence, previous_scratch[Y2_PM_SELFTEST_WORDS];
static unsigned previous_ring_header[8];
static const struct y2_pm_backstop_ops *backstop_ops;
static unsigned backstop_armed, backstop_seconds;
static bool backstop_running, backstop_paused, backstop_staged;
static int backstop_error;
static unsigned starts, pings, stops, start_fail;
static int start(unsigned s) { if (start_fail) return -EBUSY; starts++; assert(s >= 10 && s <= 30); return 0; }
static void ping(void) { pings++; }
static void stop(void) { stops++; }
static const struct y2_pm_backstop_ops ops = { start, ping, stop };
static void writel(unsigned v, void *p)
{
	unsigned off = (unsigned char *)p - (unsigned char *)sram;
	assert(off < Y2_PM_REGION);
	if (off == drop_offset || off == stuck_offset) return;
	*(unsigned *)p = v;
}
static unsigned readl(const void *p) { return *(const unsigned *)p; }
static int sysfs_emit(char *buf, const char *fmt, ...) { va_list a; va_start(a, fmt); int n = vsnprintf(buf, 4096, fmt, a); va_end(a); return n; }
static void y2_pm_backstop_ping(void);
'''


class RetainedJournal(unittest.TestCase):
    def source(self):
        return (ROOT/'kernel/platform/pm-journal.c').read_text()

    def functions(self, *names):
        s = public(self.source(), 'y2_pm_mark', 'y2_pm_backstop_begin', 'y2_pm_backstop_ping',
                   'y2_pm_backstop_pause', 'y2_pm_backstop_end', 'y2_pm_backstop_register')
        body = s[s.index('static const char *const names[]'):s.index('static const char *stage_name')]
        return body + ''.join(function(s, n) for n in names)

    def test_awake_selftest_proves_write_readback_sequence_slots_stamp_and_ring(self):
        run_c(JOURNAL_FIXTURE + self.functions(
            'stage_name', 'commit', 'ring_reset', 'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark', 'y2_pm_backstop_ping',
            'words_equal', 'scratch_pattern', 'selftest', 'retention_show') + r'''
int main(void){
 unsigned sa, sb, a, b;
 /* An existing valid record: the self-test begins a new cycle after it. */
 y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0); y2_pm_mark(Y2_PM_DEVICES_SUSPENDED, -16);
 assert(journal_record[3] == Y2_PM_DEVICES_SUSPENDED && (int)journal_record[4] == -16);
 unsigned before = journal_record[1];
 sram[Y2_PM_STAMP / 4] = 0x59325253;
 assert(!selftest(sram, &sa, &sb, &a, &b));
 assert(sa == before + 1 && sb == before + 2 && a != b);
 assert(y2_pm_valid(sram + Y2_PM_SLOT(a) / 4) && sram[Y2_PM_SLOT(a) / 4 + 2] == Y2_PM_SELFTEST_A);
 assert(y2_pm_valid(sram + Y2_PM_SLOT(b) / 4) && sram[Y2_PM_SLOT(b) / 4 + 2] == Y2_PM_SELFTEST_B);
 assert(!journal_record[4]);                      /* a fresh record, old error cleared */
 assert(sram[Y2_PM_STAMP / 4] == 0x59325253);     /* stamp restored exactly */
 assert(sram[Y2_PM_RING_HEADER / 4] == Y2_PM_RING_MAGIC && ring_sequence);
 /* The next boot sees SELFTEST_B plus the scratch pattern: retention report. */
 memcpy(previous, sram + Y2_PM_SLOT(b) / 4, sizeof(previous));
 memcpy(previous_scratch, sram + Y2_PM_SELFTEST / 4, sizeof(previous_scratch));
 char text[512]; retention_show(0, 0, text);
 assert(strstr(text, "previous_stage=SELFTEST_B") && strstr(text, "selftest_scratch=retained"));
 previous_scratch[3] ^= 1; retention_show(0, 0, text); assert(strstr(text, "selftest_scratch=absent"));
 memset(previous, 0, sizeof(previous)); retention_show(0, 0, text); assert(strstr(text, "previous_valid=0"));
 /* Failure injection: a write that never lands in SRAM is detected. */
 drop_offset = Y2_PM_SLOT(slot ^ 1) + 4; assert(!strcmp(selftest(sram, &sa, &sb, &a, &b), "stage_a_readback"));
 drop_offset = ~0U; stuck_offset = Y2_PM_STAMP; assert(!strcmp(selftest(sram, &sa, &sb, &a, &b), "reset_stamp"));
 stuck_offset = Y2_PM_SELFTEST + 8; assert(!strcmp(selftest(sram, &sa, &sb, &a, &b), "scratch"));
 stuck_offset = Y2_PM_RING_HEADER; sram[Y2_PM_RING_HEADER / 4] = 0; assert(!strcmp(selftest(sram, &sa, &sb, &a, &b), "ring"));
 stuck_offset = ~0U;
 /* A stale RAM-console magic in slot 0 must never be accepted as retained. */
 assert(y2_pm_valid(sram) || !y2_pm_valid(sram));
}
''')

    def test_stage_progression_helper_record_ring_and_torn_entries(self):
        run_c(JOURNAL_FIXTURE + self.functions(
            'stage_name', 'commit', 'ring_reset', 'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark', 'y2_pm_backstop_ping') + r'''
int main(void){
 /* Helper request starts the cycle; the kernel request continues it. */
 y2_pm_mark(Y2_PM_HELPER_REQUEST, 0); unsigned seq = journal_record[1];
 ring_write("musb-hdrc.0", 2, 0, false);
 y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0);
 assert(journal_record[2] == Y2_PM_SUSPEND_REQUEST && journal_record[1] == seq + 1);
 assert(ring_sequence && sram[y2_pm_ring_offset(ring_sequence) / 4] == ring_sequence); /* ring kept */
 unsigned stages[] = { Y2_PM_FILESYSTEM_SYNCED, Y2_PM_TASKS_FROZEN, Y2_PM_PLATFORM_BEGIN, Y2_PM_DPM_PREPARE_BEGIN,
  Y2_PM_DPM_PREPARED, Y2_PM_DEVICES_SUSPENDED, Y2_PM_LATE_SUSPENDED, Y2_PM_NOIRQ_SUSPENDED,
  Y2_PM_SECONDARIES_DISABLING, Y2_PM_SECONDARIES_OFF, Y2_PM_SYSCORE_SUSPENDED, Y2_PM_PLATFORM_ENTER };
 for (unsigned i = 0; i < ARRAY_SIZE(stages); i++) {
  y2_pm_mark(stages[i], i == 7 ? -5 : 0);
  unsigned *cur = sram + Y2_PM_SLOT(slot) / 4, *old = sram + Y2_PM_SLOT(slot ^ 1) / 4;
  assert(y2_pm_valid(cur) && cur[2] == stages[i] && y2_pm_valid(old) && y2_pm_newer(cur[1], old[1]));
 }
 assert(journal_record[3] == Y2_PM_NOIRQ_SUSPENDED && (int)journal_record[4] == -5); /* first failure kept */
 /* A kernel request without a helper request starts a fresh cycle and ring. */
 y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0);
 assert(!journal_record[4] && !sram[Y2_PM_STAMP / 4]);
 /* Fix03: the fresh ring starts with the timed stage entry itself. */
 unsigned *m = sram + y2_pm_ring_offset(ring_sequence) / 4;
 assert(sram[Y2_PM_RING_HEADER / 4 + 2] == ring_sequence && (m[1] >> 8 & 0xff) == Y2_PM_PHASE_MARK &&
        m[1] >> 16 == Y2_PM_SUSPEND_REQUEST && m[7]);
 /* Device callbacks: enter/leave with result; names keep 16 bytes (Fix03). */
 ring_write("11230000.mmc", 2, 0, false); ring_write("11230000.mmc", 2, -16, true);
 unsigned *e = sram + y2_pm_ring_offset(ring_sequence) / 4;
 assert(e[0] == ring_sequence && (e[1] & 1) && (int)e[2] == -16 && !memcmp(e + 3, "11230000.mmc", 12));
 /* A torn entry (sequence not yet written) never looks complete. */
 unsigned next = y2_pm_ring_next(ring_sequence);
 drop_offset = y2_pm_ring_offset(next); ring_write("mt6582-afe", 4, 0, false); drop_offset = ~0U;
 assert(sram[y2_pm_ring_offset(next) / 4] == 0);
 /* The ring wraps without losing order; sequence never becomes zero. */
 for (unsigned i = 0; i < 3 * Y2_PM_RING_ENTRIES; i++) ring_write("x", 1, 0, i & 1);
 assert(y2_pm_ring_next(~0U) == 1);
 unsigned words[4]; y2_pm_ring_name(words, "abcdefghijklmnopqrstuvwxyz"); assert(!memcmp(words, "abcdefghijklmnop", 16));
}
''')

    def test_backstop_is_one_shot_pings_on_progress_and_pauses_for_spm(self):
        run_c(JOURNAL_FIXTURE + self.functions(
            'stage_name', 'commit', 'ring_reset', 'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark', 'y2_pm_backstop_register',
            'y2_pm_backstop_begin', 'y2_pm_backstop_ping', 'y2_pm_backstop_pause', 'y2_pm_backstop_end') + r'''
int main(void){
 /* No provider: arming cannot start anything and reports the error. */
 backstop_armed = 30; y2_pm_backstop_begin(true); assert(!backstop_running && backstop_error == -ENODEV && !backstop_armed);
 y2_pm_backstop_register(&ops);
 /* Not armed: a normal suspend never touches the watchdog. */
 y2_pm_backstop_begin(false); assert(!starts && !backstop_running);
 /* Armed staged request: started, pinged on each stage, stopped at exit. */
 backstop_armed = 30; y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0); y2_pm_backstop_begin(true);
 assert(starts == 1 && backstop_running && backstop_staged && journal_record[2] == Y2_PM_BACKSTOP_STARTED);
 unsigned p = pings; y2_pm_mark(Y2_PM_DEVICES_SUSPENDED, 0); assert(pings == p + 1);
 y2_pm_backstop_end(); assert(stops == 1 && !backstop_running);
 /* One-shot: the next request is not covered. */
 y2_pm_backstop_begin(true); assert(starts == 1);
 /* Full sleep: paused (stopped) only around SPM, restarted after return. */
 backstop_armed = 20; y2_pm_backstop_begin(false); assert(starts == 2 && !backstop_staged);
 y2_pm_backstop_pause(true); assert(stops == 2 && backstop_paused);
 p = pings; y2_pm_mark(Y2_PM_AFTER_SPM_RETURN, 0); assert(pings == p); /* never pinged while paused */
 y2_pm_backstop_pause(false); assert(starts == 3 && !backstop_paused);
 y2_pm_backstop_end(); assert(stops == 3);
 /* Pause is inert when not running (runtime dormant path). */
 y2_pm_backstop_pause(true); assert(stops == 3);
 /* Provider refusal (userspace owns /dev/watchdog) leaves nothing running. */
 start_fail = 1; backstop_armed = 30; y2_pm_backstop_begin(true); assert(!backstop_running && backstop_error == -EBUSY);
}
''')

    def test_core_breadcrumbs_and_callback_ring_are_wired(self):
        suspend = overlay('kernel/power/suspend.c')
        for mark in ('Y2_PM_TASKS_FROZEN', 'Y2_PM_PLATFORM_BEGIN', 'Y2_PM_LATE_SUSPENDED', 'Y2_PM_NOIRQ_SUSPENDED',
                     'Y2_PM_SECONDARIES_DISABLING', 'Y2_PM_SYSCORE_SUSPENDED', 'Y2_PM_PLATFORM_ENTER',
                     'Y2_PM_TEST_RETURN', 'Y2_PM_EXIT'):
            self.assertIn('y2_pm_mark(' + mark, suspend)
        self.assertIn('y2_pm_backstop_begin(pm_test_level != TEST_NONE);', suspend)
        self.assertLess(suspend.index('y2_pm_mark(Y2_PM_SUSPEND_REQUEST'), suspend.index('y2_pm_backstop_begin(pm_test_level'))
        self.assertLess(suspend.index('y2_pm_mark(Y2_PM_EXIT'), suspend.index('y2_pm_backstop_end();'))
        main = overlay('drivers/base/power/main.c')
        run = function(main, 'dpm_run_callback')
        self.assertLess(run.index('y2_pm_device(dev, y2_pm_callback_phase(info, state), 0, false)'), run.index('error = cb(dev);'))
        self.assertLess(run.index('error = cb(dev);'), run.index('y2_pm_callback_phase(info, state), error, true'))
        self.assertIn('y2_pm_device(dev, Y2_PM_PHASE_PREPARE, error, true);', main)
        self.assertIn('y2_pm_device(dev, Y2_PM_PHASE_COMPLETE, 0, true);', main)
        spm = (ROOT/'kernel/platform/spm.c').read_text()
        finish = function(spm, 'y2_spm_finish')
        self.assertLess(finish.index('y2_pm_backstop_pause(true)'), finish.index('v7_exit_coherency_flush'))
        self.assertIn('ret = cpu_suspend(0, y2_spm_finish);\n\t\ty2_pm_backstop_pause(false);', spm)
        wdt = overlay('drivers/watchdog/mtk_wdt.c')
        self.assertIn('if (watchdog_active(wdd) || seconds < WDT_MIN_TIMEOUT', wdt)
        self.assertIn('y2_pm_backstop_register(&y2_backstop_ops);', wdt)
        # Upstream suspend/resume still act only on a core-owned active watchdog.
        self.assertIn('if (watchdog_active(&mtk_wdt->wdt_dev))\n\t\tmtk_wdt_stop', function(wdt, 'mtk_wdt_suspend'))


class UsbTransportAttribution(unittest.TestCase):
    def test_tolerance_is_bounded_and_write_free_only(self):
        run_c(r'''
#include <assert.h>
#include "../usb/fault.h"
int main(void){
 struct y2_usb_tolerance t = {0};
 /* Transient PMIC/PWRAP observation failures do not end the session. */
 for (int i = 0; i < Y2_USB_MONITOR_TOLERANCE - 1; i++) assert(!y2_usb_monitor_terminal(&t, 0));
 assert(!y2_usb_monitor_terminal(&t, 1) && !t.monitor_failures && t.monitor_transients == 3);
 /* A persistent (one second) failure remains terminal. */
 for (int i = 0; i < Y2_USB_MONITOR_TOLERANCE - 1; i++) assert(!y2_usb_monitor_terminal(&t, 0));
 assert(y2_usb_monitor_terminal(&t, 0));
 /* Reconnect: a refusal after any write is terminal immediately. */
 struct y2_usb_tolerance r = {0};
 assert(y2_usb_reconnect_terminal(&r, -5, 1));
 /* Write-free refusals retry for a bounded budget, success resets it. */
 r = (struct y2_usb_tolerance){0};
 for (int i = 0; i < Y2_USB_RECONNECT_ATTEMPTS - 1; i++) assert(!y2_usb_reconnect_terminal(&r, -19, 0));
 assert(!y2_usb_reconnect_terminal(&r, 0, 1) && !r.reconnect_failures);
 for (int i = 0; i < Y2_USB_RECONNECT_ATTEMPTS - 1; i++) assert(!y2_usb_reconnect_terminal(&r, -19, 0));
 assert(y2_usb_reconnect_terminal(&r, -19, 0));
 assert(Y2_USB_RECONNECT_ATTEMPTS * 250 == 10000 && Y2_USB_MONITOR_TOLERANCE * 250 == 1000);
 for (int i = 0; i < Y2_USB_REASON_COUNT; i++) assert(y2_usb_reason_names[i]);
}
''')

    def test_every_terminal_site_is_attributed_and_snapshot_is_read_only(self):
        s = (ROOT/'kernel/platform/usb.c').read_text()
        self.assertEqual(s.count('y2_usb_fail('), 2)  # definition and the single attributed call
        for reason in ('FIFO_LAYOUT', 'IRQ_OVERFLOW', 'DMA_BUS_ERROR', 'INIT', 'SUPPLY_MONITOR', 'REGISTER',
                       'RECONNECT', 'PREFLIGHT', 'PHY_REGION'):
            self.assertIn('Y2_USB_REASON_' + reason, s)
        capture = function(s, 'y2_usb_fail_at')
        # Only INDEX is written, and it is restored; sampled W1C status is not cleared.
        self.assertEqual(capture.count('writeb('), 2)
        self.assertIn('writeb(f.index,b+MUSB_INDEX);', capture)
        self.assertNotIn('writew(', capture)
        self.assertNotIn('writel(', capture)
        self.assertIn('y2_pm_note(note,rc);', capture)
        irq = function(s, 'y2_musb_interrupt')
        self.assertLess(irq.index('Y2_USB_REASON_DMA_BUS_ERROR'), irq.index('writel(0, musb->mregs + 0xa4);'))
        # The reconnect path marks a write before the first session write.
        reconnect = function(s, 'y2_usb_reconnect')
        self.assertLess(reconnect.index('*wrote=true;'), reconnect.index('y2_session_start('))
        worker = function(s, 'y2_usb_worker')
        self.assertIn('if(!y2_usb_monitor_terminal(&y2_tolerance,0)) goto again;', worker)
        # DMA and PIO fallbacks are unchanged: y2.usb_dma and allocation failure.
        self.assertIn('__setup("y2.usb_dma=", y2_usb_dma_option);', s)
        self.assertIn('Y2USB: DMA allocation failed, retaining PIO', s)


def harness():
    import importlib.util
    spec = importlib.util.spec_from_file_location('qualify_fix02', ROOT/'tools/development/qualify-cpu-fix02.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Fix02Harness(unittest.TestCase):
    def test_charger_presence_is_not_charging(self):
        import sys
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.charger_state import classify
        hold = 'phase=HOLD source=1 source_valid=1 charge_limit_ua=70000\nactive=0 online=1 present=1 fault=0x0 sample_error=0 stop_error=0 paused=0 last_error=0'
        self.assertEqual(classify(hold, True, 'Not charging')['category'], 'charger_hold')
        self.assertFalse(classify(hold, True, 'Not charging')['expect_refusal'])  # the Fix01 physical state
        active = hold.replace('phase=HOLD', 'phase=CONSTANT_CURRENT').replace('active=0', 'active=1')
        self.assertTrue(classify(active, True, 'Charging')['expect_refusal'])
        self.assertTrue(classify(active, True, 'Charging')['phase_consistent'])
        failed = hold.replace('stop_error=0', 'stop_error=-5')
        self.assertEqual(classify(failed, True, 'Not charging')['category'], 'charger_stop_error')
        inhibited = hold.replace('phase=HOLD', 'phase=INHIBITED')
        self.assertEqual(classify(inhibited, True, 'Not charging')['category'], 'usb_present_not_charging')
        self.assertEqual(classify(inhibited.replace('online=1', 'online=0'), False, 'Discharging')['category'], 'no_usb')
        self.assertEqual(classify(None, True, None)['category'], 'unknown')

    def test_plan_is_offline_and_run_requires_independent_observer(self):
        import subprocess
        tool = str(ROOT/'tools/development/qualify-cpu-fix02.py')
        plan = subprocess.run(['python3', tool], capture_output=True, text=True, timeout=30)
        self.assertEqual(plan.returncode, 0)
        self.assertIn('only if all awake checks passed', plan.stdout)
        refused = subprocess.run(['python3', tool, '--run'], capture_output=True, text=True, timeout=30)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn('--wifi-host is required', refused.stderr)
        text = (ROOT/'tools/development/qualify-cpu-fix02.py').read_text()
        self.assertNotIn('flash', text.split('def main')[1].lower())
        self.assertEqual(text.count("'reboot'") + text.count('; reboot'), 1)  # only the opt-in retention proof
        self.assertIn('if ok and self.args.allow_warm_reboot:', text)
        # Suspend is only reachable after every awake check and the journal self-test.
        main = text.split('def main')[1]
        self.assertLess(main.index('if not (all(awake) and selftest):'), main.index("run.suspend('devices'"))

    def test_evaluation_helpers(self):
        h = harness()
        rest = 'gated=1 suspends=9 resumes=8 retained=8 restored=0 last_mismatch=0 error=0'
        before = {'11230000.mmc': 'gated=0 suspends=2 resumes=2 error=0', '11240000.mmc': 'gated=1 suspends=1 resumes=0 error=0'}
        io = {'11230000.mmc': 'gated=0 suspends=9 resumes=9 error=0', '11240000.mmc': rest}
        self.assertTrue(h.mmc_verdict(before, {k: rest for k in before}, io)['pass'])
        self.assertFalse(h.mmc_verdict(before, {k: rest.replace('gated=1', 'gated=0') for k in before}, io)['pass'])
        self.assertFalse(h.mmc_verdict(before, {k: rest for k in before}, {k: 'resumes=10 error=-5' for k in before})['pass'])
        state = 'quiet=1 parked=3 parked_mask=0xe last_reset=screen load_mc=120 high_freq_permille=40'
        self.assertTrue(h.coordinator_verdict(state, {'slow_entries': '12'}, '0')['pass'])
        self.assertFalse(h.coordinator_verdict(state, {'slow_entries': '0'}, '0')['pass'])
        self.assertFalse(h.coordinator_verdict(state.replace('0xe', '0x8'), {'slow_entries': '5'}, '0-2')['pass'])
        attr = h.background_attribution({'1': {'name': 'init', 'ticks': 5}},
                                        {'1': {'name': 'init', 'ticks': 7}, '9': {'name': 'python3', 'ticks': 40}}, 20)
        self.assertEqual(attr['top'][0]['name'], 'python3')
        self.assertEqual(attr['processes_started'], 1)
        ring = ('ring=valid cycle=4 last=9 backstop_s=30\n1 phase=2 enter result=0 device=11230000.mmc\n'
                '2 phase=2 leave result=0 device=11230000.mmc\n3 phase=2 enter result=0 device=musb-hdrc.0.auto\n'
                '4 phase=1 leave result=-16 device=mt6323-charger\n')
        self.assertEqual(h.open_callback(ring), {'phase': 2, 'device': 'musb-hdrc.0.auto', 'sequence': 3})
        self.assertIsNone(h.open_callback(ring.replace('3 phase=2 enter', '3 phase=2 leave')))
        self.assertEqual(h.refused_prepare(ring), ['mt6323-charger'])
        good = h.receipt_verdict('0 a a 0 0', 'a')
        self.assertTrue(good['same_boot'] and good['taint_unchanged'] and good['rc'] == 0)
        self.assertFalse(h.receipt_verdict('0 a b 0 0', 'a')['same_boot'])
        self.assertFalse(h.receipt_verdict('0 a a 0 512', 'a')['taint_unchanged'])
        lost = h.usb_loss_verdict({'boot': 'a'}, {'boot': 'a', 'status': {'x': 'stage=9 error=-5 fault_reason=dma_bus_error'}})
        self.assertEqual(lost['classification'], 'usb_transport_dma_bus_error')
        self.assertEqual(h.usb_loss_verdict({'boot': 'a'}, None)['classification'], 'device_unreachable')
        self.assertEqual(h.usb_loss_verdict({'boot': 'a'}, {'boot': 'a', 'status': {}})['classification'], 'usb_transport_unattributed')
        self.assertFalse(h.usb_loss_verdict(None, None)['lost'])
        self.assertEqual(h.irq_count('20: 3 4 0 0 mt6397-rtc\n21: 1 0 mtk-pmic-keys', 'mt6397-rtc'), 7)


class DormantReadiness(unittest.TestCase):
    def test_c3_default_off_and_preflight_is_read_only(self):
        idle = (ROOT/'kernel/platform/idle.c').read_text()
        self.assertIn('CPUIDLE_FLAG_TIMER_STOP | CPUIDLE_FLAG_OFF', idle)  # experimental, default off
        spm = (ROOT/'kernel/platform/spm.c').read_text()
        pre = function(spm, 'dormant_preflight_show')
        for forbidden in ('spm_write', 'writel', 'y2_ccf_boot_vector', 'y2_ccf_deep_idle_begin', 'y2_cirq_begin',
                          'cpu_suspend', 'y2_spm_idle_arm', 'raw_spin_lock'):
            self.assertNotIn(forbidden, pre)
        entry = function(spm.replace('int y2_spm_dormant_idle(', 'static int y2_spm_dormant_idle('), 'y2_spm_dormant_idle')
        # Same prerequisites, same masks, as the real entry path.
        self.assertIn('BIT(0) | BIT(1) |\n\t\t     BIT(4) | BIT(5) | BIT(7)', entry)
        self.assertIn('BIT(0) | BIT(1) | BIT(4) | BIT(5) | BIT(7)', pre)
        self.assertIn('y2_dormant_opp(cpufreq_quick_get(0))', entry)
        self.assertIn('y2_dormant_opp(khz)', pre)
        clocks = (ROOT/'kernel/platform/clocks.c').read_text()
        begin = function(clocks.replace('int y2_ccf_deep_idle_begin(', 'static int y2_ccf_deep_idle_begin('), 'y2_ccf_deep_idle_begin')
        blockers = function(clocks.replace('int y2_ccf_deep_idle_blockers(', 'static int y2_ccf_deep_idle_blockers('), 'y2_ccf_deep_idle_blockers')
        for mask in ('y2_dpidle_peri_blockers', 'y2_uart_sleep_available', '0x0000a080U | BIT(5)'):
            self.assertIn(mask, begin)
            self.assertIn(mask, blockers)
        self.assertIn('0x02fe87fdU | 0x7800U', (ROOT/'kernel/platform/uart-idle-policy.h').read_text())
        self.assertIn('y2_mm_idle_blockers(&deep_disp0_blockers, &deep_disp1_blockers)', begin)
        self.assertIn('y2_mm_idle_blockers(&disp0, &disp1)', pre)
        self.assertIn('deep_disp0_blockers || deep_disp1_blockers', begin)
        self.assertNotIn('writel', blockers)
