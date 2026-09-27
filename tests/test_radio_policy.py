from pathlib import Path
import json
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.radio_policy import codec_runtime, wifi_policy, Coexistence


class RadioPolicy(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.ctx.path('/run/y2').mkdir(parents=True)
        self.calls = []
        self.enabled = False
        def runner(argv, **kw):
            self.calls.append(argv)
            if 'set' in argv: self.enabled = argv[-1] == 'on'
            return {'ok': True, 'reason': None, 'output': 'Power save: ' + ('on' if self.enabled else 'off')}
        self.ctx.runner = runner

    def put(self, path, value):
        dest = self.ctx.path(path)
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(json.dumps(value))

    def test_experimental_gate_is_explicit_and_sbc_fallback_is_retained(self):
        caps = {name: {'compiled_locally': True, 'distribution_approved': name == 'SBC',
                       'platform_qualified': False, 'owner_private_experiment': name != 'SBC'}
                for name in ('SBC', 'AAC', 'aptX', 'aptX-HD', 'LDAC')}
        self.put('/etc/y2linux/bluetooth-codecs.json', {'schema': 1, 'codecs': caps})
        args = codec_runtime(self.ctx)
        self.assertIn('--codec=-LDAC', args)
        self.assertIn('--codec=-AAC', args)
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'experimental': True, 'sbc_quality': 'xq'})
        args = codec_runtime(self.ctx)
        self.assertIn('--codec=LDAC', args)
        self.assertIn('--codec=AAC', args)
        self.assertIn('--sbc-quality=xq', args)
        self.assertNotIn('--codec=-SBC', args)
        caps['AAC']['compiled_locally'] = False
        self.put('/etc/y2linux/bluetooth-codecs.json', {'schema': 1, 'codecs': caps})
        self.assertNotIn('--codec=AAC', codec_runtime(self.ctx))
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'experimental': False, 'sbc_quality': 'xq'})
        with self.assertRaises(ValueError): codec_runtime(self.ctx)

    def test_standard_ps_has_readback_and_failure_is_not_success(self):
        self.put('/data/network/power-save.json', {'schema': 1, 'mode': 'standard'})
        value = wifi_policy(self.ctx)
        self.assertTrue(value['enabled'])
        self.assertEqual(value['state'], 'Ready')
        self.ctx.runner = lambda *a, **k: {'ok': False, 'reason': 'timeout', 'output': None}
        self.assertIsNone(wifi_policy(self.ctx)['enabled'])

    def test_automatic_ps_conservative_default_and_heavy_transfer_hook(self):
        self.assertFalse(wifi_policy(self.ctx)['enabled'])
        self.put('/data/network/power-save.json', {'schema': 1, 'mode': 'automatic', 'automatic_standard': True})
        self.assertTrue(wifi_policy(self.ctx)['enabled'])
        self.assertFalse(wifi_policy(self.ctx, {'wifi_heavy_transfer': True})['enabled'])
        observe = Coexistence()
        self.assertIsNone(observe.observe(self.ctx, {'state': 'Online'}, 1)['wifi_bytes_per_second'])
        value = observe.observe(self.ctx, {'state': 'Online', 'traffic_counters': {'rx_bytes': 3000000}}, 2)
        self.assertTrue(value['wifi_heavy_transfer'])
        self.assertIsNone(value['codec_bitrate_override'])
