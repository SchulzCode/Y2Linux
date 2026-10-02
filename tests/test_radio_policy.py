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

class CodecQuality(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.ctx.path('/run/y2').mkdir(parents=True)
        self.ctx.path('/data/bluetooth').mkdir(parents=True)
        self.ctx.path('/etc/y2linux').mkdir(parents=True)
        self.ctx.path('/proc/sys/kernel/random').mkdir(parents=True)
        self.ctx.path('/proc/sys/kernel/random/boot_id').write_text('current-boot')
        self.ctx.path('/etc/y2linux/bluetooth-codecs.json').write_text(json.dumps({
            'schema': 1, 'private_integration_enabled': True,
            'codecs': {name: {'compiled_locally': True, 'owner_private_experiment': True}
                       for name in ('AAC', 'aptX', 'aptX-HD', 'LDAC')}}))

    def test_private_integration_enables_all_endpoints_without_qualification_claim(self):
        from y2_platform.codec_controls import configure
        args = codec_runtime(self.ctx, record=True)
        self.ctx.path('/run/y2/bluealsa.pid').write_text('123')
        self.ctx.path('/proc/123').mkdir()
        self.ctx.path('/proc/123/cmdline').write_text('\0'.join(['/usr/bin/bluealsad', *args]) + '\0')
        for name in ('AAC', 'aptX', 'aptX-HD', 'LDAC'):
            self.assertIn('--codec=' + name, args)
        self.assertFalse(configure(self.ctx)['pending_restart'])
        changed = configure(self.ctx, ldac_quality='high', ldac_abr=False)
        self.assertTrue(changed['pending_restart'])
        self.assertEqual(changed['effective']['ldac_quality'], 'standard')
        self.assertTrue(changed['effective']['ldac_abr'])
        self.assertFalse(changed['requested']['ldac_abr'])
        args = codec_runtime(self.ctx, record=True)
        self.assertNotIn('--ldac-abr', args)
        self.ctx.path('/proc/123/cmdline').write_text('\0'.join(['/usr/bin/bluealsad', *args]) + '\0')
        self.assertFalse(configure(self.ctx)['pending_restart'])
        self.ctx.path('/proc/sys/kernel/random/boot_id').write_text('next-boot')
        self.assertIsNone(configure(self.ctx)['effective'])

    def test_invalid_preferences_never_modify_saved_settings(self):
        from y2_platform.codec_controls import configure
        for value in ('unknown', '', ';reboot'):
            with self.assertRaises(ValueError): configure(self.ctx, ldac_quality=value)
        with self.assertRaises(ValueError): configure(self.ctx, ldac_abr='yes')
        self.assertFalse(self.ctx.path('/data/bluetooth/codec-policy.json').exists())

    def test_sbc_xq_requires_negotiated_mode_and_actual_bitpool(self):
        from y2_platform.codec_observation import quality
        pcm = {'Running': True, 'Rate': 44100, 'CodecConfiguration': [0x24, 0x15, 2, 53],
               'EncoderStats': {'Active': 1, 'Bitpool': 38, 'BitrateKbps': 452}}
        self.assertEqual(quality('SBC', pcm, {})['sbc_quality'], 'xq')
        pcm['EncoderStats']['Bitpool'] = 35
        self.assertEqual(quality('SBC', pcm, {})['sbc_quality'], 'standard')
        pcm['EncoderStats']['Bitpool'] = 47
        self.assertEqual(quality('SBC', pcm, {})['sbc_quality'], 'xq+')
        pcm['CodecConfiguration'][0] = 0x21
        self.assertEqual(quality('SBC', pcm, {})['sbc_quality'], 'standard')
        pcm['Running'] = False
        self.assertIsNone(quality('SBC', pcm, {})['sbc_quality'])

    def test_ldac_abr_is_observed_only_from_active_encoder_transitions(self):
        from y2_platform.codec_observation import quality
        pcm = {'Running': True, 'Rate': 44100,
               'EncoderStats': {'Active': 1, 'BitrateKbps': 606, 'QualityIndex': 1,
                                'AbrEnabled': 1, 'AbrAdjustments': 0}}
        first = quality('LDAC', pcm, {})
        self.assertEqual(first['ldac_nominal_choices_kbps'], [303, 606, 909])
        self.assertFalse(first['ldac_abr_adaptation_observed'])
        pcm['EncoderStats']['AbrAdjustments'] = 2
        pcm['Rate'] = 48000
        next_value = quality('LDAC', pcm, {})
        self.assertEqual(next_value['ldac_nominal_choices_kbps'], [330, 660, 990])
        self.assertTrue(next_value['ldac_abr_adaptation_observed'])
        pcm['EncoderStats']['Active'] = 0
        self.assertIsNone(quality('LDAC', pcm, {})['bitrate_bps'])
        self.assertFalse(quality('LDAC', pcm, {})['ldac_abr_adaptation_observed'])
