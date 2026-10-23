"""POWER-FINAL-07 charge-current table, source policy, thermal ladder and worker."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_charger import FIXTURE, worker_fixture
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'

# Stock Y2 `CS_VTH` (0xc07572f8), 16 words in units of 0.01 mA, index == CHR_CON4
# selector. Read from evidence-private/20260908-ram-map/fm-decompressed.bin and
# equal to the MediaTek BSP CS_VTH_PMIC[] order. Do not edit this to match code.
STOCK_CS_VTH_CENTI_MA = (160000, 150000, 140000, 130000, 120000, 110000, 100000, 90000,
                         80000, 70000, 65000, 55000, 45000, 30000, 20000, 7000)


class ChargeTable(unittest.TestCase):
    def test_single_table_matches_the_stock_array_and_guards_the_pmic(self):
        expected = ','.join(str(v * 10) for v in STOCK_CS_VTH_CENTI_MA)
        run_c(FIXTURE + r'''
static const unsigned stock[16]={%s};
int main(void){
 (void)io;(void)reset;
 for(unsigned s=0;s<16;s++){
  assert(y2_cs_ua(s)==stock[s] && y2_charge_ua(s)==(int)stock[s]);
  if(s)assert(stock[s]<stock[s-1]);
  /* 1200..1600 mA exist in the table but no stock source requests them. */
  assert(y2_cs_selector_permitted(s)==(stock[s]<=1100000));
  assert(y2_cs_selector_permitted(s)==(s>=5));
  /* The PMIC write boundary uses the same predicate for CHR_CON4. */
  assert(y2_pmic_value_allowed(0x008,s)==(s>=5));
  assert(y2_pmic_value_allowed(0x008,0x70|s)==(s>=5));
  assert(y2_cs_selector_exact(stock[s])==(s>=5?(int)s:-1));
 }
 assert(y2_pmic_write_mask(0x008)==0xf);
 assert(y2_cs_selector_exact(0)<0 && y2_cs_selector_exact(650001)<0 && y2_cs_selector_exact(1200000)<0);
 /* floor never rounds a request up and never exceeds the vendor maximum */
 assert(y2_cs_floor_ua(0)==0 && y2_cs_floor_ua(69999)==0 && y2_cs_floor_ua(70000)==70000);
 assert(y2_cs_floor_ua(199999)==70000 && y2_cs_floor_ua(449999)==300000 && y2_cs_floor_ua(450000)==450000);
 assert(y2_cs_floor_ua(649999)==550000 && y2_cs_floor_ua(699999)==650000 && y2_cs_floor_ua(1099999)==1000000);
 assert(y2_cs_floor_ua(1100000)==1100000 && y2_cs_floor_ua(1600000)==1100000 && y2_cs_floor_ua(~0U)==1100000);
 for(unsigned ua=0;ua<=1700000;ua+=1000){
  unsigned f=y2_cs_floor_ua(ua);
  assert(f<=ua && f<=Y2_CHARGE_VENDOR_MAX_UA && (f==0 || y2_cs_selector_exact(f)>=0));
 }
}
''' % expected)

    def test_no_second_selector_table_and_no_raw_current_interface(self):
        for path in sorted(PLATFORM.glob('*')):
            if path.suffix not in ('.c', '.h') or path.name == 'charge-table.h':
                continue
            text = path.read_text()
            self.assertNotIn('1600000', text, path.name)
            self.assertNotRegex(text, r'selector\s*!=\s*15', path.name)
            self.assertNotRegex(text, r'&\s*0xf\)\s*!=\s*15', path.name)
        charger = (PLATFORM / 'mt6323-charger.c').read_text()
        # The policy is the only thing that picks a selector: the sole writable
        # properties are charge behaviour and the owner *ceiling*.
        writeable = re.search(r'y2_bat_writeable.*?\n\}', charger, re.S)[0]
        self.assertIn('CHARGE_BEHAVIOUR', writeable)
        self.assertIn('CONSTANT_CHARGE_CURRENT_MAX', writeable)
        self.assertNotIn('POWER_SUPPLY_PROP_CONSTANT_CHARGE_CURRENT)', writeable.replace('_MAX', 'X'))
        self.assertEqual(len(re.findall(r'DEVICE_ATTR_(?:RW|WO)', charger)), 0)
        # Every selector the driver programs comes from the decision function.
        self.assertEqual(charger.count('y2_charge_decide('), 1)


class SourcePolicy(unittest.TestCase):
    def test_stock_requests_class_ceilings_and_default_result(self):
        run_c(FIXTURE + r'''
struct row{enum y2_source_type type;int alloc;unsigned request,cls,at_default,at_vendor_max;};
static const struct row rows[]={
 /* stock select_charging_curret 0xc03a1d88; the ceiling column is Linux policy */
 {Y2_SOURCE_UNKNOWN,   500000,  70000,  70000,  70000,  70000},
 {Y2_SOURCE_SDP,       500000, 500000, 500000, 450000, 450000},
 {Y2_SOURCE_SDP,       100000, 500000, 500000, 450000, 450000},
 {Y2_SOURCE_SDP,        99999, 500000, 500000, 450000, 450000},
 {Y2_SOURCE_SDP,            0, 500000, 500000, 450000, 450000},
 {Y2_SOURCE_SDP,           -1,  70000, 500000,  70000,  70000},
 {Y2_SOURCE_SDP,      900000, 500000, 500000, 450000, 450000},
 {Y2_SOURCE_CDP,           -1, 650000, 650000, 650000, 650000},
 {Y2_SOURCE_DCP,           -1,1100000,1100000, 650000,1100000},
 {Y2_SOURCE_NONSTANDARD,   -1,1100000, 450000, 450000, 450000},
 {Y2_SOURCE_APPLE,         -1, 800000, 800000, 650000, 800000},
};
int main(void){
 (void)io;(void)reset;
 for(unsigned i=0;i<sizeof rows/sizeof rows[0];i++){
  const struct row *r=&rows[i];struct y2_charge_decision d;
  assert(y2_source_stock_request_ua(r->type,r->alloc)==r->request);
  assert(y2_source_class_ceiling_ua(r->type)==r->cls);
  assert(y2_charge_decide(&d,r->type,r->alloc,4000000,0,Y2_CHARGE_DEFAULT_CEILING_UA)==r->at_default);
  assert(d.stock_request_ua==r->request && d.class_ceiling_ua==r->cls);
  assert(y2_charge_decide(&d,r->type,r->alloc,4000000,0,Y2_CHARGE_VENDOR_MAX_UA)==r->at_vendor_max);
 }
 /* Never exceeds the stock request, the class ceiling, the owner ceiling or 1.1 A. */
 static const unsigned ceilings[]={0,70000,200000,300000,450000,550000,650000,700000,800000,900000,1000000,1100000};
 for(unsigned t=0;t<=Y2_SOURCE_APPLE;t++)
 for(int alloc=-1;alloc<=1000000;alloc+=7919)
 for(int uv=2800000;uv<=4300000;uv+=100000)
 for(unsigned level=0;level<=Y2_THERMAL_LEVELS;level++)
 for(unsigned c=0;c<sizeof ceilings/sizeof ceilings[0];c++){
  struct y2_charge_decision d;
  unsigned ua=y2_charge_decide(&d,t,alloc,uv,level,ceilings[c]);
  assert(ua<=d.stock_request_ua && ua<=d.class_ceiling_ua && ua<=ceilings[c]);
  assert(ua<=d.thermal_cap_ua && ua<=d.deep_cap_ua && ua<=Y2_CHARGE_VENDOR_MAX_UA);
  assert(ua==0 || y2_cs_selector_exact(ua)>=0);
  if(uv<=Y2_PRECHARGE_UV)assert(ua<=Y2_DEEP_DISCHARGE_MAX_UA);
  if(t==Y2_SOURCE_SDP)assert(ua<=(alloc<0 ? 70000U : 450000U));
  if(t==Y2_SOURCE_UNKNOWN)assert(ua<=70000);
  if(t==Y2_SOURCE_NONSTANDARD)assert(ua<=450000);   /* F38 */
  if(level==Y2_THERMAL_LEVELS)assert(ua==0);
 }
 /* A PC port can never be promoted by raising the owner ceiling. */
 for(unsigned c=0;c<sizeof ceilings/sizeof ceilings[0];c++){
  struct y2_charge_decision d;
  assert(y2_charge_decide(&d,Y2_SOURCE_SDP,500000,4000000,0,ceilings[c])<=450000);
 }
}
''')

    def test_stock_source_numbers_are_the_documented_vendor_values(self):
        """The Y2 binary stores charger types 1,3,2,5,4 and requests these mA."""
        src = (PLATFORM / 'source-policy.h').read_text()
        for needle in ('Y2_SOURCE_CDP: return 650000', 'Y2_SOURCE_NONSTANDARD: return 1100000',
                       'Y2_SOURCE_APPLE: return 800000', 'case Y2_SOURCE_DCP:'):
            self.assertIn(needle, src)


class Thermal(unittest.TestCase):
    def test_ladder_has_hysteresis_and_never_flaps(self):
        run_c(FIXTURE + r'''
int main(void){
 (void)io;(void)reset;
 assert(Y2_THERMAL_LEVELS==4);
 assert(y2_thermal_level(0,25000)==0 && y2_thermal_level(0,59999)==0);
 assert(y2_thermal_level(0,60000)==1 && y2_thermal_level(0,70000)==2);
 assert(y2_thermal_level(0,80000)==3 && y2_thermal_level(0,90000)==4 && y2_thermal_level(0,149000)==4);
 assert(y2_thermal_cap_ua(0)==~0U && y2_thermal_cap_ua(1)==450000 && y2_thermal_cap_ua(2)==200000);
 assert(y2_thermal_cap_ua(3)==70000 && y2_thermal_cap_ua(4)==0);
 /* A trip falls only after 5 C of hysteresis. */
 assert(y2_thermal_level(1,59999)==1 && y2_thermal_level(1,55001)==1 && y2_thermal_level(1,55000)==0);
 assert(y2_thermal_level(4,89999)==4 && y2_thermal_level(4,85001)==4 && y2_thermal_level(4,85000)==3);
 /* Sweep up then down: monotone up, monotone down, no oscillation at any boundary. */
 unsigned level=0,changes=0;
 for(int t=20000;t<=100000;t+=250){unsigned n=y2_thermal_level(level,t);assert(n>=level);changes+=n!=level;level=n;}
 assert(level==4 && changes==4);
 for(int t=100000;t>=20000;t-=250){unsigned n=y2_thermal_level(level,t);assert(n<=level);changes+=n!=level;level=n;}
 assert(level==0 && changes==8);
 /* Jitter of +-1 C around every trip never changes the level more than once. */
 for(unsigned i=0;i<Y2_THERMAL_LEVELS;i++){
  level=i;unsigned moves=0;
  for(int k=0;k<200;k++){
   int t=y2_thermal_steps[i].trip_mc+((k&1)?1000:-1000);
   unsigned n=y2_thermal_level(level,t);moves+=n!=level;level=n;
  }
  assert(moves<=1);
 }
}
''')

    def test_sense_current_estimate_is_a_labelled_charger_path_estimate(self):
        run_c(FIXTURE + r'''
int main(void){
 (void)io;(void)reset;
 assert(Y2_ISENSE_MOHM==200);
 /* Observed unit: 98.3 mV (450 mA selector) and 139.6 mV (650 mA), 2 mV offset. */
 int a=y2_isense_current_ua(98300,2000), b=y2_isense_current_ua(139600,2000);
 assert(a>470000 && a<500000 && b>680000 && b<700000);
 assert(y2_isense_current_ua(2000,2000)==0 && y2_isense_current_ua(1000,2000)==0 && y2_isense_current_ua(-5000,0)==0);
}
''')


class Worker(unittest.TestCase):
    PRELUDE = None

    @classmethod
    def fixture(cls):
        s = (PLATFORM / 'mt6323-charger.c').read_text()
        extra = ''.join(function(s, n) for n in ('y2_charger_phase',)).replace(
            'static const char *y2_charger_phase', 'static __attribute__((unused)) const char *y2_charger_phase')
        return worker_fixture().replace('static struct device dev;', extra + '\nstatic struct device dev;', 1)

    def test_dedicated_sources_follow_the_stock_policy_up_to_the_owner_ceiling(self):
        run_c(self.fixture() + r'''
static unsigned selector_now(void){return regs[8/2]&15;}
int main(void){
 struct y2_charger c;
 struct {int kind;enum y2_source_type type;unsigned ceiling,expect;unsigned sel;} rows[]={
  {0,Y2_SOURCE_SDP,       650000,  450000,12}, {0,Y2_SOURCE_SDP,     1100000,  450000,12},
  {2,Y2_SOURCE_CDP,       650000,  650000,10}, {2,Y2_SOURCE_CDP,     1100000,  650000,10},
  {4,Y2_SOURCE_DCP,       650000,  650000,10}, {4,Y2_SOURCE_DCP,     1100000, 1100000, 5},
  {4,Y2_SOURCE_DCP,       900000,  900000, 7}, {4,Y2_SOURCE_DCP,      700000,  700000, 9},
  {4,Y2_SOURCE_DCP,        70000,   70000,15},
  {3,Y2_SOURCE_NONSTANDARD,1100000,450000,12}, {3,Y2_SOURCE_NONSTANDARD,650000,450000,12},
  {5,Y2_SOURCE_APPLE,     650000,  650000,10}, {5,Y2_SOURCE_APPLE,   1100000,  800000, 8},
 };
 for(unsigned i=0;i<sizeof rows/sizeof rows[0];i++){
  reset();charger(&c);c.charge_ceiling_ua=rows[i].ceiling;
  source_kind=rows[i].kind;supply.budget=rows[i].type==Y2_SOURCE_SDP?500000:-1;
  y2_charger_run(&c);
  assert(c.source==rows[i].type && c.active && !c.fault);
  assert(c.charge_ua==rows[i].expect && c.selector==rows[i].sel && selector_now()==rows[i].sel);
  assert(c.decision.stock_request_ua>=c.charge_ua && c.decision.result_ua==c.charge_ua);
 }
}
''')

    def test_source_switches_reclassify_without_reboot_and_never_keep_a_stale_current(self):
        run_c(self.fixture() + r'''
static unsigned selector_now(void){return regs[8/2]&15;}
static void plug(struct y2_charger *c,int kind,int budget){
 source_kind=kind;supply.budget=budget;regs[0]|=0x20;clock_ms+=1000;y2_charger_run(c);
}
static void unplug(struct y2_charger *c){
 regs[0]&=~0x20;clock_ms+=1000;y2_charger_run(c);
 assert(!c->active && !(regs[0]&0x18) && !c->source_valid && c->source==Y2_SOURCE_UNKNOWN);
 assert(c->status==POWER_SUPPLY_STATUS_DISCHARGING);
}
int main(void){
 struct y2_charger c;reset();charger(&c);c.charge_ceiling_ua=1100000;
 /* PC -> disconnected -> wall -> disconnected -> PC -> CDP -> Apple -> nonstandard */
 plug(&c,0,500000);assert(c.source==Y2_SOURCE_SDP && c.charge_ua==450000);
 unplug(&c);
 plug(&c,4,-1);assert(c.source==Y2_SOURCE_DCP && c.charge_ua==1100000 && selector_now()==5);
 unplug(&c);
 plug(&c,0,500000);assert(c.source==Y2_SOURCE_SDP && c.charge_ua==450000 && selector_now()==12);
 unplug(&c);
 plug(&c,2,-1);assert(c.source==Y2_SOURCE_CDP && c.charge_ua==650000);
 unplug(&c);
 plug(&c,5,-1);assert(c.source==Y2_SOURCE_APPLE && c.charge_ua==800000);
 unplug(&c);
 plug(&c,3,-1);assert(c.source==Y2_SOURCE_NONSTANDARD && c.charge_ua==450000);
 unplug(&c);
 /* Wall charger pulled and a PC port plugged between two polls: the interrupt
  * only sets source_changed; the next poll must stop, reclassify and re-program
  * the PC allowance instead of continuing at 1100 mA. */
 plug(&c,4,-1);assert(c.charge_ua==1100000 && c.active);
 source_kind=0;supply.budget=500000;c.source_changed=true;
 clock_ms+=1000;y2_charger_run(&c);
 assert(c.source==Y2_SOURCE_SDP && c.active && c.charge_ua==450000 && selector_now()==12 && !c.fault);
 /* Defect C: a suspended/reset host (allowance 0) keeps the stock 450 mA. */
 supply.budget=0;clock_ms+=1000;y2_charger_run(&c);assert(c.active && c.charge_ua==450000 && !c.fault);
 supply.budget=500000;clock_ms+=1000;y2_charger_run(&c);assert(c.active && c.charge_ua==450000);
 /* A classification failure never leaves a previous wall-charger current programmed. */
 unplug(&c);plug(&c,4,-1);assert(c.charge_ua==1100000);
 phy_error=-EIO;c.source_changed=true;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && !(regs[0]&0x18) && c.fault==Y2_FAULT_SOURCE);
}
''')

    def test_thermal_derating_pause_resume_and_the_latched_emergency_fault(self):
        run_c(self.fixture() + r'''
static unsigned selector_now(void){return regs[8/2]&15;}
static void step(struct y2_charger *c,int die_mc){die.raw=die_mc;clock_ms+=1000;y2_charger_run(c);}
int main(void){
 struct y2_charger c;reset();charger(&c);c.charge_ceiling_ua=1100000;source_kind=4;supply.budget=-1;
 die.raw=30000;y2_charger_run(&c);
 assert(c.active && c.charge_ua==1100000 && c.thermal_level==0);
 step(&c,59000);assert(c.charge_ua==1100000 && starts==1);
 step(&c,61000);assert(c.active && c.charge_ua==450000 && selector_now()==12 && c.thermal_level==1 && !c.fault);
 step(&c,72000);assert(c.active && c.charge_ua==200000 && selector_now()==14 && c.thermal_level==2);
 step(&c,83000);assert(c.active && c.charge_ua==70000 && selector_now()==15 && c.thermal_level==3);
 /* Reaching the last trip pauses charging: no fault, engines off, watchdog serviced off. */
 step(&c,91000);
 assert(!c.active && !(regs[0]&0x18) && !c.fault && c.status==POWER_SUPPLY_STATUS_NOT_CHARGING);
 assert(c.thermal_level==Y2_THERMAL_LEVELS && !strcmp(y2_charger_phase(&c),"THERMAL_HOLD"));
 /* Hysteresis: still held at 86 C, resumes at 84 C at the next step down. */
 step(&c,86000);assert(!c.active && c.thermal_level==4);
 step(&c,84000);assert(c.active && c.charge_ua==70000 && c.thermal_level==3 && !c.fault);
 step(&c,76000);assert(c.charge_ua==70000 && c.thermal_level==3);   /* 80 C trip - 5 C = 75 C */
 step(&c,74000);assert(c.charge_ua==200000 && c.thermal_level==2);
 step(&c,66000);assert(c.charge_ua==200000 && c.thermal_level==2);   /* 70 C trip - 5 C = 65 C */
 step(&c,64000);assert(c.charge_ua==450000 && c.thermal_level==1);
 step(&c,56000);assert(c.charge_ua==450000 && c.thermal_level==1);   /* 60 C trip - 5 C = 55 C */
 step(&c,54000);assert(c.charge_ua==1100000 && selector_now()==5 && c.thermal_level==0 && !c.fault);
 /* The emergency PMIC-die fault is unchanged: latched, engines off. */
 step(&c,150000);assert(!c.active && (c.fault&Y2_FAULT_THERMAL) && !(regs[0]&0x18));
 step(&c,30000);assert(!c.active && (c.fault&Y2_FAULT_THERMAL));
}
''')

    def test_high_current_keeps_first_sample_voltage_termination_and_recharge(self):
        run_c(self.fixture() + r'''
static unsigned selector_now(void){return regs[8/2]&15;}
int main(void){
 struct y2_charger c;reset();charger(&c);c.charge_ceiling_ua=1100000;source_kind=4;supply.budget=-1;
 bat.raw=17000;die.raw=30000;y2_charger_run(&c);
 assert(c.active && c.charge_ua==1100000 && selector_now()==5);
 /* The 4.175 V crossing stops on the first sample at 1.1 A exactly as at 450 mA. */
 bat.raw=19001;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && c.cycle.hold && !c.fault && !(regs[0]&0x18));
 bat.raw=18850;
 for(unsigned n=0;n<60;n++){clock_ms+=1000;y2_charger_run(&c);}
 assert(c.cycle.full && c.status==POWER_SUPPLY_STATUS_FULL && !c.active);
 bat.raw=18000; /* 3.955 V: below the 4.00 V maintenance recharge level */
 for(unsigned n=0;n<61;n++){clock_ms+=1000;y2_charger_run(&c);}
 assert(c.active && !c.cycle.full && c.charge_ua==1100000 && selector_now()==5);
 /* The independent 4.2 V guard is not raised for the higher current. */
 bat.raw=19115;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active && (c.fault&Y2_FAULT_VOLTAGE));
 /* Deep discharge keeps 450 mA even on a wall charger with the ceiling at 1.1 A. */
 reset();charger(&c);c.charge_ceiling_ua=1100000;source_kind=4;bat.raw=14000;
 y2_charger_run(&c);assert(c.active && c.charge_ua==450000);
 bat.raw=17000;clock_ms+=1000;y2_charger_run(&c);assert(c.active && c.charge_ua==1100000);
}
''')

    def test_charger_path_current_estimate_is_zero_when_off_and_tracks_the_drop(self):
        run_c(self.fixture() + r'''
int main(void){
 struct y2_charger c;reset();charger(&c);source_kind=0;supply.budget=500000;
 bat.raw=17000;
 /* ISENSE sits about 2 mV above BATSNS with both engines off. */
 isense.raw=17000+9;die.raw=30000;y2_charger_run(&c);
 assert(c.active && c.sense_offset_valid && c.sense_offset_uv>1500 && c.sense_offset_uv<2500);
 int offset=c.sense_offset_uv;
 isense.raw=17000+9+447; /* about +98 mV while the 450 mA selector regulates */
 clock_ms+=1000;y2_charger_run(&c);
 assert(c.active && c.sense_offset_uv==offset); /* offset is frozen while charging */
 int est=y2_isense_current_ua(c.sense_drop_uv,c.sense_offset_uv);
 assert(c.sense_drop_uv>99000 && c.sense_drop_uv<101000 && est>440000 && est<520000);
 /* The estimate is a separate quantity from the programmed selector. */
 assert(c.charge_ua==450000 && est!=(int)c.charge_ua);
 /* An unplugged reading must never become the next session's offset. */
 regs[0]&=~0x20;isense.raw=17000+120;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.online && !c.sense_offset_valid);
 regs[0]|=0x20;isense.raw=17000+9;clock_ms+=1000;y2_charger_run(&c);
 assert(c.sense_offset_valid && c.sense_offset_uv>1500 && c.sense_offset_uv<2500);
 /* With the engines off the same drop is the offset again, so the estimate is 0. */
 c.behaviour=1;clock_ms+=1000;y2_charger_run(&c);
 assert(!c.active);
 isense.raw=17000+9;clock_ms+=1000;y2_charger_run(&c);
 assert(y2_isense_current_ua(c.sense_drop_uv,c.sense_offset_uv)==0);
}
''')


if __name__ == '__main__':
    unittest.main()
