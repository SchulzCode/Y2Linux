"""Protocol/boot-policy tests. No DRM device or running Reborn is touched."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
REBORN = ROOT.parent / 'Y2Reborn'


class Splash(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix='reborn-splash-build-')
        cls.binary = Path(cls.build.name) / 'splash'
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'libdrm'], text=True).split()
        subprocess.run(['cc', '-DRB_SPLASH_TEST', '-Os', '-Wall', '-Wextra', '-Werror',
                        str(ROOT / 'tools/graphics/reborn-splash.c'), *flags, '-o', str(cls.binary)], check=True)
        driver = Path(cls.build.name) / 'client.c'
        header = REBORN / 'crates/reborn-graphics/native/splash-handoff.h'
        driver.write_text('''#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static double now(void) {struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
''' + '#include "' + str(header) + '"\n' + '''
int main(int argc,char **argv) {
 if(argc!=2)return 2;
 int fd=rb_splash_begin(argv[1]);
 if(fd < -1)return 3;
 if(fd == -1){puts("absent");return 0;}
 puts("released");rb_splash_presented(fd);return 0;
}
''')
        cls.client_binary = Path(cls.build.name) / 'client'
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', str(driver), '-o', str(cls.client_binary)], check=True)
        picture = ROOT / 'tests/splash_picture.c'
        cls.picture_binary = Path(cls.build.name) / 'picture'
        subprocess.run(['cc', '-Os', '-Wall', '-Wextra', '-Werror', str(picture), *flags,
                        '-o', str(cls.picture_binary)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reborn-splash-')
        self.root = Path(self.temp.name)
        self.sock = self.root / 'control.sock'
        self.process = subprocess.Popen([str(self.binary), '--test', str(self.root), str(self.sock)])
        self.wait(lambda: self.sock.exists())

    def tearDown(self):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=3)
        self.temp.cleanup()

    def wait(self, predicate, seconds=2):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if predicate(): return
            time.sleep(.01)
        self.fail('deadline expired')

    def connect(self):
        s = socket.socket(socket.AF_UNIX); s.settimeout(2); s.connect(str(self.sock)); return s

    def events(self):
        return [json.loads(s)['event'] for s in (self.root / 'events.jsonl').read_text().splitlines()]

    def test_native_client_explicit_ready_then_presented(self):
        self.assertEqual(self.sock.stat().st_mode & 0o777, 0o600)
        p = subprocess.run([str(self.client_binary), str(self.sock)], capture_output=True, timeout=3)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(p.stdout.strip(), b'released')
        self.assertEqual(self.process.wait(timeout=3), 0)
        e = self.events()
        self.assertLess(e.index('splash_test_release'), e.index('splash_presented'))
        self.assertTrue((self.root / 'done').exists())

    def test_timeout_is_static_failure_but_late_ready_still_works(self):
        self.wait(lambda: 'splash_timeout' in self.events())
        self.assertIsNone(self.process.poll())
        self.assertFalse((self.root / 'done').exists())
        p = subprocess.run([str(self.client_binary), str(self.sock)], capture_output=True, timeout=3)
        self.assertEqual(p.returncode, 0)
        self.assertEqual(self.process.wait(timeout=3), 0)

    def test_disconnect_during_handoff_reclaims_display(self):
        with self.connect() as s:
            s.sendall(b'READY 1\n'); self.assertEqual(s.recv(32), b'RELEASED 1\n')
        self.wait(lambda: 'splash_test_claim' in self.events())
        self.assertIn('splash_handoff_aborted', self.events())
        self.assertFalse((self.root / 'done').exists())

    def test_malformed_and_oversized_requests_do_not_release_kms(self):
        for message in (b'PRESENTED 1\n', b'exec reboot\n', b'x'*100):
            with self.connect() as s:
                s.sendall(message)
                try: self.assertEqual(s.recv(32), b'')
                except ConnectionResetError: pass
        self.assertNotIn('splash_test_release', self.events())

    def test_slow_client_deadline_and_singleton(self):
        with self.connect() as s:
            s.settimeout(4); s.sendall(b'R')
            self.assertEqual(s.recv(32), b'')
        p = subprocess.run([str(self.binary), '--test', str(self.root), str(self.sock)], timeout=2)
        self.assertEqual(p.returncode, 0)
        self.assertIsNone(self.process.poll())

    def test_no_service_is_a_compatible_renderer_fallback(self):
        p = subprocess.run([str(self.client_binary), str(self.root / 'missing')], capture_output=True, timeout=3)
        self.assertEqual((p.returncode, p.stdout.strip()), (0, b'absent'))

    def test_client_rejects_wrong_reply(self):
        address = self.root / 'wrong'
        server = socket.socket(socket.AF_UNIX); server.bind(str(address)); server.listen(1)
        def answer():
            with server.accept()[0] as s:
                self.assertEqual(s.recv(32), b'READY 1\n'); s.sendall(b'NOTREADY 1\n')
        thread = threading.Thread(target=answer); thread.start()
        p = subprocess.run([str(self.client_binary), str(address)], capture_output=True, timeout=3)
        thread.join(timeout=3); server.close(); self.assertNotEqual(p.returncode, 0)

    def test_early_quiet_precedes_display_and_splash_follows_charger_gate(self):
        init = (ROOT / 'initramfs/production/init').read_text()
        self.assertLess(init.index('--quiet-if-normal'), init.index('/sbin/y2-platform-start'))
        self.assertLess(init.index("/sbin/y2-offline-charge || rescue"), init.index('/sbin/reborn-splash --start'))
        self.assertLess(init.index('/sbin/reborn-splash --start'), init.index('y2_find_partition'))
        source = (ROOT / 'tools/graphics/reborn-splash.c').read_text()
        self.assertIn('valid==1 && offline==0', source)
        self.assertNotIn('KDSETMODE,KD_TEXT', source)
        self.assertNotIn('system(', source)
        display = (ROOT / 'buildroot/board/y2/overlay/etc/init.d/S25y2-display').read_text()
        self.assertLess(display.index('reborn-splash'), display.index('echo 0'))

    # ---- real milestone -> progress mapping -------------------------------------------------

    def picture(self, *args):
        out = subprocess.check_output([str(self.picture_binary), *args])
        header = b'P6\n480 360\n255\n'
        self.assertTrue(out.startswith(header))
        self.assertEqual(len(out), len(header) + 480 * 360 * 3)
        from PIL import Image
        return Image.frombytes('RGB', (480, 360), out[len(header):])

    def report(self, *tokens):
        lines = subprocess.check_output([str(self.picture_binary), 'report', *tokens], text=True).splitlines()
        return [dict(item.split('=') for item in line.split()[1:]) for line in lines]

    def layout(self):
        path = REBORN / 'docs/ui/previews/v2/boot-layout.json'
        if not path.exists():
            self.skipTest('Reborn boot layout not present')
        return json.loads(path.read_text())

    def test_progress_table_is_generated_from_reborns_phases(self):
        layout = self.layout()
        table = [line.split() for line in subprocess.check_output([str(self.picture_binary), 'table'], text=True).splitlines()]
        self.assertEqual([(t, int(f)) for t, f, _ in table],
                         [(p['token'], p['fill_permille']) for p in layout['phases']])
        fills = [int(f) for _, f, _ in table]
        self.assertEqual((fills[0], fills[-1]), (0, 1000))
        self.assertEqual(fills, sorted(set(fills)), 'strictly increasing coarse positions')
        generated = Path(self.build.name) / 'regenerated.h'
        subprocess.run([sys.executable, str(ROOT / 'tools/graphics/make-splash-mark.py'),
                        str(REBORN / 'docs/ui/previews/v2/boot-layout.json'), str(generated),
                        '--reborn', str(REBORN)], check=True, capture_output=True)
        self.assertEqual(generated.read_text(), (ROOT / 'tools/graphics/reborn-splash-mark.h').read_text(),
                         'reborn-splash-mark.h is stale: rerun tools/graphics/make-splash-mark.py')

    def test_every_milestone_a_producer_reports_is_in_the_table(self):
        tokens = {p['token'] for p in self.layout()['phases']}
        init = (ROOT / 'initramfs/production/init').read_text()
        import re
        stages = set(re.findall(r'^\s*y2_stage (\w+)', init, re.M))
        known = tokens | set(self.layout()['failure_tokens'])
        # Stages the splash does not show: before it exists, or after Reborn owns the screen.
        self.assertEqual(stages - known, {'initramfs'})
        self.assertIn('rescue', stages)
        main = (REBORN / 'app/reborn/src/main.rs').read_text()
        reported = set(re.findall(r'startup_phase\(&log, process_started, "(\w+)"\)', main))
        self.assertEqual(reported - tokens, set(), 'Reborn reports a milestone the splash does not know')
        self.assertTrue({'graphics_ready', 'storage_ready', 'library_workers_ready', 'core_services_ready',
                         'audio_ready', 'radio_workers_ready', 'runtime_ready'} <= reported)
        self.assertIn('boot_milestone("ready")', main)

    def test_progress_never_regresses_and_ignores_unknown_names(self):
        states = self.report('fsck_complete', 'storage_discovery', 'bogus', 'graphics_ready', 'switch_root',
                             'graphics_ready', 'audio_ready', 'x' * 100, 'Rescue', 'ready')
        targets = [int(s['target']) for s in states]
        self.assertEqual(targets, sorted(targets), 'bar target never decreases')
        changed = [s['changed'] for s in states]
        self.assertEqual(changed, ['1', '0', '0', '1', '0', '0', '1', '0', '0', '1'])
        self.assertEqual({s['failed'] for s in states}, {'0'})

    def test_a_missing_milestone_waits_and_a_later_one_jumps_forward(self):
        only_start, skipped = self.report('storage_discovery'), self.report('storage_discovery', 'graphics_ready')
        self.assertEqual(only_start[0]['target'], skipped[0]['target'])
        self.assertGreater(int(skipped[1]['target']), int(skipped[0]['target']))
        # With no report at all the bar never moves on a timer: nothing to step toward.
        out = subprocess.check_output([str(self.picture_binary), 'steps'], text=True)
        self.assertEqual(out.strip(), 'ticks=0 shown=0 target=0')

    def test_failed_milestone_is_reported_once_and_sticks(self):
        states = self.report('storage_discovery', 'rescue', 'rescue', 'graphics_ready')
        self.assertEqual([s['failed'] for s in states], ['0', '1', '1', '1'])
        self.assertEqual([s['changed'] for s in states], ['1', '1', '0', '1'])

    def test_bar_glides_forward_and_settles_quickly(self):
        for tokens, limit in (['ready'], 50), (['radio_workers_ready', 'runtime_ready'], 40), (['storage_discovery'], 24):
            out = subprocess.check_output([str(self.picture_binary), 'steps', *tokens], text=True)
            fields = dict(item.split('=') for item in out.split())
            self.assertEqual(fields['shown'], fields['target'])
            self.assertGreater(int(fields['ticks']), 1, 'smooth, not a jump')
            self.assertLessEqual(int(fields['ticks']), limit, f'{tokens} settles within {limit * 33} ms')

    # ---- pixels --------------------------------------------------------------------------------

    def test_splash_is_pixel_identical_to_reborns_boot_screens(self):
        from PIL import Image
        cases = [('01-boot-splash', ['start']), ('01b-boot-25', ['fsck_complete']),
                 ('01c-boot-60', ['graphics_ready']), ('01d-boot-final-phase', ['runtime_ready']),
                 ('02-boot-handoff', ['ready'])]
        for name, tokens in cases:
            reference = REBORN / f'docs/ui/previews/v2/{name}.png'
            if not reference.exists():
                self.skipTest('Reborn v2 preview not present')
            splash, reborn = self.picture('frame', *tokens), Image.open(reference).convert('RGB')
            a, b = splash.tobytes(), reborn.tobytes()
            differing = sum(a[i:i + 3] != b[i:i + 3] for i in range(0, len(a), 3))
            self.assertEqual(differing, 0, f'{name}: {differing} pixels differ from Reborn')

    def test_ready_repaints_only_what_changed_and_equals_a_full_redraw(self):
        final = self.picture('frame', 'ready').tobytes()
        for earlier in ('start', 'storage_discovery', 'graphics_ready', 'audio_ready', 'runtime_ready'):
            self.assertEqual(self.picture('finish', earlier).tobytes(), final, earlier)

    def test_failure_screen_is_pixel_identical_and_has_no_bar(self):
        reference = REBORN / 'docs/ui/previews/v2/03b-boot-failed.png'
        if not reference.exists():
            self.skipTest('Reborn v2 preview not present')
        from PIL import Image
        self.assertEqual(self.picture('failure').tobytes(), Image.open(reference).convert('RGB').tobytes())

    def test_subpixel_leading_edge_is_a_blend_and_nothing_else_changes(self):
        layout = self.layout()
        bar = layout['bar']
        track, fill = bar['track'], bar['fill']
        y = int(bar['y'])
        half = self.picture('mid', str(int(36.5 * 256)), 'fsck_complete').load()
        x0 = int(bar['x'])
        self.assertEqual(half[x0 + 35, y], tuple(fill.to_bytes(3, 'big')))
        edge = half[x0 + 36, y]
        low, high = tuple(track.to_bytes(3, 'big')), tuple(fill.to_bytes(3, 'big'))
        self.assertTrue(all(min(l, h) <= e <= max(l, h) for l, e, h in zip(low, edge, high)) and edge not in (low, high))
        self.assertEqual(half[x0 + 37, y], low)
        self.assertEqual(half[x0 + 36, y + 1], edge, 'both rows of the thin bar agree')

    def test_no_yellow_and_only_the_current_bar_state_is_drawn(self):
        gold = {(0xe7, 0xc9, 0x8b), (0xff, 0xe2, 0xa4), (0x7d, 0x63, 0x37)}
        for args in (['frame', 'start'], ['frame', 'graphics_ready'], ['frame', 'ready'], ['failure']):
            pixels = set(self.picture(*args).getdata())
            self.assertFalse(pixels & gold, f'{args}: accent colour present')
            self.assertFalse([p for p in pixels if p[0] - p[2] > 0x30], f'{args}: yellow-ish pixel')
        source = (ROOT / 'tools/graphics/reborn-splash.c').read_text()
        for word in ('ACCENT', 'pulse'):
            self.assertNotIn(word, source)
        self.assertNotIn('0xe7c98b', (ROOT / 'tools/graphics/reborn-splash-mark.h').read_text())

    def test_status_text_never_exposes_technical_names(self):
        for phase in self.layout()['phases']:
            for word in ('switch_root', 'systemd', 'drm', 'alsa', 'mount', '/', '_', '%'):
                self.assertNotIn(word, phase['label'].lower())

    # ---- daemon -------------------------------------------------------------------------------

    def write_phase(self, name):
        tmp = self.root / 'phase.tmp'
        tmp.write_text(name + '\n')
        (self.root / 'phase').write_bytes(tmp.read_bytes())

    def phase_events(self, kind='splash_phase'):
        return [json.loads(s) for s in (self.root / 'events.jsonl').read_text().splitlines()
                if json.loads(s)['event'] == kind]

    def test_daemon_follows_reported_milestones_and_ignores_regression(self):
        self.write_phase('storage_discovery')
        self.wait(lambda: len(self.phase_events()) == 1)
        self.write_phase('fsck_complete')
        self.wait(lambda: len(self.phase_events()) == 2)
        self.write_phase('storage_discovery')
        self.wait(lambda: len(self.phase_events('splash_phase_ignored')) == 1)
        self.write_phase('nonsense')
        self.wait(lambda: len(self.phase_events('splash_phase_ignored')) == 2)
        self.assertEqual([e['phase'] for e in self.phase_events()], ['storage_discovery', 'fsck_complete'])

    def test_rescue_milestone_shows_failure_without_waiting_for_the_timeout(self):
        # A fresh daemon that already finds the rescue milestone in its phase file.
        other = tempfile.TemporaryDirectory(prefix='rs-')
        self.addCleanup(other.cleanup)
        root = Path(other.name)
        (root / 'phase').write_text('rescue\n')
        process = subprocess.Popen([str(self.binary), '--test', str(root), str(root / 'control.sock')])
        def stop():
            process.terminate()
            process.wait(timeout=3)
        self.addCleanup(stop)
        end = time.monotonic() + 2
        state = None
        while time.monotonic() < end and state != 'failed':
            try:
                state = json.loads((root / 'state.json').read_text())['state']
            except (OSError, ValueError):
                pass
            time.sleep(.005)
        self.assertEqual(state, 'failed')
        self.assertLess(time.monotonic() - (end - 2), .2, 'well before the 250 ms test timeout would matter')

    def test_ready_then_presented_leaves_no_splash_behind_and_does_not_wait(self):
        self.write_phase('graphics_ready')
        self.wait(lambda: len(self.phase_events()) == 1)
        with self.connect() as s:
            started = time.monotonic()
            s.sendall(b'READY 1\n')
            self.assertEqual(s.recv(32), b'RELEASED 1\n')
            self.assertLess(time.monotonic() - started, .2, 'release is not delayed by the bar')
            released = time.monotonic()
            s.sendall(b'PRESENTED 1\n')
            self.assertEqual(self.process.wait(timeout=1), 0)
            self.assertLess(time.monotonic() - released, .3, 'the splash exits as soon as it is acknowledged')
        self.assertTrue((self.root / 'done').exists())
        self.assertFalse(self.sock.exists(), 'no stale control socket survives the hand-off')

    def test_timeout_counts_from_the_last_milestone_not_from_the_start(self):
        # The test daemon times out 250 ms after its last milestone.
        end = time.monotonic() + 1.0
        for token in ('storage_discovery', 'rescue_update', 'storage_preflight', 'fsck_start',
                      'fsck_complete', 'root_data_mounted', 'switch_root', 'model_restored'):
            self.write_phase(token)
            time.sleep(.08)
        self.assertGreater(time.monotonic() - (end - 1.0), .5, 'longer than one timeout in total')
        self.assertNotIn('splash_timeout', self.events(), 'a progressing startup is not a failure')
        self.wait(lambda: 'splash_timeout' in self.events())

    def test_idle_splash_does_not_wake_up_to_animate(self):
        self.wait(lambda: 'splash_timeout' in self.events())
        def switches():
            fields = dict(line.split(':', 1) for line in Path(f'/proc/{self.process.pid}/status').read_text().splitlines()
                          if 'ctxt_switches' in line)
            return sum(int(v) for v in fields.values())
        before = switches()
        time.sleep(.8)
        self.assertLessEqual(switches() - before, 3, 'no polling loop while startup is quiet')

    def test_a_dropped_phase_file_is_not_followed_through_symlinks(self):
        (self.root / 'phase').symlink_to('/etc/hostname')
        time.sleep(.2)
        self.assertEqual(self.phase_events(), [])
        self.assertIsNone(self.process.poll())


if __name__ == '__main__': unittest.main()
