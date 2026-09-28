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
