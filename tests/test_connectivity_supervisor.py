"""Radio lifecycle, restart bounds and fd sleep against the production monitor."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.connectivity import HELPERS, Supervisor, alive


class ConnectivitySupervisor(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root/'run/y2').mkdir(parents=True)
        (self.root/'sys/class/net').mkdir(parents=True)
        self.calls = []
        self.answers = []
        def command(argv, **kwargs):
            self.calls.append(argv)
            return self.answers.pop(0) if self.answers else {'ok': True, 'output': 'OK'}
        self.ctx = Context(self.root, runner=command)
        self.monitor = Supervisor(self.ctx)

    def test_healthy_radio_off_uses_native_reads_and_no_commands(self):
        with patch('y2_platform.connectivity.alive', return_value=os.getpid()):
            self.monitor.health(0)
            self.monitor.interface(0)
            self.monitor.health(20)
            self.monitor.health(30)
        self.assertEqual(self.calls, [])
        self.assertFalse(self.monitor.attached)

    def test_missing_helper_crash_events_cannot_bypass_restart_rate(self):
        with patch('y2_platform.connectivity.alive', return_value=None), \
             patch.object(self.monitor, 'restart') as restart:
            self.monitor.health(0)
            self.assertEqual(restart.call_count, 4)
            self.monitor.health(1, force=True)
            self.monitor.health(29, force=True)
            self.assertEqual(restart.call_count, 4)
            self.monitor.health(30, force=True)
            self.assertEqual(restart.call_count, 8)

    def test_stale_reused_pid_is_not_a_healthy_helper(self):
        name = 'wpa'
        (self.root/'run/y2/wpa.pid').write_text(str(os.getpid()))
        proc = self.root/f'proc/{os.getpid()}'
        proc.mkdir(parents=True)
        (proc/'exe').symlink_to('/different-program')
        self.assertIsNone(alive(self.ctx, name, HELPERS[name]))
        (proc/'exe').unlink()
        (proc/'exe').symlink_to(self.root/'usr/sbin/wpa_supplicant')
        self.assertEqual(alive(self.ctx, name, HELPERS[name]), os.getpid())

    def test_warm_restart_reuses_supplicant_interface_and_action_hook(self):
        (self.root/'sys/class/net/wlan0').mkdir()
        self.answers = [{'ok': False, 'output': 'FAIL'}, {'ok': True, 'output': 'wlan0'},
                        {'ok': True, 'output': 'OK'}, {'ok': True, 'output': ''}]
        self.monitor.interface(0)
        self.assertTrue(self.monitor.attached)
        # A missing AP must back off its scans instead of rescanning every ~5 s.
        self.assertEqual(self.calls[-2], ['wpa_cli','-i','wlan0','autoscan','exponential:2:120'])
        self.assertEqual(self.calls[-1], ['wpa_cli','-i','wlan0','-a',
                         '/usr/libexec/y2/wpa-event','-B','-P','/run/y2/wifi-action.pid'])
        self.monitor.interface(60)
        self.assertEqual(len(self.calls), 4)

    def test_netdev_removal_runs_disconnect_cleanup_once(self):
        self.monitor.attached = True
        self.monitor.interface(0)
        self.monitor.interface(30)
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(self.calls[0], ['/usr/libexec/y2/wpa-event','wlan0','DISCONNECTED'])
        self.assertEqual(self.calls[-1][-2:], ['interface_remove','wlan0'])
        self.assertFalse(self.monitor.attached)

    def test_a_new_netdev_under_the_old_name_gets_a_new_supplicant_interface(self):
        # F53: a removal missed between two looks left the supplicant's old
        # interface on the new wlan0 (INTERFACE_DISABLED for 106 min).
        (self.root/'sys/class/net/wlan0').mkdir()
        (self.root/'sys/class/net/wlan0/ifindex').write_text('5\n')
        self.monitor.interface(0)
        self.assertEqual((self.monitor.attached, self.monitor.ifindex), (True, '5'))
        self.calls.clear()
        self.monitor.interface(10)
        self.assertEqual(self.calls, [])
        (self.root/'sys/class/net/wlan0/ifindex').write_text('7\n')
        self.monitor.interface(20)
        self.assertEqual(self.calls[0], ['/usr/libexec/y2/wpa-event','wlan0','DISCONNECTED'])
        self.assertEqual(self.calls[2][-2:], ['interface_remove','wlan0'])
        self.assertEqual(self.calls[3][3:5], ['interface_add','wlan0'])
        self.assertEqual((self.monitor.attached, self.monitor.ifindex), (True, '7'))

    def test_interface_left_disabled_with_radio_on_is_added_again(self):
        import json
        (self.root/'proc/sys/kernel/random').mkdir(parents=True)
        (self.root/'proc/sys/kernel/random/boot_id').write_text('boot-one')
        rfkill = self.root/'sys/class/rfkill/rfkill1'
        rfkill.mkdir(parents=True)
        (rfkill/'type').write_text('wlan\n')
        (rfkill/'soft').write_text('0\n')
        def network(association):
            (self.root/'run/y2/network.json').write_text(json.dumps({
                'boot_id': 'boot-one', 'monotonic_s': time.monotonic(), 'association': association,
                'radio_present': True}))
        network('INTERFACE_DISABLED')
        self.monitor.attached = True
        self.monitor.verify(100)
        self.monitor.verify(129)
        self.assertEqual(self.calls, [])
        self.monitor.verify(130)
        self.assertEqual(self.calls[-1][-2:], ['interface_remove','wlan0'])
        self.assertFalse(self.monitor.attached)
        # Backed off when the new interface is disabled again.
        self.calls.clear()
        self.monitor.attached = True
        self.monitor.verify(131)
        self.monitor.verify(161)
        self.assertEqual(self.calls, [])
        self.monitor.verify(190)
        self.assertEqual(len(self.calls), 3)
        # A radio switched off (sleep, the owner) or a stale record is no reason.
        self.calls.clear()
        self.monitor.attached = True
        (rfkill/'soft').write_text('1\n')
        for now in (1000, 1100, 1200):
            self.monitor.verify(now)
        (rfkill/'soft').write_text('0\n')
        (self.root/'run/y2/network.json').write_text(json.dumps({
            'boot_id': 'boot-one', 'monotonic_s': time.monotonic() - 60, 'association': 'INTERFACE_DISABLED',
            'radio_present': True}))
        for now in (1300, 1400):
            self.monitor.verify(now)
        network('DISCONNECTED')
        for now in (1500, 1600):
            self.monitor.verify(now)
        self.assertEqual(self.calls, [])

    def test_failed_interface_attach_has_bounded_backoff(self):
        (self.root/'sys/class/net/wlan0').mkdir()
        self.answers = [{'ok': False, 'output': ''}] * 50
        self.monitor.interface(0)
        self.monitor.interface(1)
        self.assertEqual(len(self.calls), 2)
        self.monitor.interface(2)
        self.assertEqual(len(self.calls), 4)
        for now in (6,14,30,62,122):
            self.monitor.interface(now)
        self.assertEqual(self.monitor.retry_delay, 60)
        self.assertEqual(self.monitor.retry_at, 182)

    def test_wpa_restart_preserves_hooks_and_reconnect_single_owner(self):
        self.monitor.attached = True
        self.monitor.restart('wpa')
        self.assertFalse(self.monitor.attached)
        self.assertIn(['/usr/sbin/y2-platform','wifi-event','SERVICE_RESTART'], self.calls)
        self.monitor.restart('reconnect')
        self.assertIn('/usr/sbin/y2-bt-reconnect', self.calls[-1])
        self.assertFalse(any('connect' in call or 'bluetoothctl' in call for call in self.calls))

    def test_codec_policy_failure_does_not_tear_down_other_helpers(self):
        with patch('y2_platform.connectivity.codec_runtime', side_effect=ValueError):
            self.assertFalse(self.monitor.restart('bluealsa')['ok'])
        self.assertEqual(self.calls, [])

    def test_idle_fd_wait_is_signal_interruptible_and_cleans_up(self):
        marker = self.root/'ran'
        code = "from pathlib import Path; from y2_platform.common import Context; from y2_platform.connectivity import serve; "
        code += f"ctx=Context({str(self.root)!r},runner=lambda *a,**k: (Path({str(marker)!r}).write_text('tick'), {{'ok':False,'output':''}})[1]); serve(ctx)"
        env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parents[1]/'tools/platform'))
        child = subprocess.Popen([sys.executable,'-c',code], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic()+5
            while not marker.exists() and child.poll() is None and time.monotonic()<deadline:
                time.sleep(.02)
            self.assertTrue(marker.exists())
            time.sleep(.1)
            started = time.monotonic()
            child.send_signal(signal.SIGTERM)
            _, error = child.communicate(timeout=2)
            self.assertEqual(child.returncode, 0, error.decode())
            self.assertLess(time.monotonic()-started, 1)
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()
