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
        # SBC XQ is an owner choice now, not an experiment.
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'experimental': False, 'sbc_quality': 'xq'})
        self.assertIn('--sbc-quality=xq', codec_runtime(self.ctx))
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'sbc_quality': 'max'})
        with self.assertRaises(ValueError): codec_runtime(self.ctx)

    def test_aac_quality_and_adaptive_rate_arguments(self):
        caps = {name: {'compiled_locally': True, 'distribution_approved': True, 'platform_qualified': True}
                for name in ('SBC', 'AAC', 'LDAC')}
        self.put('/etc/y2linux/bluetooth-codecs.json', {'schema': 1, 'codecs': caps})
        args = codec_runtime(self.ctx)
        self.assertIn('--codec=AAC', args)
        for arg in ('--aac-afterburner', '--aac-bitrate=320000', '--aac-abr'):
            self.assertIn(arg, args)  # the defaults
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'aac_quality': 'standard', 'aac_abr': False})
        args = codec_runtime(self.ctx)
        self.assertFalse([arg for arg in args if arg.startswith('--aac-')])
        # The arguments are exactly the ones the start script accepts.
        script = (Path(__file__).resolve().parents[1] /
                  'buildroot/board/y2/production-overlay/usr/libexec/y2/connectivity').read_text()
        for arg in ('--aac-afterburner', '--aac-bitrate=320000', '--aac-abr'):
            self.assertIn('|' + arg, script)
        caps['AAC']['distribution_approved'] = False
        self.put('/etc/y2linux/bluetooth-codecs.json', {'schema': 1, 'codecs': caps})
        self.put('/data/bluetooth/codec-policy.json', {'schema': 1, 'experimental': False})
        args = codec_runtime(self.ctx)
        self.assertIn('--codec=-AAC', args)
        self.assertFalse([arg for arg in args if arg.startswith('--aac-')])

    def test_standard_ps_has_readback_and_failure_is_not_success(self):
        self.put('/data/network/power-save.json', {'schema': 1, 'mode': 'standard'})
        value = wifi_policy(self.ctx)
        self.assertTrue(value['enabled'])
        self.assertEqual(value['state'], 'Ready')
        self.ctx.runner = lambda *a, **k: {'ok': False, 'reason': 'timeout', 'output': None}
        self.assertIsNone(wifi_policy(self.ctx)['enabled'])

    def test_set_only_on_change_and_reestablish_after_epoch_or_interface_change(self):
        self.put('/data/network/power-save.json', {'schema': 1, 'mode': 'standard'})
        self.assertTrue(wifi_policy(self.ctx)['applied'])
        self.calls.clear()
        value = wifi_policy(self.ctx)
        self.assertFalse(value['applied'])
        self.assertEqual(value['requested_driver_profile'], 'FAST_PSP')
        self.assertFalse(any('set' in c for c in self.calls))
        # An external change, new association or interface recreation requires
        # a fresh firmware command even when the cfg80211 flag already agrees.
        self.enabled = False
        self.assertTrue(wifi_policy(self.ctx)['applied'])
        self.ctx.path('/run/y2/wifi-epoch').write_text('new-lease')
        self.assertTrue(wifi_policy(self.ctx)['applied'])
        self.assertFalse(wifi_policy(self.ctx)['applied'])
        dest = self.ctx.path('/sys/class/net/wlan0/ifindex')
        dest.parent.mkdir(parents=True)
        dest.write_text('5')
        self.assertTrue(wifi_policy(self.ctx)['applied'])
        self.assertFalse(wifi_policy(self.ctx)['applied'])

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

    def test_sbc_adaptive_bitpool_is_on_by_default_and_switchable(self):
        from y2_platform.codec_controls import configure
        args = codec_runtime(self.ctx)
        self.assertIn('--sbc-abr', args)  # also for settings saved before 8.17
        self.assertLess(args.index('--sbc-quality=high'), args.index('--sbc-abr'))
        script = (Path(__file__).resolve().parents[1] /
                  'buildroot/board/y2/production-overlay/usr/libexec/y2/connectivity').read_text()
        self.assertIn('|--sbc-abr|', script)
        value = configure(self.ctx, sbc_abr=False)
        self.assertEqual((value['requested']['sbc_abr'], value['supported']['sbc_abr']), (False, True))
        self.assertNotIn('--sbc-abr', codec_runtime(self.ctx))
        with self.assertRaises(ValueError): configure(self.ctx, sbc_abr='on')

    def test_sbc_live_bitpool_and_adjustments_are_observed(self):
        from y2_platform.codec_observation import quality
        pcm = {'Running': True, 'Rate': 44100, 'CodecConfiguration': [0x21, 0x15, 2, 53],
               'EncoderStats': {'Active': 1, 'Bitpool': 53, 'BitrateKbps': 328,
                                'AbrEnabled': 1, 'AbrAdjustments': 0}}
        value = quality('SBC', pcm, {})
        self.assertEqual((value['sbc_bitpool'], value['sbc_abr_enabled'], value['sbc_abr_adjustments']), (53, True, 0))
        pcm['EncoderStats'].update(Bitpool=35, BitrateKbps=229, AbrAdjustments=2)
        value = quality('SBC', pcm, {})
        self.assertEqual((value['sbc_bitpool'], value['bitrate_bps'], value['sbc_abr_adjustments']), (35, 229000, 2))
        pcm['Running'] = False
        self.assertIsNone(quality('SBC', pcm, {})['sbc_abr_enabled'])

    def test_invalid_preferences_never_modify_saved_settings(self):
        from y2_platform.codec_controls import configure
        for value in ('unknown', '', ';reboot'):
            with self.assertRaises(ValueError): configure(self.ctx, ldac_quality=value)
            with self.assertRaises(ValueError): configure(self.ctx, aac_quality=value)
        with self.assertRaises(ValueError): configure(self.ctx, aac_abr='yes')
        with self.assertRaises(ValueError): configure(self.ctx, ldac_abr='yes')
        self.assertFalse(self.ctx.path('/data/bluetooth/codec-policy.json').exists())

    def test_aac_settings_are_saved_and_reported(self):
        from y2_platform.codec_controls import configure
        value = configure(self.ctx)
        self.assertEqual((value['requested']['aac_quality'], value['requested']['aac_abr']), ('high', True))
        self.assertEqual(value['supported']['aac_quality'], ['standard', 'high'])
        self.assertEqual(value['supported']['sbc_quality'], ['high', 'xq', 'xq+'])
        value = configure(self.ctx, aac_quality='standard', aac_abr=False)
        self.assertEqual((value['requested']['aac_quality'], value['requested']['aac_abr']), ('standard', False))
        self.assertEqual(json.loads(self.ctx.path('/data/bluetooth/codec-policy.json').read_text())['aac_quality'], 'standard')
        inventory = json.loads(self.ctx.path('/etc/y2linux/bluetooth-codecs.json').read_text())
        inventory['codecs']['AAC']['compiled_locally'] = False
        self.ctx.path('/etc/y2linux/bluetooth-codecs.json').write_text(json.dumps(inventory))
        with self.assertRaisesRegex(ValueError, 'aac_encoder_not_compiled'):
            configure(self.ctx, aac_quality='high')

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

    def test_aac_rate_and_adaptation_are_observed_from_the_live_encoder(self):
        from y2_platform.codec_observation import quality
        pcm = {'Running': True, 'Rate': 44100,
               'EncoderStats': {'Active': 1, 'BitrateKbps': 256, 'QualityIndex': 0,
                                'AbrEnabled': 1, 'AbrAdjustments': 0, 'Errors': 0}}
        value = quality('AAC', pcm, {})
        self.assertEqual((value['bitrate_bps'], value['aac_abr_enabled'], value['aac_abr_level']), (256000, True, 0))
        pcm['EncoderStats'].update(BitrateKbps=160, QualityIndex=2, AbrAdjustments=2)
        value = quality('AAC', pcm, {})
        self.assertEqual((value['bitrate_bps'], value['aac_abr_level'], value['aac_abr_adjustments']), (160000, 2, 2))
        pcm['Running'] = False
        self.assertIsNone(quality('AAC', pcm, {})['bitrate_bps'])
