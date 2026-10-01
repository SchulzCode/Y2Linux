"""CPU Final Fix03 regressions. Synthetic evidence, never a device receipt."""
from pathlib import Path
import importlib.util
import json
import sys
import tempfile
import unittest
from test_audio import function
from test_power import run_c
from test_cpu_fix02 import JOURNAL_FIXTURE, overlay, public
ROOT = Path(__file__).resolve().parents[1]


class BusDcmBaseline(unittest.TestCase):
    """Fix02: SLIDLE refused every attempt because TOPCKGEN+4 read 0x0."""
    FIXTURE = r'''
#pragma clang diagnostic ignored "-Wunused-function"
#pragma clang diagnostic ignored "-Wunused-variable"
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#include "idle-clock-policy.h"
#define __iomem
static unsigned top[64],peri[64],infra[64],online=1,cpu,entries,slow_blockers,slow_restore_failures,slow_blockers_single,slow_blocker_bits[24];
static unsigned deep_peri_blockers,deep_infra_blockers;
static unsigned long slow_reject_topology,slow_reject_lock,slow_reject_bus,slow_reject_clock;
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define BIT(n) (1U<<(n))
static void *y2_clock_bases[]={top,peri,infra};static int y2_clk_lock;
static unsigned num_online_cpus(void){return online;}
static unsigned smp_processor_id(void){return cpu;}
static int spin_trylock(int *p){if(*p)return 0;*p=1;return 1;}
static void spin_unlock(int *p){assert(*p);*p=0;}
static unsigned readl(void *p){return *(unsigned *)p;}
static void writel(unsigned v,void *p){*(unsigned *)p=v;}
#define dsb(x) do{}while(0)
static void cpu_do_idle(void){assert(y2_clk_lock && top[1]==0x8f);entries++;}
static int y2_ccf_deep_idle_end(void);
'''

    def source(self):
        clocks = (ROOT/'kernel/platform/clocks.c').read_text()
        for name in ('y2_ccf_slow_idle', 'y2_ccf_deep_idle_begin', 'y2_ccf_deep_idle_end'):
            clocks = clocks.replace('int ' + name + '(', 'static int ' + name + '(')
        return clocks

    def test_policy_is_the_stock_reset_default_and_post_disable_value(self):
        run_c(r'''
#include <assert.h>
#include "idle-clock-policy.h"
int main(void){
 assert(y2_bus_dcm_baseline(0x00));   /* reset default, board readback */
 assert(y2_bus_dcm_baseline(0x0f));   /* after stock bus_dcm_disable */
 assert(!y2_bus_dcm_baseline(0x8f));  /* DCM already enabled: another owner */
 assert(!y2_bus_dcm_baseline(0x80) && !y2_bus_dcm_baseline(0x07) && !y2_bus_dcm_baseline(0x1234));
 assert(Y2_BUS_DCM_IDLE == 0x8f);
}
''')

    def test_slow_idle_enters_from_the_board_value_and_restores_it_exactly(self):
        s = self.source()
        run_c(self.FIXTURE + 'static unsigned idle_bus, idle_audio;\n' + function(s, 'y2_ccf_slow_idle') + r'''
int main(void){
 peri[0x18/4]=~0U;
 top[1]=0x00;assert(!y2_ccf_slow_idle() && entries==1 && top[1]==0x00 && !slow_reject_bus);
 top[1]=0x0f;assert(!y2_ccf_slow_idle() && entries==2 && top[1]==0x0f);
 top[1]=0x8f;assert(y2_ccf_slow_idle()==-EBUSY && top[1]==0x8f && slow_reject_bus==1 && entries==2);
 top[1]=0x1234;assert(y2_ccf_slow_idle()==-EBUSY && top[1]==0x1234 && slow_reject_bus==2);
 /* Clock blockers are still checked first and still refuse. */
 top[1]=0;peri[0x18/4]=~(1U<<20);assert(y2_ccf_slow_idle()==-EBUSY && slow_reject_clock==1 && entries==2);
 assert(!y2_clk_lock);
}
''')

    def test_deep_idle_uses_the_same_baseline_and_restores_exactly(self):
        s = self.source()
        run_c(self.FIXTURE + 'static unsigned idle_bus, idle_audio;\n' + function(s, 'y2_ccf_deep_idle_end') +
              function(s, 'y2_ccf_deep_idle_begin') + r'''
int main(void){
 peri[0x18/4]=~0U;infra[0x40/4]=~0U;top[0x70/4]=0x07123456;
 top[1]=0x00;assert(!y2_ccf_deep_idle_begin() && top[1]==0x8f && top[0x70/4]==0x00123456 && y2_clk_lock);
 assert(!y2_ccf_deep_idle_end() && top[1]==0x00 && top[0x70/4]==0x07123456 && !y2_clk_lock);
 top[1]=0x0f;assert(!y2_ccf_deep_idle_begin());assert(!y2_ccf_deep_idle_end() && top[1]==0x0f);
 top[1]=0x8f;assert(y2_ccf_deep_idle_begin()==-EBUSY && top[1]==0x8f && !y2_clk_lock);
}
''')

    def test_preflight_reports_bus_with_the_same_predicate(self):
        spm = (ROOT/'kernel/platform/spm.c').read_text()
        pre = function(spm, 'dormant_preflight_show')
        self.assertIn('!y2_bus_dcm_baseline(bus) ? "bus," : ""', pre)
        self.assertNotIn('0x0f', pre)
        clocks = (ROOT/'kernel/platform/clocks.c').read_text()
        self.assertNotIn('!= 0x0f', clocks)
        self.assertEqual(clocks.count('y2_bus_dcm_baseline('), 2)


USB_GUARD = r'''
#include <assert.h>
#include "../usb/fault.h"
'''


class UsbStormGuard(unittest.TestCase):
    def test_fix02_loaded_upload_is_not_a_storm(self):
        # Per 512-byte packet: RX endpoint interrupt -> DMA program (progress),
        # then the DMA completion interrupt. Fix02 tripped at 513 of these.
        run_c(USB_GUARD + r'''
int main(void){
 struct y2_usb_irq_guard g={0};unsigned progress=0,packet;
 for(packet=0;packet<1000;packet++){   /* 2000 interrupts in one 10-ms jiffy */
  assert(!y2_usb_irq_storm(&g,7,progress)); progress++;
  assert(!y2_usb_irq_storm(&g,7,progress));
 }
 assert(g.max_jiffy==2000 && g.max_burst<=2);
 /* The Fix02 counts: 1155 interrupts for 611 DMA programs, even all at once. */
 struct y2_usb_irq_guard h={0};unsigned i,p=0;
 for(i=0;i<1155;i++){ if(i%2==0 && p<611) p++; assert(!y2_usb_irq_storm(&h,9,p)); }
}
''')

    def test_stuck_source_without_progress_is_terminal(self):
        run_c(USB_GUARD + r'''
int main(void){
 struct y2_usb_irq_guard g={0};unsigned i;
 /* Some real progress, then an RX/DMA status that keeps re-asserting. */
 for(i=0;i<100;i++)assert(!y2_usb_irq_storm(&g,1,i));
 for(i=0;i<Y2_USB_IRQ_BURST_LIMIT;i++)assert(!y2_usb_irq_storm(&g,1,100));
 assert(y2_usb_irq_storm(&g,1,100));   /* the 513th interrupt without progress */
 assert(g.max_burst==Y2_USB_IRQ_BURST_LIMIT+1);
 /* A new jiffy resets the no-progress count; the source still never recovers. */
 struct y2_usb_irq_guard k={0};
 for(i=0;i<400;i++)assert(!y2_usb_irq_storm(&k,1,5));
 for(i=0;i<Y2_USB_IRQ_BURST_LIMIT;i++)assert(!y2_usb_irq_storm(&k,2,5));
 assert(y2_usb_irq_storm(&k,2,5));
}
''')

    def test_hard_ceiling_bounds_any_rate_even_with_progress(self):
        run_c(USB_GUARD + r'''
int main(void){
 struct y2_usb_irq_guard g={0};unsigned i;
 for(i=1;i<=Y2_USB_IRQ_JIFFY_LIMIT;i++)assert(!y2_usb_irq_storm(&g,3,i));
 assert(y2_usb_irq_storm(&g,3,i));
 /* Above the HS bulk maximum: 13 packets/125 us, two interrupts each. */
 assert(Y2_USB_IRQ_JIFFY_LIMIT > 13*8*1000/100*2);
}
''')

    def test_driver_counts_programming_as_progress_and_stays_terminal(self):
        usb = (ROOT/'kernel/platform/usb.c').read_text()
        program = function(usb, 'y2_dma_program')
        self.assertLess(program.index('if (!ret) {'), program.index('WRITE_ONCE(y2_usb_progress, y2_usb_progress + 1);'))
        isr = function(usb, 'y2_musb_interrupt')
        self.assertIn('y2_usb_irq_storm(&y2_irq_guard,jiffies,READ_ONCE(y2_usb_progress))', isr)
        self.assertLess(isr.index('y2_usb_irq_storm('), isr.index('Y2_USB_REASON_IRQ_OVERFLOW'))
        self.assertIn('disable_irq_nosync(irq);', isr)  # still terminal
        self.assertNotIn('y2_irq_burst', usb)
        self.assertIn('irq_max_jiffy=%u irq_jiffy_limit=%u irq_progress=%u', usb)


IDLE = r'''
#include <assert.h>
#include "system-idle-policy.h"
'''


class CoordinatorWake(unittest.TestCase):
    def test_wake_burst_does_not_escalate_the_hold(self):
        run_c(IDLE + r'''
int main(void){
 struct y2_idle_state s;y2_idle_init(&s,0);
 s.parked=3;s.parked_since_ms=100000;
 /* Render/start-up burst on the single CPU, then the display restore 0.6 s later. */
 y2_idle_restored(&s,110000,1);assert(s.hold_ms==120000 && s.pressure_restores==1);
 y2_idle_wake(&s,110630);
 assert(s.hold_ms==60000 && s.pressure_restores==0 && s.wake_reclassified==1);
 assert(s.hold_until_ms==110630+60000);
 /* A later wake never undoes it twice. */
 y2_idle_wake(&s,110700);assert(s.hold_ms==60000 && s.wake_reclassified==1);
}
''')

    def test_sustained_pressure_still_escalates(self):
        run_c(IDLE + r'''
int main(void){
 struct y2_idle_state s;y2_idle_init(&s,0);
 s.parked=3;s.parked_since_ms=100000;
 y2_idle_restored(&s,110000,1);
 y2_idle_wake(&s,110000+Y2_SYSTEM_WAKE_GRACE_MS+1); /* wake well after: unrelated */
 assert(s.hold_ms==120000 && s.pressure_restores==1 && !s.wake_reclassified);
 s.parked=3;s.parked_since_ms=200000;
 y2_idle_restored(&s,210000,1);assert(s.hold_ms==240000 && s.pressure_restores==2);
 y2_idle_wake(&s,210000+Y2_SYSTEM_WAKE_GRACE_MS);   /* boundary is inclusive */
 assert(s.hold_ms==120000 && s.pressure_restores==1 && s.wake_reclassified==1);
 /* Non-pressure restores never arm a reclassification. */
 s.parked=3;s.parked_since_ms=300000;y2_idle_restored(&s,310000,0);y2_idle_wake(&s,310001);
 assert(s.hold_ms==120000 && s.wake_reclassified==1);
}
''')

    def test_display_workload_and_input_restores_are_wakes(self):
        source = (ROOT/'kernel/platform/system-idle.c').read_text()
        restore = function(source.replace('int y2_system_idle_restore(', 'static int y2_system_idle_restore('),
                           'y2_system_idle_restore')
        self.assertLess(restore.index('restore_locked(false);'), restore.index('y2_idle_wake(&state, now_ms());'))
        sample = function(source, 'sample_work_fn')
        self.assertLess(sample.index('if (in.demand)\n\t\ty2_idle_wake(&state, in.now_ms);'),
                        sample.index('y2_idle_decide(&state, &in, &why)'))
        self.assertIn('wake_reclassified=%u', source)
        for caller in ('backlight.c', 'workload.c'):
            self.assertIn('y2_system_idle_restore()', (ROOT/'kernel/platform'/caller).read_text())


class SdInventoryIdentity(unittest.TestCase):
    def setUp(self):
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.common import Context
        from y2_platform import service
        self.service = service
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.root = Path(self.temp.name)

    def card(self, diskseq='17', cid='0353445331364780'):
        disk = self.root/'sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:0001/block/mmcblk1'
        part = disk/'mmcblk1p1'
        part.mkdir(parents=True, exist_ok=True)
        (disk/'device').mkdir(exist_ok=True)
        (disk/'dev').write_text('179:24\n')
        (disk/'diskseq').write_text(diskseq + '\n')
        (disk/'device/cid').write_text(cid + '\n')
        (part/'dev').write_text('179:25\n')
        (part/'partition').write_text('1\n')
        block = self.root/'sys/class/block'
        block.mkdir(parents=True, exist_ok=True)
        for name, target in (('mmcblk1', disk), ('mmcblk1p1', part)):
            link = block/name
            if link.is_symlink():
                link.unlink()
            link.symlink_to(target)

    def test_kernfs_reclaim_is_not_a_media_change(self):
        from y2_platform.media import inventory
        self.card()
        before = [list(v) for v in inventory(self.ctx)]
        self.assertEqual(before, [['mmcblk1', '179:24', '17', '0353445331364780'],
                                  ['mmcblk1p1', '179:25', '17', '0353445331364780']])
        # drop_caches: kernfs recreates the class links (new inodes and ctimes).
        self.card()
        self.assertEqual([list(v) for v in inventory(self.ctx)], before)
        path = self.root/'run/y2/media-lifecycle.json'
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({'schema': 1, 'inventory': before, 'retry': False}))
        self.assertFalse(self.service.media_reconcile_needed(self.ctx))
        # Re-enumeration of the identical card allocates a new diskseq.
        self.card(diskseq='18')
        self.assertTrue(self.service.media_reconcile_needed(self.ctx))

    def test_source_no_longer_uses_inode_identity(self):
        media = (ROOT/'tools/platform/y2_platform/media.py').read_text()
        self.assertNotIn('st_ino', media)
        self.assertNotIn('st_ctime', media)


DEVICE = r'''
struct device { const char *name; };
static const char *dev_name(const struct device *d) { return d->name; }
static int sysfs_emit_at(char *buf, int at, const char *fmt, ...) { va_list a; va_start(a, fmt); int n = vsnprintf(buf + at, 8192 - at, fmt, a); va_end(a); return n; }
'''


class ResumeCompletionDiagnostics(unittest.TestCase):
    def functions(self, *names):
        s = public((ROOT/'kernel/platform/pm-journal.c').read_text(), 'y2_pm_mark', 'y2_pm_backstop_begin',
                   'y2_pm_backstop_ping', 'y2_pm_backstop_pause', 'y2_pm_backstop_end',
                   'y2_pm_backstop_register', 'y2_pm_device', 'y2_pm_reset_status')
        body = s[s.index('static const char *const names[]'):s.index('static const char *stage_name')]
        return body + ''.join(function(s, n) for n in names)

    def test_every_call_between_device_resume_and_exit_is_marked(self):
        suspend = overlay('kernel/power/suspend.c')
        order = ['dpm_resume_end(PMSG_RESUME);', 'y2_pm_mark(Y2_PM_DEVICES_RESUMED, error);',
                 'console_resume_all();', 'y2_pm_mark(Y2_PM_CONSOLE_RESUMED, error);',
                 'platform_resume_end(state);', 'y2_pm_mark(Y2_PM_PLATFORM_ENDED, error);']
        positions = [suspend.index(x) for x in order]
        self.assertEqual(positions, sorted(positions))
        finish = function(suspend, 'suspend_finish')
        order = ['suspend_thaw_processes();', 'y2_pm_mark(Y2_PM_TASKS_THAWED, 0);', 'filesystems_thaw();',
                 'y2_pm_mark(Y2_PM_FILESYSTEMS_THAWED, 0);', 'pm_notifier_call_chain(PM_POST_SUSPEND);',
                 'y2_pm_mark(Y2_PM_POST_SUSPEND_NOTIFIED, 0);', 'pm_restore_console();',
                 'y2_pm_mark(Y2_PM_CONSOLE_RESTORED, 0);']
        positions = [finish.index(x) for x in order]
        self.assertEqual(positions, sorted(positions))
        # Each mark pings the backstop; marks remain the only writer of EXIT.
        mark = function(public((ROOT/'kernel/platform/pm-journal.c').read_text(), 'y2_pm_mark'), 'y2_pm_mark')
        self.assertIn('y2_pm_backstop_ping();', mark)

    def test_stage_entries_are_timed_and_decoded(self):
        run_c(JOURNAL_FIXTURE + DEVICE + self.functions(
            'stage_name', 'commit', 'ring_reset', 'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark',
            'y2_pm_backstop_ping', 'y2_pm_device', 'emit_ring') + r'''
int main(void){
 static char text[8192]; static unsigned entries[Y2_PM_RING_ENTRIES][Y2_PM_RING_WORDS]; unsigned header[8], i, w;
 struct device faux = { "faux" }, mmc = { "a-very-long-platform-name.11230000.mmc" };
 y2_pm_mark(Y2_PM_HELPER_REQUEST, 0); y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0);
 y2_pm_mark(Y2_PM_DEVICES_RESUMING, 0);
 y2_pm_device(&mmc, 7, 0, false); y2_pm_device(&mmc, 7, 0, true);
 y2_pm_device(&faux, 8, 0, false); y2_pm_device(&faux, 8, 0, true);
 y2_pm_mark(Y2_PM_DEVICES_RESUMED, 0); y2_pm_mark(Y2_PM_CONSOLE_RESUMED, 0);
 for (i = 0; i < 4; i++) header[i] = sram[Y2_PM_RING_HEADER / 4 + i];
 for (i = 0; i < Y2_PM_RING_ENTRIES; i++) for (w = 0; w < Y2_PM_RING_WORDS; w++)
  entries[i][w] = sram[(Y2_PM_RING + (i * Y2_PM_RING_WORDS + w) * 4) / 4];
 emit_ring(text, entries, header);
 assert(strstr(text, "ring=valid"));
 assert(strstr(text, "phase=254 mark result=0 ms="));
 assert(strstr(text, "stage=DEVICES_RESUMING\n") && strstr(text, "stage=CONSOLE_RESUMED\n"));
 assert(strstr(text, "phase=8 leave result=0 ms=") && strstr(text, "device=faux\n"));
 assert(strstr(text, "device=ame.11230000.mmc\n")); /* distinguishing 16-byte tail */
 /* Ordered and monotonic: the last line is the last stage reached. */
 char *last = strrchr(text, '\n'); *last = 0; last = strrchr(text, '\n') + 1;
 assert(strstr(last, "stage=CONSOLE_RESUMED"));
 unsigned *a = sram + y2_pm_ring_offset(ring_sequence - 1) / 4, *b = sram + y2_pm_ring_offset(ring_sequence) / 4;
 assert(a[7] && b[7] > a[7]);
 /* Fix02-layout rings are recognized, not misdecoded. */
 header[0] = Y2_PM_RING_MAGIC_FIX02; emit_ring(text, entries, header);
 assert(strstr(text, "ring=fix02_layout") && !strstr(text, "phase="));
}
''')

    def test_device_callbacks_extend_the_backstop(self):
        run_c(JOURNAL_FIXTURE + DEVICE + self.functions(
            'stage_name', 'commit', 'ring_reset', 'ring_ms', 'ring_put', 'ring_write', 'y2_pm_mark',
            'y2_pm_backstop_register', 'y2_pm_backstop_begin', 'y2_pm_backstop_ping',
            'y2_pm_backstop_pause', 'y2_pm_backstop_end', 'y2_pm_device') + r'''
int main(void){
 struct device d = { "musb-hdrc.4.auto" };
 y2_pm_backstop_register(&ops); backstop_armed = 30;
 y2_pm_mark(Y2_PM_SUSPEND_REQUEST, 0); y2_pm_backstop_begin(false);
 unsigned p = pings; y2_pm_device(&d, 7, 0, false); y2_pm_device(&d, 7, 0, true); assert(pings == p + 2);
 y2_pm_backstop_pause(true); p = pings; y2_pm_device(&d, 4, 0, false); assert(pings == p);
 y2_pm_backstop_pause(false); y2_pm_backstop_end();
}
''')

    def test_rgu_status_decode(self):
        run_c(r'''
#include <assert.h>
#include <string.h>
#include "pm-journal-policy.h"
int main(void){
 assert(!strcmp(y2_rgu_cause(0x80000000), "watchdog_timeout"));
 assert(!strcmp(y2_rgu_cause(0x40000000), "software_reset"));
 assert(!strcmp(y2_rgu_cause(0x20000000), "watchdog_irq"));
 assert(!strcmp(y2_rgu_cause(0x00080000), "debug_reset"));
 assert(!strcmp(y2_rgu_cause(0x00000001), "spm_watchdog"));
 assert(!strcmp(y2_rgu_cause(0xc0000000), "watchdog_timeout"));
 assert(!strcmp(y2_rgu_cause(0), "none_reported") && !strcmp(y2_rgu_cause(0x100), "unknown"));
}
''')
        wdt = overlay('drivers/watchdog/mtk_wdt.c')
        probe = function(wdt, 'mtk_wdt_probe')
        self.assertLess(probe.index('y2_pm_reset_status(readl(mtk_wdt->wdt_base + 0x0c));'),
                        probe.index('mtk_wdt_init(&mtk_wdt->wdt_dev);'))
        journal = (ROOT/'kernel/platform/pm-journal.c').read_text()
        self.assertIn('&reset_status_attr.attr', journal)

    def test_boot_journal_records_the_decoded_cause(self):
        sys.path.insert(0, str(ROOT/'tools/platform'))
        from y2_platform.boot import reset_cause, transition
        self.assertEqual(reset_cause('valid=1 raw=0x80000000 cause=watchdog_timeout hw_watchdog=1'),
                         {'reset_cause': 'watchdog_timeout', 'reset_cause_reason': 'rgu_wdt_status_at_probe',
                          'reset_status_raw': '0x80000000'})
        zero = reset_cause('valid=1 raw=0x0 cause=none_reported')
        self.assertIsNone(zero['reset_cause'])
        self.assertEqual(zero['reset_cause_reason'], 'rgu_status_none_reported')
        self.assertEqual(reset_cause(None)['reset_cause_reason'], 'not_observed')
        record = transition(None, 'b1', 'platform_start', {}, 1,
                            reset_cause('valid=1 raw=0x40000000 cause=software_reset'))
        self.assertEqual(record['reset_cause'], 'software_reset')
        self.assertIsNone(transition(None, 'b1', 'platform_start', {}, 1)['reset_cause'])


class DvfsAdmissionLabel(unittest.TestCase):
    def test_changed_constraint_is_reported_as_success(self):
        dvfs = function((ROOT/'kernel/platform/cpu-dvfs.c').read_text(), 'stock_admission_work')
        self.assertIn('if (ret < 0) WRITE_ONCE(qualification_max_khz, 1040000);\n\t\telse ret = 0;', dvfs)
        self.assertLess(dvfs.index('else ret = 0;'), dvfs.index('admission_error = ret;'))


class Harness(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('h3', ROOT/'tools/development/qualify-cpu-fix03.py')
        self.h = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.h)

    def test_parking_and_slidle_are_separate_verdicts(self):
        h = self.h
        state = 'quiet=1 parked=3 parked_mask=0xe last_reset=none load_mc=161 high_freq_permille=214'
        self.assertTrue(h.coordinator_verdict(state, {'slow_entries': '0'}, '0')['pass'])
        self.assertFalse(h.coordinator_verdict(state.replace('0xe', '0xc'), {}, '0-1')['pass'])
        wake = 'hold_ms=60000 pressure_restores=0 wake_reclassified=1 last_reset=burst'
        self.assertTrue(h.wake_hold_verdict(wake)['pass'])
        self.assertFalse(h.wake_hold_verdict('hold_ms=120000 pressure_restores=1')['pass'])
        samples = [{'idle': {'slow_entries': '0'}, 'clk': {'slow_reject_bus': '0', 'slow_reject_clock': '3'}},
                   {'idle': {'slow_entries': '412'}, 'clk': {'slow_reject_bus': '0', 'slow_reject_clock': '9'}}]
        self.assertTrue(h.slidle_verdict(samples)['pass'])
        samples[1]['clk']['slow_reject_bus'] = '20809'
        self.assertFalse(h.slidle_verdict(samples)['pass'])
        self.assertFalse(h.slidle_verdict([])['pass'])

    def test_attribution_percent_is_of_one_core(self):
        before = {'1': {'name': 'reborn', 'ticks': 0}}
        after = {'1': {'name': 'reborn', 'ticks': 819}}  # 8.19 s CPU over 243 s
        row = self.h.background_attribution(before, after, 243)['top'][0]
        self.assertEqual(row['cpu_ms'], 8190)
        self.assertAlmostEqual(row['percent_of_one_core'], 3.37, places=2)

    def test_charger_refusal_uses_the_journal_failed_stage(self):
        journal = 'valid=1 sequence=9 stage=EXIT failed_stage=DPM_PREPARED error=-16'
        ring = 'ring=valid cycle=4 last=40\n40 phase=254 mark result=-16 ms=5 stage=EXIT\n'
        verdict = self.h.charger_refused(journal, ring, '')
        self.assertTrue(verdict['refused'] and verdict['journal_failed_stage'] and not verdict['ring_prepare'])
        self.assertFalse(self.h.charger_refused('failed_stage=NONE error=0', ring, '')['refused'])
        ring = 'ring=valid\n3 phase=1 leave result=-16 ms=4 device=y2-mt6323-charger\n'
        self.assertEqual(self.h.refused_prepare(ring), ['y2-mt6323-charger'])

    def test_resume_boundary_names_the_stalled_call(self):
        ring = ('ring=valid cycle=101 last=9 backstop_s=30\n'
                '1 phase=254 mark result=0 ms=100 stage=DEVICES_RESUMING\n'
                '2 phase=8 enter result=0 ms=900 device=faux\n'
                '3 phase=8 leave result=0 ms=901 device=faux\n'
                '4 phase=254 mark result=0 ms=902 stage=DEVICES_RESUMED\n')
        boundary = self.h.resume_boundary(ring)
        self.assertEqual(boundary['last_stage'], 'DEVICES_RESUMED')
        self.assertEqual(boundary['stalled_call'], 'console_resume_all')
        self.assertIsNone(boundary['open_device_callback'])
        ring += '5 phase=254 mark result=0 ms=903 stage=CONSOLE_RESUMED\n6 phase=254 mark result=0 ms=904 stage=PLATFORM_ENDED\n'
        self.assertEqual(self.h.resume_boundary(ring)['stalled_call'], 'suspend_thaw_processes')
        self.assertEqual(self.h.open_callback('7 phase=7 enter result=0 ms=9 device=musb-hdrc.4.auto\n'),
                         {'phase': 7, 'device': 'musb-hdrc.4.auto', 'sequence': 7})

    def test_launch_steps_never_retry_and_qos_uses_back(self):
        source = (ROOT/'tools/development/qualify-cpu-fix03.py').read_text()
        self.assertIn("key('navigation',158)", source)
        self.assertNotIn("key('navigation',103)", source)
        suspend = source[source.index('    def suspend('):source.index('def irq_count')]
        self.assertIn('self.remote(body, hosts=[self.live_host()])', suspend)
        self.assertIn("capture_output=True,text=True);(directory/'rtc-alarm')", suspend)
        journal = source[source.index('    def journal('):source.index('    # --- suspend')]
        self.assertIn("self.live_host(), 'sync; reboot'", journal)
        self.assertIn("for cycle in range(5):", source)

    def test_plan_is_offline(self):
        import io
        import contextlib
        out = io.StringIO()
        sys_argv = sys.argv
        try:
            sys.argv = ['qualify-cpu-fix03.py']
            with contextlib.redirect_stdout(out):
                self.h.main()
        finally:
            sys.argv = sys_argv
        self.assertIn('five RTC cycles', out.getvalue())


if __name__ == '__main__':
    unittest.main()
