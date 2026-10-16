"""Product sleep state is derived from same-boot kernel and application proof."""
import fcntl
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.sleep import product, request, status, transition
from y2_platform.suspend_record import record


class Reply:
    def __init__(self, ok=True):
        self.ok = ok

    def __enter__(self):
        return self

    def __exit__(self, *unused):
        pass

    def settimeout(self, unused):
        pass

    def connect(self, unused):
        pass

    def sendall(self, unused):
        pass

    def recv(self, unused):
        return json.dumps({'version': 1, 'id': 6582, 'ok': self.ok, 'result': {'sleep_ready': True, 'screen_off': False}}).encode() + b'\n'


class SleepProduct(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ctx = Context(self.tmp.name)
        self.write('/proc/sys/kernel/random/boot_id', 'boot-one')
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=EXIT failed_stage=NONE error=0 wake=0x20 reset_resume_entry=0x59325253 pcm_length=596')
        self.write('/sys/firmware/y2_pm/stage', '')
        self.write('/sys/power/state', '')
        self.write('/sys/power/pm_test', '[none] core processors platform devices freezer')

    def write(self, name, text):
        path = self.ctx.path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def test_product_request_is_typed_durable_refusal_without_kernel_or_radio_work(self):
        result = request(self.ctx)
        self.assertEqual(result['schema'], 'org.y2linux.sleep/v1')
        self.assertEqual(result['state'], 'refused')
        self.assertEqual(result['reason'], 'physical_qualification_required')
        self.assertEqual(status(self.ctx), result)
        self.assertEqual(self.ctx.read('/sys/power/state'), '')
        self.assertEqual(self.ctx.read('/sys/firmware/y2_pm/stage'), '')

    def test_charging_refusal_is_specific_and_never_bypasses_the_kernel_guard(self):
        self.write('/sys/class/power_supply/BAT0/status', 'Charging')
        value = request(self.ctx)
        self.assertEqual(value['state'], 'refused')
        self.assertEqual(value['reason'], 'charger_active')
        self.assertEqual(self.ctx.read('/sys/power/state'), '')

    def test_parallel_product_request_does_not_replace_owner_receipt(self):
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        lock = self.ctx.path('/run/y2/suspend.lock')
        lock.parent.mkdir(parents=True)
        with lock.open('w') as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(request(self.ctx)['reason'], 'sleep_operation_in_progress')
        self.assertEqual(status(self.ctx)['state'], 'sleeping')

    def test_completed_kernel_and_reborn_on_same_boot_restore(self):
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        restoring = record(self.ctx, 'restoring_radios', 0)
        self.assertTrue(restoring['product']['kernel_completed'])
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=RADIOS_RESTORING error=0')
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()):
            result = record(self.ctx, 'complete_result_0', 0)
        self.assertEqual(result['product']['state'], 'restored')
        self.assertTrue(result['product']['same_boot'])
        self.assertEqual(self.ctx.read('/sys/firmware/y2_pm/stage'), 'REBORN_READY')
        # SPM EINT bit 5 alone does not distinguish the Power and RTC PMIC IRQs.
        self.assertEqual(result['product']['wake_reason'], 'unknown')

    def test_wake_requires_serviced_same_boot_child_irq_delta(self):
        for source in ('rtc', 'power', 'multiple'):
            record(self.ctx, 'quiescing_radios', 0)
            self.write('/proc/interrupts', '  66: 1 0 0 0 mt6323 20 Level mt6397-rtc\n'
                       '  70: 2 0 0 0 mt6323 5 Level mtk-pmic-keys\n')
            record(self.ctx, 'kernel_suspend', 0)
            rtc = 2 if source in ('rtc', 'multiple') else 1
            power = 3 if source in ('power', 'multiple') else 2
            self.write('/proc/interrupts', f'  66: {rtc} 0 0 0 mt6323 20 Level mt6397-rtc\n'
                       f'  70: {power} 0 0 0 mt6323 5 Level mtk-pmic-keys\n')
            value = record(self.ctx, 'restoring_radios', 0)
            self.assertEqual(value['product']['wake_reason'], source)

    def test_terminal_previous_boot_success_or_refusal_does_not_report_current_failure(self):
        for state in ('restored', 'refused'):
            value = {'product': {'state': state, 'request_boot_id': 'boot-old',
                                 'kernel_completed': state == 'restored', 'wake_reason': 'rtc'}}
            current = product(value, 'boot-new')
            self.assertEqual(current['state'], 'idle')
            self.assertIsNone(current['same_boot'])
            self.assertFalse(current['kernel_completed'])
        self.assertEqual(product({'product': {'state': 'restore_failed', 'request_boot_id': 'boot-old'}},
                                 'boot-new')['state'], 'restore_failed')

    def test_reset_is_not_same_boot_resume(self):
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        self.write('/proc/sys/kernel/random/boot_id', 'boot-two')
        self.assertEqual(status(self.ctx)['state'], 'restore_failed')
        self.assertEqual(status(self.ctx)['reason'], 'boot_changed_before_restoration')
        record(self.ctx, 'restoring_radios', 0)
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()):
            result = record(self.ctx, 'complete_result_0', 0)
        self.assertEqual(result['product']['state'], 'restore_failed')
        self.assertFalse(result['product']['same_boot'])
        self.assertEqual(result['result'], 1)
        self.assertEqual(self.ctx.read('/sys/firmware/y2_pm/stage'), '')

    def test_missing_kernel_exit_and_reborn_failure_are_not_success(self):
        for kernel, ready, reason in [
                ('valid=1 stage=DEVICES_RESUMED error=0', True, 'kernel_exit_not_proven'),
                ('valid=0 stage=EXIT error=0', True, 'kernel_exit_not_proven'),
                ('valid=1 stage=EXIT error=0', False, 'reborn_resume_probe_failed')]:
            record(self.ctx, 'quiescing_radios', 0)
            record(self.ctx, 'kernel_suspend', 0)
            self.write('/sys/firmware/y2_pm/state', kernel)
            record(self.ctx, 'restoring_radios', 0)
            with patch('y2_platform.suspend_record.socket.socket', return_value=Reply(ready)):
                result = record(self.ctx, 'complete_result_0', 0)
            self.assertEqual(result['product']['reason'], reason)
            self.assertEqual(result['product']['state'], 'restore_failed')
            self.assertEqual(result['result'], 1)

    def test_charger_device_prepare_refusal_survives_radio_restore(self):
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=EXIT failed_stage=DPM_PREPARED error=-16')
        record(self.ctx, 'restoring_radios', 1)
        result = record(self.ctx, 'complete_result_1', 1)
        self.assertEqual(result['product']['state'], 'refused')
        self.assertEqual(result['product']['reason'], 'device_prepare_refused')

    def test_older_or_malformed_receipt_is_idle_without_resume_claim(self):
        self.assertEqual(product({}, 'boot-one')['state'], 'idle')
        self.assertIsNone(product({}, None)['same_boot'])
        self.write('/data/system/platform/suspend-last.json', '[]')
        self.assertEqual(status(self.ctx)['state'], 'idle')

    def test_pm_test_return_never_proves_deep_sleep(self):
        for mode in ('freezer', 'devices', 'platform', 'processors', 'core'):
            self.write('/sys/power/pm_test', '[' + mode + '] none')
            record(self.ctx, 'quiescing_radios', 0)
            record(self.ctx, 'kernel_suspend', 0)
            record(self.ctx, 'restoring_radios', 0)
            with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()):
                value = record(self.ctx, 'complete_result_0', 0)
            self.assertEqual(value['result'], 0)
            self.assertEqual(value['product']['state'], 'idle')
            self.assertFalse(value['product']['kernel_completed'])
            self.assertNotEqual(self.ctx.read('/sys/firmware/y2_pm/stage'), 'REBORN_READY')

    def test_exit_without_reset_pcm_or_wake_is_not_deep_resume(self):
        for kernel in ('valid=1 stage=EXIT error=0',
                       'valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=479 wake=0x20',
                       'valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=596 wake=0x0'):
            record(self.ctx, 'quiescing_radios', 0)
            record(self.ctx, 'kernel_suspend', 0)
            self.write('/sys/firmware/y2_pm/state', kernel)
            value = record(self.ctx, 'restoring_radios', 0)
            self.assertFalse(value['product']['kernel_completed'])

    def test_failed_suspend_still_restores_the_application(self):
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        record(self.ctx, 'restoring_radios', 1)
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()) as rpc:
            value = record(self.ctx, 'complete_result_1', 1)
        rpc.assert_called_once()
        self.assertTrue(value['reborn_restore'])
        self.assertEqual(value['result'], 1)
        self.assertEqual(value['product']['state'], 'restore_failed')

    def test_preparation_has_a_distinct_state_and_requires_acknowledgement(self):
        from y2_platform.sleep import prepare
        record(self.ctx, 'requested', 0)
        with patch('y2_platform.sleep.prepare'):
            self.assertEqual(status(self.ctx)['state'], 'requested')
        with patch('y2_platform.suspend_record.application_rpc', return_value={'sleep_ready': True, 'screen_off': True}):
            self.assertEqual(prepare(self.ctx)['state'], 'preparing')
        value = record(self.ctx, 'quiescing_radios', 0)
        self.assertTrue(value['reborn_prepared'])
        with patch('y2_platform.suspend_record.application_rpc', return_value={'sleep_ready': False}):
            with self.assertRaises(ValueError):
                prepare(self.ctx)

    def test_screen_is_restored_before_the_radios(self):
        # POWER-FINAL-17: archive 110 lit the screen ≈2.6 s after Power, 1.9 s
        # of it waiting for Wi-Fi/Bluetooth firmware and calibration.
        source = (ROOT / 'tools/platform/y2_platform/product_sleep.py').read_text()
        restore = source[source.index('# Keep every operation lock held until radios and Reborn restore.'):]
        self.assertLess(restore.index('screen = screen_first(ctx)'), restore.index("record(ctx, 'restoring_radios'"))
        self.assertLess(restore.index("record(ctx, 'restoring_radios'"), restore.index("'on-runtime'"))
        self.assertIn('restore_ack=screen', restore)

    def test_early_screen_restore_is_reused_and_a_failed_one_retried(self):
        from y2_platform.product_sleep import screen_first
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()):
            ack = screen_first(self.ctx)
        self.assertEqual(ack, {'sleep_ready': True, 'screen_off': False})
        with patch('y2_platform.suspend_record.socket.socket', side_effect=OSError('down')):
            self.assertIsNone(screen_first(self.ctx))
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        record(self.ctx, 'restoring_radios', 0)
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()) as rpc:
            value = record(self.ctx, 'complete_result_0', 0, restore_ack=ack)
        rpc.assert_not_called()
        self.assertTrue(value['reborn_restore'])
        record(self.ctx, 'quiescing_radios', 0)
        record(self.ctx, 'kernel_suspend', 0)
        with patch('y2_platform.suspend_record.socket.socket', return_value=Reply()) as rpc:
            value = record(self.ctx, 'complete_result_0', 0, restore_ack=None)
        rpc.assert_called_once()
        self.assertTrue(value['reborn_restore'])

    def test_state_directory_symlink_is_refused(self):
        self.ctx.path('/data').mkdir()
        self.ctx.path('/data/system').symlink_to(self.tmp.name)
        with self.assertRaisesRegex(ValueError, 'state_directory_not_directory'):
            record(self.ctx, 'quiescing_radios', 0)



class FMQuiesce(unittest.TestCase):
    """A tuner left on must not refuse the connectivity suspend."""
    def test_tuner_is_switched_off_only_when_on_and_failure_refuses(self):
        from y2_platform import product_sleep
        root = Path(tempfile.mkdtemp(prefix='y2-fm-sleep-'))
        ctx = Context(root, lambda argv, **kw: {'ok': True})
        power = ctx.path(product_sleep.FM_POWER)
        self.assertFalse(product_sleep.quiesce_fm(ctx))       # no tuner node: nothing to do
        power.parent.mkdir(parents=True)
        power.write_text('0\n')
        self.assertFalse(product_sleep.quiesce_fm(ctx))
        self.assertEqual(power.read_text(), '0\n')
        power.write_text('1\n')
        self.assertTrue(product_sleep.quiesce_fm(ctx))
        self.assertEqual(power.read_text(), '0\n')
        power.write_text('1\n')
        with patch.object(product_sleep, 'control_write', side_effect=OSError(16, 'busy')):
            with self.assertRaisesRegex(product_sleep.Refused, 'fm_power_down'):
                product_sleep.quiesce_fm(ctx)


if __name__ == '__main__':
    unittest.main()
