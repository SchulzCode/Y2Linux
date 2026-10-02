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
        return json.dumps({'version': 1, 'id': 6582, 'ok': self.ok}).encode() + b'\n'


class SleepProduct(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ctx = Context(self.tmp.name)
        self.write('/proc/sys/kernel/random/boot_id', 'boot-one')
        self.write('/sys/firmware/y2_pm/state', 'valid=1 stage=EXIT failed_stage=NONE error=0 wake=0x20')
        self.write('/sys/firmware/y2_pm/stage', '')
        self.write('/sys/power/state', '')

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

    def test_state_directory_symlink_is_refused(self):
        self.ctx.path('/data').mkdir()
        self.ctx.path('/data/system').symlink_to(self.tmp.name)
        with self.assertRaisesRegex(ValueError, 'state_directory_not_directory'):
            record(self.ctx, 'quiescing_radios', 0)


if __name__ == '__main__':
    unittest.main()
