import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools/platform'))
from y2_platform.common import Context
from y2_platform import diagnostics as d


class Diagnostics(unittest.TestCase):
    def test_projection_and_event_excerpts_never_include_private_free_text(self):
        secret = 'unique-personal-secret'
        raw = {'ssid': secret, 'password': secret, 'foo': secret,
               'track_title': secret, '/private/'+secret: 1,
               'PersonalNameThatMustNeverAppear': 123, 'nested': {'temperature_c': 34, 'enabled': True, 'state': 'Ready'},
               'boot_id': '159f7c81-ab89-4440-bc89-df827dd8bc34'}
        output = d.project(raw)
        self.assertNotIn(secret, json.dumps(output))
        self.assertNotIn('PersonalNameThatMustNeverAppear', json.dumps(output))
        self.assertEqual(output['nested']['temperature_c'], 34)
        self.assertEqual(output['boot_id'], raw['boot_id'])
        log = d.events('reborn failed to open /data/music/'+secret+' error=-5\n'
                       'wifi error password='+secret+' ssid=personal\n')
        self.assertNotIn(secret, json.dumps(log))
        self.assertEqual(log[0]['errno'], [-5])
        self.assertEqual(log[0]['events'], ['failed', 'error'])

    def test_fixed_kernel_fields_keep_useful_pm_storage_usb_evidence(self):
        raw = {'spm': 'entries=1 resumes=1 wake=0x20 secret=owner-token',
               'suspend_persistent': {'current': 'valid=1 stage=EXIT error=0'},
               'mmc_runtime_pm': {'11230000.mmc': 'gated=1 suspends=42 last_mismatch=0x0'},
               'usb_restore': {'controllers': {'11200000.usb':
                               'transfer=inventra_dma dma_errors=0 irq_max_burst=26'}},
               'product': {'state': 'restored', 'same_boot': True, 'wake_reason': 'rtc'}}
        result = d.project(raw)
        self.assertEqual(result['spm']['wake'], '0x20')
        self.assertEqual(result['suspend_persistent']['current']['stage'], 'EXIT')
        self.assertEqual(result['mmc_runtime_pm']['11230000.mmc']['suspends'], '42')
        self.assertEqual(result['usb_restore']['controllers']['11200000.usb']['transfer'], 'inventra_dma')
        self.assertTrue(result['product']['same_boot'])
        self.assertEqual(result['product']['state'], 'restored')
        self.assertNotIn('owner-token', json.dumps(result))

    def test_codec_observation_survives_without_peer_identifiers(self):
        raw={'bluetooth_link':{'negotiated_codec':'LDAC','preferred_codec':'Auto',
                'runtime_enabled':['SBC','LDAC'],'codec_inventory':{'codecs':{
                    'LDAC':{'compiled_locally':True,'distribution_approved':False}}},
                'encoder':{'ldac_abr_adjustments':7,'ldac_abr_adaptation_observed':True},
                'selected_peer':{'address':'AA:BB:CC:DD:EE:FF','name':'Private peer','connected':True}}}
        result=d.project(raw)['bluetooth_link']
        self.assertEqual(result['negotiated_codec'],'LDAC')
        self.assertEqual(result['preferred_codec'],'Auto')
        self.assertEqual(result['encoder']['ldac_abr_adjustments'],7)
        self.assertTrue(result['codec_inventory']['codecs']['LDAC']['compiled_locally'])
        self.assertNotIn('AA:BB:CC:DD:EE:FF',json.dumps(result))
        self.assertNotIn('Private peer',json.dumps(result))

    def test_archive_is_private_projected_and_mount_bound(self):
        with tempfile.TemporaryDirectory() as root:
            ctx = Context(root)
            ctx.path('/data').mkdir()
            volume = {'generation': 'one', 'space_state': 'Normal', 'available_bytes': 200*1024**2}
            with patch.object(d, 'data_volume', return_value=volume), patch.object(d, 'collect', return_value={'redacted': True}):
                result = d.export(ctx)
                path = ctx.path(result['path'])
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                with tarfile.open(path) as archive:
                    self.assertEqual(archive.getnames(), ['diagnostics.json'])
                    self.assertTrue(json.load(archive.extractfile('diagnostics.json'))['redacted'])
                with patch.object(d, 'data_volume', side_effect=[volume, dict(volume, generation='two')]):
                    with self.assertRaisesRegex(ValueError, 'generation_changed'):
                        d.export(ctx)
                self.assertEqual(len(list(path.parent.glob('*.tar.gz'))), 1)
                self.assertFalse(list(path.parent.glob('*.partial')))
                with patch.object(d, 'data_volume', return_value=dict(volume, available_bytes=10)):
                    with self.assertRaisesRegex(ValueError, 'space_reserved'):
                        d.export(ctx)

    def test_log_parent_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            ctx = Context(root)
            ctx.path('/data').mkdir()
            ctx.path('/secret').mkdir()
            ctx.path('/secret/system.log').write_text('usb error private')
            ctx.path('/data/logs').symlink_to(ctx.path('/secret'), target_is_directory=True)
            with self.assertRaises(OSError):
                d.log_tail(ctx, 'logs/system.log')


if __name__ == '__main__':
    unittest.main()
