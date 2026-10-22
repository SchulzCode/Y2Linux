"""Exercise real lock ownership, policy refusal and complete product PM flows."""
import fcntl
import errno
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from y2_platform.common import Context
from y2_platform import product_sleep as pm
from y2_platform.sleep import request
from y2_platform.suspend_record import record
from y2_platform.workload import workload


class ProductAutoSleep(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.calls = []
        self.rpc = []
        self.ctx = Context(self.tmp.name, runner=self.command)
        for name, text in {
            '/proc/sys/kernel/random/boot_id': 'same-boot',
            '/proc/sys/kernel/osrelease': 'corrected-kernel',
            '/proc/sys/kernel/tainted': '0',
            '/etc/y2linux/versions.json': '{"build_git_commit":"corrected-source"}',
            '/sys/power/pm_test': '[none] core',
            '/sys/power/state': 'mem', '/sys/power/wakeup_count': '8',
            '/sys/firmware/y2_pm/stage': '',
            '/sys/firmware/y2_pm/backstop_s': 'provider=1 armed_s=0 running=0',
            '/sys/class/power_supply/y2-usb-presence/online': '0',
            '/sys/class/power_supply/BAT0/status': 'Discharging',
            '/sys/class/power_supply/BAT0/voltage_now': '3900000',
            '/sys/module/workload/parameters/leases': '',
            '/sys/devices/platform/18070000.connectivity/status': 'powered=0 functions=0 error=0 ',
            '/data/network/enabled': '0', '/data/bluetooth/enabled': '0',
            '/data/.y2data-schema': '1', '/run/y2/suspend.lock': '',
        }.items():
            self.write(name, text)
        self.ctx.path('/data/updates').mkdir()
        self.write('/etc/y2linux/power-policy.json', (ROOT / 'buildroot/board/y2/production-overlay/etc/y2linux/power-policy.json').read_text())
        self.write('/run/y2/power.json', json.dumps({'boot_id': 'same-boot', 'state': 'Normal',
                                                   'monotonic_ns': time.monotonic_ns()}))
        self.power, self.rtc = 2, 1
        self.irqs()
        self.spm(False, 0)
        self.write('/sys/firmware/y2_pm/state', 'valid=0 stage=EXIT error=0')
        self.fd = os.open(self.ctx.path('/run/y2/suspend.lock'), os.O_RDWR)
        self.addCleanup(os.close, self.fd)
        fcntl.flock(self.fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.addCleanup(patch.stopall)
        patch('y2_platform.suspend_record.application_rpc', side_effect=self.application).start()

    def write(self, name, value):
        path = self.ctx.path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    def command(self, argv, **kwargs):
        self.calls.append(argv)
        return {'ok': True, 'output': ''}

    def application(self, ctx, op):
        self.rpc.append(op)
        return {'sleep_ready': True, 'screen_off': op == 'sleep_prepare'}

    def irqs(self):
        self.write('/proc/interrupts', f'  66: {self.rtc} 0 mt6323 20 Level mt6397-rtc\n'
                   f'  70: {self.power} 0 mt6323 5 Level mtk-pmic-keys\n')

    def spm(self, timer, ticks):
        self.write('/sys/bus/platform/devices/10006000.power-controller/state',
                   f'system_sleep_abi=1 broken=0 timer_wake={int(timer)} ticks32k={ticks}')

    def returned(self, timer=False, rtc=False):
        if not timer:
            if rtc:
                self.rtc += 1
            else:
                self.power += 1
        self.irqs()
        self.spm(timer, (600 if timer else 25) * 32768)
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=EXIT error=0 wake=' +
                   ('0x1' if timer else '0x20') + ' reset_resume_entry=0x59325253 pcm_length=596')

    def run_sleep(self, enter):
        record(self.ctx, 'requested', 0, mode='product')
        return pm.execute(self.ctx, self.fd, enter)

    def test_timer_maintenance_keeps_app_quiesced_then_power_fully_restores(self):
        calls = []
        def enter(ctx):
            calls.append(1)
            self.assertEqual(self.rpc, ['sleep_prepare'])
            self.returned(timer=len(calls) == 1)
        with patch('y2_platform.product_sleep.time.sleep'):
            self.assertEqual(self.run_sleep(enter), 0)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['cycles'], 2)
        self.assertEqual(receipt['maintenance_wakes'], 1)
        self.assertEqual(receipt['suspended_seconds'], 625)
        self.assertLess(receipt['first_entry']['boottime_s'], receipt['entry']['boottime_s'])
        self.assertEqual(receipt['first_entry']['voltage_uv'], 3900000)
        self.assertEqual(receipt['product']['wake_reason'], 'power')
        self.assertEqual(receipt['product']['state'], 'restored')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])
        record(self.ctx, 'requested', 0, mode='product')
        self.assertNotIn('first_entry', self.ctx.json('/data/system/platform/suspend-last.json'))

    def test_playback_may_start_while_the_radios_come_back(self):
        # F24: a wired open right after a wake (shared activity lease) was refused as
        # busy until the radio restore had finished.
        self.write('/data/network/enabled', '1')
        lease = []
        def command(argv, **kwargs):
            self.calls.append(argv)
            if argv[-1] == 'on-runtime':
                fd = os.open(self.ctx.path('/run/y2/activity.lock'), os.O_RDWR)
                try:
                    fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
                    lease.append('free')
                except BlockingIOError:
                    lease.append('held')
                finally:
                    os.close(fd)
                # The firmware/update and maintenance locks stay held.
                fd = os.open(self.ctx.path('/data/system/platform/.maintenance.lock'), os.O_RDWR)
                try:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
                finally:
                    os.close(fd)
            return {'ok': True, 'output': ''}
        self.ctx.runner = command
        self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)
        self.assertEqual(lease, ['free'])

    def test_sleep_before_application_ready_waits_for_it_then_sleeps(self):
        # 4fed1b6a (8.12): sleep 8 s after Reborn started, power features not yet
        # armed, then a hang right after resume. The kernel entry now waits.
        self.write('/data/system/platform/boot.json', json.dumps({'boot_id': 'same-boot', 'last_stage': 'services_started'}))
        ticks = []
        def tick(seconds):
            ticks.append(seconds)
            if len(ticks) == 3:
                self.write('/data/system/platform/boot.json',
                           json.dumps({'boot_id': 'same-boot', 'last_stage': 'application_ready'}))
        entered = []
        def enter(ctx):
            entered.append(len(ticks))
            self.returned()
        with patch('y2_platform.product_sleep.time.sleep', side_effect=tick):
            self.assertEqual(self.run_sleep(enter), 0)
        self.assertEqual(entered, [3])
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        stages = [s['stage'] for s in receipt['stages']]
        self.assertLess(stages.index('reborn_prepared'), stages.index('waiting_application_ready'))
        self.assertLess(stages.index('waiting_application_ready'), stages.index('quiescing_radios'))
        # Durable rows around the screen restore locate a later stall.
        self.assertLess(stages.index('kernel_resumed'), stages.index('screen_restoring'))
        self.assertLess(stages.index('screen_restoring'), stages.index('screen_restored'))
        self.assertLess(stages.index('screen_restored'), stages.index('restoring_radios'))
        self.assertEqual(receipt['product']['state'], 'restored')

    def test_the_wait_for_application_ready_is_bounded_and_a_power_press_cancels_it(self):
        self.write('/data/system/platform/boot.json', json.dumps({'boot_id': 'same-boot', 'last_stage': 'platform_start'}))
        clock = [1000.0]
        def tick(seconds):
            clock[0] += seconds
        with patch('y2_platform.product_sleep.time.sleep', side_effect=tick), \
             patch('y2_platform.product_sleep.time.monotonic', side_effect=lambda: clock[0]):
            self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)
        self.assertGreaterEqual(clock[0] - 1000, pm.APPLICATION_READY_WAIT_S)
        self.assertEqual(self.ctx.json('/data/system/platform/suspend-last.json')['product']['state'], 'restored')
        # Power during the wait: no kernel entry, the screen comes back.
        self.rpc.clear()
        self.write('/sys/firmware/y2_pm/backstop_s', 'provider=1 armed_s=0 running=0')
        def press(seconds):
            self.power += 1
            self.irqs()
        entered = []
        with patch('y2_platform.product_sleep.time.sleep', side_effect=press):
            self.assertEqual(self.run_sleep(lambda ctx: entered.append(1)), 2)
        self.assertEqual(entered, [])
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual((receipt['product']['state'], receipt['product']['reason']), ('refused', 'wake_event_in_progress'))
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_rtc_restores_without_automatic_reentry(self):
        self.assertEqual(self.run_sleep(lambda ctx: self.returned(rtc=True)), 0)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['cycles'], 1)
        self.assertEqual(receipt['product']['wake_reason'], 'rtc')

    def test_battery_notification_during_maintenance_uses_a_fresh_handshake(self):
        entries = []
        writes = []
        original_write = pm.control_write

        def kernel_control(ctx, name, value):
            if name == '/sys/power/wakeup_count':
                writes.append(value.strip())
                # Model pm_save_wakeup_count: a completed event since the
                # read invalidates the store even when no IRQ stays active.
                if value.strip() != ctx.read(name):
                    raise OSError(errno.EINVAL, 'intervening battery notification')
            original_write(ctx, name, value)

        def enter(ctx):
            entries.append(1)
            self.returned(timer=len(entries) == 1, rtc=len(entries) != 1)

        def safety_tick(seconds):
            self.assertEqual(seconds, 2)
            # A routine post-resume BAT0 notification: no cable, Power or
            # RTC event, and the normal battery safety owner stays healthy.
            self.write('/sys/power/wakeup_count', '9')
            self.write('/run/y2/power.json', json.dumps({
                'boot_id': 'same-boot', 'state': 'Normal',
                'monotonic_ns': time.monotonic_ns()}))

        with patch.object(pm, 'control_write', side_effect=kernel_control), \
                patch('y2_platform.product_sleep.time.sleep', side_effect=safety_tick):
            self.assertEqual(self.run_sleep(enter), 0,
                             self.ctx.json('/data/system/platform/suspend-last.json').get('product'))
        self.assertEqual(len(entries), 2)
        self.assertEqual(writes, ['8', '9'])
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['maintenance_wakes'], 1)
        self.assertEqual(receipt['product']['wake_reason'], 'rtc')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])

    def test_power_press_after_maintenance_read_still_cancels_entry(self):
        entries = []
        stores = []
        original_write = pm.control_write

        def kernel_control(ctx, name, value):
            if name == '/sys/power/wakeup_count':
                stores.append(value.strip())
                if len(stores) == 2:
                    self.write(name, '10')  # Power pressed after the fresh read
                    self.power += 1
                    self.irqs()
                if value.strip() != ctx.read(name):
                    raise OSError(errno.EINVAL, 'new wakeup before entry')
            original_write(ctx, name, value)

        def enter(ctx):
            entries.append(1)
            self.returned(timer=True)

        with patch.object(pm, 'control_write', side_effect=kernel_control), \
                patch('y2_platform.product_sleep.time.sleep'):
            self.assertEqual(self.run_sleep(enter), 2)
        self.assertEqual(len(entries), 1)
        self.assertEqual(len(stores), 2)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['product']['reason'], 'wake_event_in_progress')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])

    def test_battery_notification_during_preparation_takes_a_fresh_count(self):
        # Archive 156, 10:31: Power to sleep; a BAT0 notification during the
        # ≈6 s preparation invalidated the first count and the sleep was refused.
        stores = []
        original_write = pm.control_write

        def kernel_control(ctx, name, value):
            if name == '/sys/power/wakeup_count':
                stores.append(value.strip())
                if value.strip() != ctx.read(name):
                    raise OSError(errno.EINVAL, 'intervening battery notification')
            original_write(ctx, name, value)

        def preparation(ctx, op):
            if op == 'sleep_prepare':
                self.write('/sys/power/wakeup_count', '9')
            return self.application(ctx, op)

        with patch.object(pm, 'control_write', side_effect=kernel_control), \
                patch('y2_platform.suspend_record.application_rpc', side_effect=preparation):
            self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)
        self.assertEqual(stores, ['8', '9'])
        self.assertEqual(self.ctx.json('/data/system/platform/suspend-last.json')['product']['wake_reason'], 'power')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])

    def test_cable_during_preparation_cancels_instead_of_retrying(self):
        original_write = pm.control_write

        def kernel_control(ctx, name, value):
            if name == '/sys/power/wakeup_count' and value.strip() != ctx.read(name):
                raise OSError(errno.EINVAL, 'cable event')
            original_write(ctx, name, value)

        def preparation(ctx, op):
            if op == 'sleep_prepare':
                self.write('/sys/power/wakeup_count', '9')
                self.write('/sys/class/power_supply/y2-usb-presence/online', '1')
                self.write('/sys/class/power_supply/BAT0/status', 'Charging')
            return self.application(ctx, op)

        with patch.object(pm, 'control_write', side_effect=kernel_control), \
                patch('y2_platform.suspend_record.application_rpc', side_effect=preparation):
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('slept with a cable')), 2)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['product']['reason'], 'charger_or_usb_active')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])

    def test_continuous_events_refuse_after_bounded_retries(self):
        stores = []
        original_write = pm.control_write

        def kernel_control(ctx, name, value):
            if name == '/sys/power/wakeup_count':
                stores.append(value.strip())
                self.write(name, str(int(value) + 1))  # an event before every store
                raise OSError(errno.EINVAL, 'busy wake source')
            original_write(ctx, name, value)

        with patch.object(pm, 'control_write', side_effect=kernel_control):
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('entered while busy')), 2)
        self.assertEqual(len(stores), pm.STALE_COUNT_RETRIES + 1)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['product']['reason'], 'wake_event_in_progress')
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])

    def test_kernel_busy_restores_once_without_claiming_sleep_or_retrying(self):
        calls = []
        def busy(ctx):
            calls.append(1)
            raise OSError(errno.EBUSY, 'wake event during device suspend')
        self.assertEqual(self.run_sleep(busy), 2)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(calls, [1])
        self.assertEqual(receipt['product']['reason'], 'kernel_suspend_refused')
        self.assertFalse(receipt['product']['kernel_completed'])
        self.assertNotIn('cycles', receipt)
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_power_press_during_preparation_cancels_entry_and_restores(self):
        original = self.application
        def preparation(ctx, op):
            if op == 'sleep_prepare':
                self.power += 1
                self.irqs()
            return original(ctx, op)
        with patch('y2_platform.suspend_record.application_rpc', side_effect=preparation):
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('lost Power gesture')), 2)
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_playback_preparation_refusal_remains_a_named_inhibitor(self):
        with patch('y2_platform.suspend_record.application_rpc', side_effect=ValueError('playback_active')):
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('entered during playback')), 2)
        value = self.ctx.json('/data/system/platform/suspend-last.json')['product']
        self.assertEqual((value['state'], value['reason']), ('refused', 'playback_active'))
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_request_passes_lock_ownership_to_detached_helper(self):
        fcntl.flock(self.fd, fcntl.LOCK_UN)
        owned = []
        def launch(ctx, fd):
            owned.append(os.dup(fd))
        try:
            with patch('y2_platform.product_sleep.launch', side_effect=launch):
                self.assertEqual(request(self.ctx)['state'], 'requested')
            self.assertEqual(request(self.ctx)['reason'], 'sleep_operation_in_progress')
        finally:
            for fd in owned:
                os.close(fd)
        with patch('y2_platform.product_sleep.launch', side_effect=launch):
            self.assertEqual(request(self.ctx)['state'], 'requested')
        os.close(owned[-1])

    def test_power_press_during_maintenance_gap_cancels_next_entry(self):
        calls = []
        def enter(ctx):
            calls.append(1)
            self.returned(timer=True)
        def gap(seconds):
            self.power += 1
            self.irqs()
        with patch('y2_platform.product_sleep.time.sleep', side_effect=gap):
            self.assertEqual(self.run_sleep(enter), 2)
        self.assertEqual(len(calls), 1)
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_rtc_during_maintenance_gap_cancels_next_entry(self):
        calls = []
        def enter(ctx):
            calls.append(1)
            self.returned(timer=True)
        def gap(seconds):
            self.rtc += 1
            self.irqs()
            self.write('/sys/power/wakeup_count', '9')
        with patch('y2_platform.product_sleep.time.sleep', side_effect=gap):
            self.assertEqual(self.run_sleep(enter), 2)
        self.assertEqual(len(calls), 1)
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_active_owner_refuses_but_dead_owner_and_leftover_file_do_not(self):
        lease = self.ctx.path('/run/y2/activity.lock')
        child = subprocess.Popen([sys.executable, '-c',
            'import fcntl,sys,time; f=open(sys.argv[1],"w"); fcntl.flock(f,fcntl.LOCK_SH); '
            'print("held",flush=True); time.sleep(60)', str(lease)], stdout=subprocess.PIPE, text=True)
        self.addCleanup(child.stdout.close)
        try:
            self.assertEqual(child.stdout.readline().strip(), 'held')
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('entered with live owner')), 2)
            self.assertEqual(self.ctx.json('/data/system/platform/suspend-last.json')['product']['reason'], 'activity_operation')
        finally:
            child.terminate()
            child.wait(timeout=5)
        self.assertTrue(lease.exists())
        self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)

    def test_kernel_fault_quarantines_same_image_and_new_image_retries(self):
        def broken(ctx):
            raise OSError('failed after kernel entry')
        self.assertEqual(self.run_sleep(broken), 1)
        self.assertTrue(self.ctx.json(pm.GUARD)['pending'])
        self.assertEqual(self.ctx.read('/sys/firmware/y2_pm/backstop_s'), '0')
        # A regular fixture file records the write; real sysfs reads status.
        self.write('/sys/firmware/y2_pm/backstop_s', 'provider=1 armed_s=0 running=0')
        with self.assertRaisesRegex(pm.Refused, 'sleep_recovery_required'):
            pm.admission(self.ctx)
        self.write('/etc/y2linux/versions.json', '{"build_git_commit":"next-source"}')
        pm.admission(self.ctx)

    def prepare_refusal_ring(self, *extra):
        marks = ['HELPER_REQUEST', 'SUSPEND_REQUEST', 'BACKSTOP_STARTED', 'FILESYSTEM_SYNCED', *extra]
        lines = ['ring=valid cycle=50 last=1976 backstop_s=0']
        lines += [f'{1972 + i} phase=254 mark result=0 ms=297939 stage={stage}' for i, stage in enumerate(marks)]
        lines.append('1976 phase=254 mark result=-1 ms=298617 stage=EXIT')
        self.write('/sys/firmware/y2_pm/devices', '\n'.join(lines))
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=EXIT failed_stage=EXIT error=-1')

    def test_notifier_refusal_before_freeze_waits_instead_of_quarantining(self):
        # c43e8711 (8.10): PM_SUSPEND_PREPARE refused with -EPERM before any
        # task froze; the pending guard then refused sleep 237 times in 4.4 h.
        def refused(ctx):
            self.prepare_refusal_ring()
            raise OSError(errno.EPERM, 'Operation not permitted')
        self.assertEqual(self.run_sleep(refused), 2)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['product']['reason'], 'kernel_prepare_refused')
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        self.write('/sys/firmware/y2_pm/backstop_s', 'provider=1 armed_s=0 running=0')
        with self.assertRaisesRegex(pm.Refused, 'kernel_prepare_retry_wait'):
            pm.admission(self.ctx)
        retry = self.ctx.json(pm.RETRY)
        self.write(pm.RETRY, json.dumps(retry | {'not_before_ns': retry['not_before_ns'] - 121 * 10**9}))
        pm.admission(self.ctx)
        # The wait belongs to one boot.
        self.write(pm.RETRY, json.dumps(retry | {'boot_id': 'earlier-boot'}))
        pm.admission(self.ctx)

    def test_failure_after_freeze_or_unclear_ring_still_quarantines(self):
        for extra, ring in ((('TASKS_FROZEN',), True), ((), False)):
            def failed(ctx):
                if ring:
                    self.prepare_refusal_ring(*extra)
                else:
                    self.write('/sys/firmware/y2_pm/devices', 'ring=absent cycle=0 last=0 backstop_s=0')
                raise OSError(errno.EPERM, 'Operation not permitted')
            self.write(pm.GUARD, '{}')
            self.write('/sys/firmware/y2_pm/backstop_s', 'provider=1 armed_s=0 running=0')
            self.assertEqual(self.run_sleep(failed), 1)
            self.assertTrue(self.ctx.json(pm.GUARD)['pending'])
            self.assertIsNone(self.ctx.json(pm.RETRY))

    def test_radio_quiesce_failure_before_kernel_entry_never_quarantines(self):
        # POWER-FINAL-12 Defect A: a Bluetooth off refused by the reconnect
        # owner (result 1, kernel never entered) left the guard pending and
        # every later sleep was refused, across reboots, until a reflash.
        def refuse_bluetooth_off(argv, **kwargs):
            self.calls.append(argv)
            return {'ok': argv[1:] != ['bluetooth', 'off-runtime'], 'output': ''}
        self.ctx.runner = refuse_bluetooth_off
        with patch('y2_platform.product_sleep.time.sleep'), \
                patch.object(pm, 'RADIO_QUIESCE_SECONDS', 0):
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('entered without radios off')), 1)
        receipt = self.ctx.json('/data/system/platform/suspend-last.json')
        self.assertEqual(receipt['product']['reason'], 'sleep_service_failed:y2-radio:bluetooth:attempts=1')
        guard = self.ctx.json(pm.GUARD)
        self.assertFalse(guard['pending'])
        self.assertFalse(guard['kernel_entry'])
        self.assertEqual(self.rpc, ['sleep_prepare', 'sleep_restore'])
        pm.admission(self.ctx)
        # The next request succeeds normally.
        self.ctx.runner = self.command
        self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)

    def test_bluetooth_operation_in_progress_is_waited_out_within_the_window(self):
        refusals = []
        def reconnect_holds_lock(argv, **kwargs):
            self.calls.append(argv)
            if argv[1:] == ['bluetooth', 'off-runtime'] and len(refusals) < 3:
                refusals.append(argv)
                return {'ok': False, 'output': '{"ok": false}'}
            return {'ok': True, 'output': ''}
        self.ctx.runner = reconnect_holds_lock
        with patch('y2_platform.product_sleep.time.sleep') as pause:
            self.assertEqual(self.run_sleep(lambda ctx: self.returned()), 0)
        self.assertEqual(len(refusals), 3)
        self.assertEqual(pause.call_count, 3)
        self.assertEqual(self.ctx.json('/data/system/platform/suspend-last.json')['product']['state'], 'restored')
        self.assertFalse(self.ctx.json(pm.GUARD)['pending'])

    def test_stale_pre_kernel_guard_from_any_boot_does_not_refuse(self):
        identity = pm.identity(self.ctx)
        for boot in ('same-boot', 'earlier-boot'):
            self.write(pm.GUARD, json.dumps({'identity': identity, 'pending': True,
                                             'kernel_entry': False, 'boot_id': boot,
                                             'updated': time.time()}))
            pm.admission(self.ctx)
        # A pending kernel attempt (or a 12-format record without the field)
        # still quarantines the image.
        for record_value in ({'kernel_entry': True}, {}):
            self.write(pm.GUARD, json.dumps({'identity': identity, 'pending': True,
                                             'boot_id': 'earlier-boot', 'updated': time.time()}
                                            | record_value))
            with self.assertRaisesRegex(pm.Refused, 'sleep_recovery_required'):
                pm.admission(self.ctx)

    def test_kernel_warn_alone_keeps_product_sleep(self):
        # F26: one cfg80211 WARN (taint 512) kept a player awake on a weak charger for hours.
        self.write('/proc/sys/kernel/tainted', '512')
        pm.admission(self.ctx)
        for value in (str(512 | 4096), '1', 'unreadable'):
            with self.subTest(taint=value):
                self.write('/proc/sys/kernel/tainted', value)
                with self.assertRaisesRegex(pm.Refused, 'kernel_sleep_unavailable'):
                    pm.admission(self.ctx)

    def test_old_kernel_and_tainted_diagnostic_cannot_enable_product_sleep(self):
        self.write('/proc/sys/kernel/tainted', '4096')
        with self.assertRaisesRegex(pm.Refused, 'kernel_sleep_unavailable'):
            pm.admission(self.ctx)
        self.write('/sys/bus/platform/devices/10006000.power-controller/state', 'broken=0')
        with self.assertRaisesRegex(pm.Refused, 'physical_qualification_required'):
            pm.admission(self.ctx)

    def test_active_wakeup_count_read_is_bounded_without_periodic_polling(self):
        path = self.ctx.path('/sys/power/wakeup_count')
        path.unlink()
        os.mkfifo(path)
        holder = os.open(path, os.O_RDWR | os.O_NONBLOCK)
        try:
            with self.assertRaisesRegex(pm.Refused, 'wake_event_in_progress'):
                pm.wakeup_count(self.ctx, timeout=.02)
        finally:
            os.close(holder)

    def test_workload_expiry_and_mandatory_operation_exclusion(self):
        self.write('/sys/module/workload/parameters/leases', 'Maintenance remaining_ms=3000 min_khz=598000')
        with self.assertRaisesRegex(pm.Refused, 'workload_Maintenance'):
            pm.operation_state(self.ctx)
        self.write('/sys/module/workload/parameters/leases', 'Maintenance remaining_ms=0 min_khz=0')
        pm.operation_state(self.ctx)
        @workload('Maintenance')
        def work(ctx):
            return 'completed'
        with pm.operation_locks(self.ctx):
            with self.assertRaisesRegex(ValueError, 'sleep_operation_in_progress'):
                work(self.ctx)
        self.assertEqual(work(self.ctx), 'completed')

    def test_pending_update_low_voltage_and_usb_each_prevent_entry(self):
        for path, value, reason in (
                ('/sys/class/power_supply/BAT0/voltage_now', '3590000', 'battery_reserve'),
                ('/sys/class/power_supply/y2-usb-presence/online', '1', 'charger_or_usb_active'),
                ('/data/updates/state.json', '{"schema":1,"state":"Queued"}', 'firmware_update')):
            old = self.ctx.read(path)
            self.write(path, value)
            self.assertEqual(self.run_sleep(lambda ctx: self.fail('unsafe entry')), 2)
            self.assertEqual(self.ctx.json('/data/system/platform/suspend-last.json')['product']['reason'], reason)
            if old is None:
                self.ctx.path(path).unlink()
            else:
                self.write(path, old)


    def test_idle_cable_without_host_admits_sleep(self):
        """Defect C (archive 109): VBUS from a powered-off PC, charger idle."""
        self.write('/sys/class/power_supply/y2-usb-presence/online', '1')
        self.write('/sys/class/udc/musb-hdrc.4.auto/state', 'default')
        for status in ('Not charging', 'Full'):
            self.write('/sys/class/power_supply/BAT0/status', status)
            pm.battery_and_cable(self.ctx)
        # A configured host (SSH/MTP) or an active charge keeps the Y2 awake.
        self.write('/sys/class/udc/musb-hdrc.4.auto/state', 'configured')
        with self.assertRaisesRegex(pm.Refused, 'charger_or_usb_active'):
            pm.battery_and_cable(self.ctx)
        self.write('/sys/class/udc/musb-hdrc.4.auto/state', 'not attached')
        self.write('/sys/class/power_supply/BAT0/status', 'Charging')
        with self.assertRaisesRegex(pm.Refused, 'charger_or_usb_active'):
            pm.battery_and_cable(self.ctx)
        # Without a cable the battery must really be discharging.
        self.write('/sys/class/power_supply/y2-usb-presence/online', '0')
        self.write('/sys/class/power_supply/BAT0/status', 'Not charging')
        with self.assertRaisesRegex(pm.Refused, 'battery_state_unavailable'):
            pm.battery_and_cable(self.ctx)

if __name__ == '__main__':
    unittest.main()
