"""Hardware Final integration boundaries; source fixtures, never device access."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools/platform'))
from y2_platform.common import Context
from y2_platform.observe import timer_runtime, storage, system
from y2_platform.timekeeping import alarm, ntp_event
from y2_platform.capabilities import status
from tools.build.dev_initramfs import boot_module_paths
from tools.production.system_update import CONFIGURATION_FILES, configuration_inventory


class HardwareFinalContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ctx = Context(self.root)

    def put(self, name, value):
        path = self.ctx.path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
        return path

    def test_actual_timer_modes_are_observed_and_unknown_is_not_false_success(self):
        self.assertIsNone(timer_runtime(self.ctx)['highres_active'])
        self.put('/proc/timer_list', 'cpu: 0\n .hres_active : 1\n .nohz : 1\n .tick_stopped : 0\n'
                 'cpu: 1\n .hres_active : 0\n .nohz : 1\n .tick_stopped : 1\n')
        result = timer_runtime(self.ctx)
        self.assertFalse(result['highres_active'])
        self.assertTrue(result['no_hz_active'])
        self.assertEqual(result['per_cpu'][1]['tick_stopped'], 1)
        self.put('/proc/timer_list', 'cpu: 0\n .hres_active : 1\n')
        self.assertIsNone(timer_runtime(self.ctx)['no_hz_active'])
        self.put('/sys/bus/platform/devices/11230000.mmc/y2_performance',
                 'cap_hz=25000000 actual_hz=25000000 transport_errors=2 fallbacks=1 clock_error=0\n')
        self.assertEqual(storage(self.ctx)['controllers'][0]['fallbacks'], 1)
        self.put('/sys/bus/platform/drivers/y2-usb/11200000.usb/status',
                 'transfer=inventra_dma dma_irqs=12 dma_rx_programmed_bytes=1024 dma_errors=1\n')
        dma = system(self.ctx)['usb']['dma']
        self.assertEqual(dma['transfer'], 'inventra_dma')
        self.assertEqual(dma['dma_rx_programmed_bytes'], 1024)
        self.assertEqual(dma['dma_errors'], 1)

    def test_experimental_enablement_does_not_promote_qualification(self):
        self.put('/etc/y2linux/capabilities.json', (ROOT/'tools/platform/capabilities.json').read_text())
        self.put('/etc/y2linux/bluetooth-codecs.json', json.dumps({'schema': 1, 'codecs': {
            'AAC': {'compiled_locally': True, 'owner_private_experiment': True, 'platform_qualified': False}}}))
        self.assertFalse(status(self.ctx)['capabilities']['bluetooth_aac']['enabled'])
        self.put('/data/bluetooth/codec-policy.json', '{"schema":1,"experimental":true}')
        cap = status(self.ctx)['capabilities']['bluetooth_aac']
        self.assertTrue(cap['implemented'])
        self.assertTrue(cap['enabled'])
        self.assertFalse(cap['qualified'])
        self.assertFalse(status(self.ctx)['capabilities']['usb_host']['enabled'])

    def test_alarm_replacement_cancel_and_failed_readback(self):
        self.put('/sys/class/rtc/rtc0/since_epoch', '1790000000')
        self.put('/sys/class/rtc/rtc0/wakealarm', '1')
        self.assertEqual(alarm(self.ctx, 60)['alarm_epoch'], 1790000060)
        self.assertEqual(alarm(self.ctx, 0)['state'], 'Disabled')
        with self.assertRaises(ValueError): alarm(self.ctx, 604801)
        original = self.ctx.integer
        self.ctx.integer = lambda name: 1 if name.endswith('/wakealarm') else original(name)
        with self.assertRaisesRegex(ValueError, 'readback_mismatch'): alarm(self.ctx, 60)
        self.assertEqual(self.ctx.read('/sys/class/rtc/rtc0/wakealarm'), '0')

    def test_ntp_source_backed_default_and_owner_disable(self):
        self.put('/etc/y2linux/build-epoch', '1790000000')
        self.put('/etc/y2linux/rtc-policy.json', '{"schema":1,"write_enabled":true,"source":"stock"}')
        calls = []
        self.ctx.runner = lambda argv, **kw: calls.append(argv) or {'ok': True, 'output': '', 'reason': None}
        self.assertTrue(ntp_event(self.ctx, 'step', {'stratum': '3'}, now=1790000010)['rtc_write'])
        self.put('/data/system/platform/rtc-policy.json', '{"schema":1,"write_enabled":false}')
        calls.clear()
        self.assertNotIn('rtc_write', ntp_event(self.ctx, 'step', {'stratum': '3'}, now=1790000011))
        self.assertFalse(calls)

    def test_module_allowlist_cannot_smuggle_extra_modules(self):
        paths = ['drivers/gpu/drm/mediatek/mediatek-drm', 'drivers/usb/gadget/function/usb_f_ncm',
                 'drivers/usb/gadget/legacy/g_ncm']
        self.put('/kernel/.config', 'CONFIG_USB_G_NCM=m\n')
        self.put('/kernel/modules.order', '\n'.join(path+'.o' for path in paths)+'\n')
        self.assertEqual(boot_module_paths(self.root), paths)
        self.put('/kernel/modules.order', '\n'.join(path+'.o' for path in paths)+ '\ndrivers/unknown.o\n')
        with self.assertRaises(ValueError): boot_module_paths(self.root)

    def test_package_configuration_hashes_and_independent_states(self):
        for name in CONFIGURATION_FILES:
            source = ROOT/'buildroot/board/y2/production-overlay/etc/y2linux'/name
            value = (ROOT/'tools/platform/capabilities.json').read_text() if name == 'capabilities.json' else (
                source.read_text() if source.exists() else '{"schema":1}')
            self.put('/etc/y2linux/'+name, value)
        before = configuration_inventory(self.root)
        self.put('/etc/y2linux/rtc-policy.json', '{"schema":1,"write_enabled":false}')
        after = configuration_inventory(self.root)
        self.assertNotEqual(before['files']['rtc-policy.json']['sha256'], after['files']['rtc-policy.json']['sha256'])
        caps = self.ctx.json('/etc/y2linux/capabilities.json')
        caps['capabilities']['wired_48']['qualified'] = True
        self.put('/etc/y2linux/capabilities.json', json.dumps(caps))
        with self.assertRaises(ValueError): configuration_inventory(self.root)
