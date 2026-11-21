"""Dark playback and the core-parking coordinator. Synthetic evidence, never a device receipt."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_cpu_fix02 import COORDINATOR
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
PLATFORM = ROOT / 'kernel/platform'


class PlaybackParking(unittest.TestCase):
    def test_dark_playback_parks_with_the_measured_load_and_frequency_mix(self):
        """9.3.35 on the device: ~0.24 cores busy, schedutil above 598 MHz about 58% of the time."""
        run_c(COORDINATOR + r'''
/* 6% of four cores (240 milli-cores), a raise above 598 MHz in 58% of the samples. */
static void music(unsigned seconds)
{
	for (unsigned i = 0; i < seconds * 4; i++) step(60, i % 100 < 58 ? 1040000 : 598000);
}
int main(void){
 y2_idle_init(&st, 0);
 /* Without the playback class the high-frequency proxy keeps all four cores (the old behaviour). */
 lease = Y2_LEASE_NONE; music(300);
 assert(!nparks && online == 4);
 /* With ordinary playback live, the same load parks one core per step, highest first. */
 y2_idle_init(&st, t); lease = Y2_LEASE_PLAYBACK;
 music(300);
 assert(nparks == 3 && online == 1 && parks[0] == 3 && parks[1] == 2 && parks[2] == 1);
 /* The sample after parking is still the same music on one core: it stays parked. */
 music(120);
 assert(online == 1 && !restores);
}
''')

    def test_playback_still_obeys_the_load_and_saturation_guards(self):
        run_c(COORDINATOR + r'''
int main(void){
 y2_idle_init(&st, 0);
 lease = Y2_LEASE_PLAYBACK;
 /* 15% of four cores (600 milli-cores, e.g. a heavy stream): above the 400 limit, never parks. */
 for (unsigned i = 0; i < 4 * 600; i++) step(150, 1040000);
 assert(!nparks && online == 4);
 /* A light load parks; then one second of saturation restores, as with any other parked state. */
 y2_idle_init(&st, t);
 for (unsigned i = 0; i < 4 * 300; i++) step(50, 598000);
 assert(online == 1);
 enum y2_idle_action a = Y2_IDLE_WAIT;
 for (unsigned i = 0; i < 4 && a != Y2_IDLE_RESTORE; i++) a = step(1000, 1300000);
 assert(a == Y2_IDLE_RESTORE && online == 4 && pressure_restores_seen == 1);
 /* The restore doubles the hold, so parking does not come straight back. */
 unsigned before = nparks;
 for (unsigned i = 0; i < 4 * 100; i++) step(50, 598000);
 assert(nparks == before && online == 4);
}
''')

    def test_a_hard_lease_still_blocks_and_undoes_parking_even_during_playback(self):
        run_c(COORDINATOR + r'''
int main(void){
 y2_idle_init(&st, 0);
 lease = Y2_LEASE_PLAYBACK;
 for (unsigned i = 0; i < 4 * 300; i++) step(50, 598000);
 assert(online == 1);
 /* A scan, a transfer, input-driven Interactive or heavy playback is a hard lease. */
 lease = Y2_LEASE_HARD;
 assert(step(50, 598000) == Y2_IDLE_RESTORE && online == 4);
 unsigned before = nparks;
 for (unsigned i = 0; i < 4 * 600; i++) step(50, 598000);
 assert(nparks == before);
 /* The screen lit also ends it, playback or not. */
 lease = Y2_LEASE_PLAYBACK; dark = 1; y2_idle_init(&st, t);
 for (unsigned i = 0; i < 4 * 300; i++) step(50, 598000);
 assert(online == 1);
 dark = 0; assert(step(50, 598000) == Y2_IDLE_RESTORE && online == 4);
}
''')

    def test_wiring_the_coordinator_and_the_dormant_gate(self):
        idle = (PLATFORM / 'system-idle.c').read_text()
        workload = (PLATFORM / 'workload.c').read_text()
        spm = (PLATFORM / 'spm.c').read_text()
        sample = function(idle, 'sample_work_fn')
        # The sampler classifies leases; the old any-lease question is gone.
        self.assertIn('y2_workload_parking_blocked() ? Y2_LEASE_HARD', sample)
        self.assertIn('y2_workload_playing() ? Y2_LEASE_PLAYBACK', sample)
        self.assertNotIn('y2_workload_active', idle + workload)
        # Clock reclaim stays limited to the no-lease idle (nothing new runs during playback).
        self.assertRegex(sample, r'in\.lease == Y2_LEASE_NONE && in\.online == 1')
        # DORMANT is not changed: ordinary playback still counts as an idle blocker.
        self.assertIn('y2_workload_idle_blocker(hint, strcmp(name, "Idle") != 0)', function(workload, 'y2_workload_write'))
        self.assertIn('y2_workload_idle_blocked()', spm)
        # The run-time switch exists and defaults to on.
        self.assertRegex(workload, r'static bool soft_playback = true;\s*module_param\(soft_playback, bool, 0644\);')
        # A renewal skips the restore only for the soft class while dark.
        write = function(workload, 'y2_workload_write')
        self.assertIn('y2_playback_is_soft(name) && y2_backlight_dark()', write)
        self.assertLess(write.index('renewal'), write.index('y2_system_idle_restore()'))
        self.assertTrue(re.search(r'y2_workload_live\(false\)', workload) and re.search(r'y2_workload_live\(true\)', workload))


if __name__ == '__main__':
    unittest.main()
