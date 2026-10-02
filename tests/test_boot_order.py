"""Boot order: Reborn starts last behind a bounded readiness gate, every service is a
splash milestone, and the early-boot waits that cost seconds are gone. The shell
scripts run for real against fake file trees; no device is touched."""
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
OVERLAY = ROOT / 'buildroot/board/y2/production-overlay'
GATE = OVERLAY / 'usr/libexec/y2/boot-gate'
RCS = OVERLAY / 'etc/init.d/rcS'
SH = shutil.which('busybox') and ['busybox', 'sh'] or ['sh']


def rooted(text, root):
    """Point the script's absolute runtime paths at a scratch root."""
    for path in ('/etc/init.d', '/run', '/data', '/sys/devices', '/sys/class', '/dev/kmsg'):
        text = text.replace(path, str(root) + path)
    # The real file holds the latest milestone; the test copy keeps every one.
    return text.replace('> "$phase"', '>> "$phase"').replace('> ' + str(root) + '/run/reborn-splash/phase',
                                                              '>> ' + str(root) + '/run/reborn-splash/phase')


class Tree:
    def __init__(self):
        self.temp = tempfile.TemporaryDirectory(prefix='bo-')
        self.sockets = []
        self.root = Path(self.temp.name)
        for directory in ('run/y2/factory', 'run/reborn-splash', 'run/dbus', 'data/network', 'data/bluetooth',
                          'sys/devices/platform/18070000.connectivity', 'sys/class/net', 'sys/class/bluetooth',
                          'etc/init.d', 'dev'):
            (self.root / directory).mkdir(parents=True)
        (self.root / 'dev/kmsg').write_text('')

    def path(self, name):
        return self.root / name

    def socket(self, name):
        """A real listening UNIX socket: the gate tests for sockets, not files."""
        path = self.path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        sock = socket.socket(socket.AF_UNIX)
        sock.bind(str(path))
        self.sockets.append(sock)

    sockets = []

    def put(self, name, text=''):
        path = self.path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def script(self, source, name):
        path = self.root / name
        path.write_text(rooted(source.read_text(), self.root))
        path.chmod(0o755)
        return path

    def phases(self):
        try:
            return (self.root / 'run/reborn-splash/phase').read_text().split()
        except OSError:
            return []

    def phase(self):
        return (self.phases() or [''])[-1]


class BootOrder(unittest.TestCase):
    def setUp(self):
        self.tree = Tree()
        self.addCleanup(self.tree.temp.cleanup)

    def run_gate(self, stages, limit=20, radios=()):
        """Run the real gate while `stages` (delay, callable) bring the system up."""
        tree = self.tree
        for name in radios:
            tree.put(f'data/{name}/enabled', '1\n')
        sleeper = subprocess.Popen(['sleep', '60'])
        self.addCleanup(sleeper.kill)
        tree.put('run/y2/connectivity.pid', f'{sleeper.pid}\n')
        gate = tree.script(GATE, 'gate')
        def bring_up():
            for delay, action in stages:
                time.sleep(delay)
                action()
        helper = threading.Thread(target=bring_up)
        helper.start()
        started = time.monotonic()
        result = subprocess.run([*SH, str(gate), str(limit)], capture_output=True, text=True, timeout=60)
        elapsed = time.monotonic() - started
        helper.join()
        self.assertEqual(result.returncode, 0, result.stderr)
        return tree.phases(), elapsed

    # ---- order ---------------------------------------------------------------------------------------

    def test_reborn_is_installed_last_and_everything_else_runs_before_it(self):
        package = ROOT / 'buildroot/package/reborn'
        self.assertTrue((package / 'S90reborn').is_file())
        self.assertFalse((package / 'S05reborn').exists())
        recipe = (package / 'reborn.mk').read_text()
        self.assertIn('/etc/init.d/S90reborn', recipe)
        self.assertNotIn('install -D -m 0755 $(REBORN_PKGDIR)/S05reborn', recipe)
        numbers = [int(m.group(1)) for m in
                   (re.match(r'S(\d\d)', p.name) for p in (OVERLAY / 'etc/init.d').iterdir()) if m]
        self.assertLess(max(numbers), 90)
        script = (package / 'S90reborn').read_text()
        self.assertLess(script.index('boot-gate'), script.index('start-stop-daemon -S'))
        for source in ('tools/platform/y2_platform/power.py', 'tools/platform/y2_platform/maintenance.py'):
            text = (ROOT / source).read_text()
            self.assertIn('S90reborn', text)
            self.assertNotIn('S05reborn', text)

    def test_rcs_runs_scripts_in_order_reports_milestones_and_keeps_a_trace(self):
        tree = self.tree
        order = tree.path('run/ran')
        for name in ('S01syslogd', 'S02y2-data', 'S03seedrng', 'S40bluetoothd', 'S42y2-readiness', 'S90reborn'):
            script = tree.path('etc/init.d') / name
            script.write_text(f'#!/bin/sh\necho "$0 $1" >> {order}\n')
            script.chmod(0o755)
        (tree.path('etc/init.d') / 'S99dangling').symlink_to(tree.path('missing'))
        rcs = tree.script(RCS, 'etc/init.d/rcS')
        result = subprocess.run([*SH, str(rcs)], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        ran = [line.split()[0].rsplit('/', 1)[1] for line in order.read_text().splitlines()]
        self.assertEqual(ran, ['S01syslogd', 'S02y2-data', 'S03seedrng', 'S40bluetoothd', 'S42y2-readiness', 'S90reborn'])
        trace = [__import__('json').loads(line) for line in tree.path('run/y2/boot-trace.jsonl').read_text().splitlines()]
        self.assertEqual([t['script'] for t in trace], ran)
        self.assertTrue(all(t['end_s'] >= t['start_s'] for t in trace))
        # The last reported service milestone is the last one with a name here.
        self.assertEqual(tree.phase(), 'rc_readiness')

    # ---- gate ----------------------------------------------------------------------------------------

    def test_gate_reports_each_real_step_and_waits_for_what_reborn_needs(self):
        tree = self.tree
        stages = [
            (.15, lambda: tree.socket('run/y2/power.sock')),
            (.0, lambda: tree.put('run/y2/power.json', '{}\n')),
            (.0, lambda: tree.put('run/y2/network.json', '{}\n')),
            (.25, lambda: tree.socket('run/dbus/system_bus_socket')),
        ]
        seen, elapsed = self.run_gate(stages)
        self.assertEqual(seen, ['platform_ready', 'system_ready'])
        self.assertGreater(elapsed, .35, 'did not wait for the platform and the bus')
        self.assertLess(elapsed, 3, 'no fixed hold once everything is up')

    def test_radios_are_only_waited_for_when_enabled_and_each_step_is_reported(self):
        tree = self.tree
        status = 'sys/devices/platform/18070000.connectivity/status'
        tree.put(status, 'activated=0\n')
        def platform():
            tree.put('run/y2/power.json', '{}\n'); tree.put('run/y2/network.json', '{}\n')
            tree.socket('run/y2/power.sock'); tree.socket('run/dbus/system_bus_socket')
        stages = [
            (.1, platform),
            (.3, lambda: tree.put('run/y2/factory/ready')),
            (.3, lambda: tree.put('run/y2/calibration-ready')),
            (.3, lambda: tree.put(status, 'activated=1 mode=full\n')),
            (.3, lambda: (tree.socket('run/wpa_supplicant/global'),
                          tree.path('sys/class/net/wlan0').mkdir())),
            (.3, lambda: (tree.path('sys/class/bluetooth/hci0').mkdir(), tree.put('run/y2/bluealsa.pid', '1\n'))),
        ]
        seen, elapsed = self.run_gate(stages, radios=('network', 'bluetooth'))
        self.assertEqual(seen, ['platform_ready', 'conn_factory', 'conn_calibration', 'conn_activated',
                                'conn_wifi_wait', 'conn_wifi', 'conn_bluetooth_wait', 'conn_bluetooth', 'system_ready'])
        self.assertGreater(elapsed, 1.5)

    def test_disabled_radios_cost_no_wait(self):
        tree = self.tree
        def platform():
            tree.put('run/y2/power.json', '{}\n'); tree.put('run/y2/network.json', '{}\n')
            tree.socket('run/y2/power.sock'); tree.socket('run/dbus/system_bus_socket')
        seen, elapsed = self.run_gate([(.1, platform)])
        self.assertEqual(seen, ['platform_ready', 'system_ready'])
        self.assertLess(elapsed, 2)

    def test_a_dead_connectivity_service_does_not_hold_boot(self):
        tree = self.tree
        def platform():
            tree.put('run/y2/power.json', '{}\n'); tree.put('run/y2/network.json', '{}\n')
            tree.socket('run/y2/power.sock'); tree.socket('run/dbus/system_bus_socket')
        victim = subprocess.Popen(['sleep', '60'])
        stages = [(.1, platform), (.2, lambda: (victim.kill(), victim.wait()))]
        tree.put('data/network/enabled', '1\n')
        sleeper = victim
        tree.put('run/y2/connectivity.pid', f'{sleeper.pid}\n')
        gate = tree.script(GATE, 'gate')
        threading.Timer(.1, platform).start()
        threading.Timer(.4, lambda: (victim.kill(), victim.wait())).start()
        started = time.monotonic()
        result = subprocess.run([*SH, str(gate), '30'], capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLess(time.monotonic() - started, 5, 'gave up on radios as soon as their service was gone')
        self.assertEqual(tree.phase(), 'system_ready')

    def test_gate_never_blocks_boot_for_good(self):
        seen, elapsed = self.run_gate([], limit=1)
        self.assertEqual(seen[-1], 'system_ready')
        self.assertLess(elapsed, 3.5)
        self.assertNotIn('platform_ready', seen)

    # ---- early-boot waits ----------------------------------------------------------------------------

    def test_initramfs_credits_the_seed_before_anything_waits_for_randomness(self):
        init = (ROOT / 'initramfs/production/init').read_text()
        credit = init.index('seedrng --seed-dir=/newroot/data/system/entropy')
        self.assertLess(init.index('/newroot/data ||'), credit)
        self.assertLess(credit, init.index('y2_stage root_data_mounted'))
        self.assertLess(credit, init.index('exec switch_root'))
        # The same per-device seed directory the installed system uses.
        self.assertIn('--seed-dir=/data/system/entropy',
                      (ROOT / 'buildroot/board/y2/post-build-production.sh').read_text())

    def test_storage_discovery_polls_in_small_steps(self):
        init = (ROOT / 'initramfs/production/init').read_text()
        loop = init[init.index('# Allow asynchronous controller discovery'):init.index('[ -n "$datadev" ] || rescue')]
        self.assertIn('sleep 0.05', loop)
        self.assertNotIn('sleep 1\n', loop)
        self.assertIn('-lt 800', loop, 'still waits about 40 s in total')

    def test_platform_records_do_not_need_the_random_pool(self):
        sys.path.insert(0, str(ROOT / 'tools/platform'))
        from y2_platform import common
        self.assertNotIn('import tempfile', Path(common.__file__).read_text())
        real = os.urandom
        def blocked(*args, **kwargs):
            raise AssertionError('atomic_json asked for randomness')
        os.urandom = blocked
        self.addCleanup(setattr, os, 'urandom', real)
        target = self.tree.path('record.json')
        common.atomic_json(target, {'a': 1}, durable=True)
        common.atomic_json(target, {'a': 2})
        self.assertEqual(target.read_text().strip(), '{"a":2}')
        self.assertEqual(oct(target.stat().st_mode & 0o777), '0o600')
        self.assertEqual([p.name for p in target.parent.iterdir() if p.name.startswith('.record')], [])


if __name__ == '__main__':
    unittest.main()
