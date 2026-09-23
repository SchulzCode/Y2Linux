import copy
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from y2_platform.bluetooth import normalize
from y2_platform.common import Context
from codec_manifest import manifest
from y2_platform.bt_control import power


class BluetoothObservation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.ctx = Context(self.temp.name)
        self.device = '/org/bluez/hci0/dev_00_11_22_33_44_55'
        self.pcm = '/org/bluealsa/hci0/dev_00_11_22_33_44_55/a2dpsrc/sink'
        self.raw = {'schema':1,'stable_owners':True,'bluez_owner':':1.2','bluealsa_owner':':1.3',
                    'bluez':{'/org/bluez/hci0':{'org.bluez.Adapter1':{'Powered':True}},
                             self.device:{'org.bluez.Device1':{'Address':'00:11:22:33:44:55','Connected':True,'Paired':True,'Trusted':True}}},
                    'bluealsa':{self.pcm:{'org.bluealsa.PCM1':{'Device':self.device,'Transport':'A2DP-source','Mode':'sink',
                        'Codec':'SBC','Format':0x8210,'Rate':44100,'Channels':2,'Sequence':3,'Running':True}}},
                    'manager':{'Version':'5.0.0','Codecs':['a2dp-source:SBC']}, 'codec_pcm':self.pcm,
                    'mutually_available_codecs':{'SBC':{'Rates':[44100,48000],'Channels':[2]}}}

    def test_observation_separates_local_remote_mutual_and_actual_pcm(self):
        result = normalize(self.ctx, self.raw)
        self.assertEqual(result['negotiated_codec'], 'SBC')
        self.assertEqual(result['pcm']['format'], 'S16_LE')
        self.assertEqual(result['runtime_enabled'], ['SBC'])
        self.assertIsNone(result['remote_advertised'])
        self.assertIsNone(result['requested_codec'])
        self.assertIsNone(result['actual_bitrate_bps'])
        self.assertEqual(list(result['mutually_usable']), ['SBC'])
        pcm = self.raw['bluealsa'][self.pcm]['org.bluealsa.PCM1']
        pcm.update(Codec='aptX-HD', Format=0x8418)
        result = normalize(self.ctx, self.raw)
        self.assertEqual(result['pcm']['valid_bits'], 24)
        self.assertEqual(result['pcm']['physical_bits'], 32)

    def test_disappearance_wrong_direction_owner_change_and_missing_properties_fail_closed(self):
        self.raw['bluez'][self.device]['org.bluez.Device1']['Connected'] = False
        self.assertIsNone(normalize(self.ctx, self.raw)['negotiated_codec'])
        self.raw['bluez'][self.device]['org.bluez.Device1']['Connected'] = True
        pcm = self.raw['bluealsa'][self.pcm]['org.bluealsa.PCM1']
        pcm['Mode'] = 'source'
        self.assertIsNone(normalize(self.ctx, self.raw)['pcm'])
        pcm['Mode'] = 'sink'
        pcm['Format'] = 123
        self.assertEqual(normalize(self.ctx, self.raw)['state'], 'Degraded')
        self.raw['stable_owners'] = False
        self.assertIsNone(normalize(self.ctx, self.raw)['pcm'])

    def test_build_manifest_rejects_accidental_optional_encoder_enablement(self):
        root = Path(self.temp.name)
        (root/'src').mkdir(); (root/'src/bluealsad').touch()
        (root/'config.h').write_text('#define PACKAGE_VERSION "v5.0.0"\n')
        self.assertFalse(manifest(root)['codecs']['AAC']['compiled_locally'])
        (root/'config.h').write_text('#define PACKAGE_VERSION "v5.0.0"\n#define ENABLE_AAC 1\n')
        with self.assertRaisesRegex(ValueError, 'not_approved'):
            manifest(root)

    def test_native_observer_private_bus_with_missing_owners_is_not_ready(self):
        result = subprocess.run(['pkg-config','--cflags','--libs','gio-2.0'], capture_output=True, text=True)
        if result.returncode:
            self.skipTest('native GIO development files unavailable; ARM package build covers compilation')
        binary = Path(self.temp.name) / 'observe'
        subprocess.run(['cc','-Wall','-Wextra','-Werror',str(ROOT/'tools/connectivity/bt-observe.c'),
                        *result.stdout.split(),'-o',str(binary)],check=True)
        daemon = subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            address = daemon.stdout.readline().decode().strip()
            answer = subprocess.run([str(binary)],env={**os.environ,'DBUS_SYSTEM_BUS_ADDRESS':address},capture_output=True,timeout=5,check=True)
            raw = json.loads(answer.stdout)
            self.assertTrue(raw['stable_owners'])
            self.assertIsNone(raw['bluez_owner'])
            self.assertEqual(normalize(self.ctx, raw)['state'], 'Unavailable')
        finally:
            daemon.terminate();daemon.communicate(timeout=3)

    def test_manual_operation_cannot_overlap_auto_and_unknown_completion_inhibits(self):
        directory = self.ctx.path('/run/y2'); directory.mkdir(parents=True)
        boot = self.ctx.path('/proc/sys/kernel/random/boot_id')
        boot.parent.mkdir(parents=True)
        boot.write_text('11111111-1111-1111-1111-111111111111')
        calls = []
        def run(argv, **kwargs):
            calls.append(argv)
            return {'ok': False, 'reason': 'command_timeout', 'output': None}
        self.ctx.runner = run
        with (directory/'bt-operation.lock').open('w') as automatic:
            fcntl.flock(automatic, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(power(self.ctx, 'on')['reason'], 'platform_bluetooth_operation_in_progress')
            self.assertEqual(calls, [])
        self.assertFalse(power(self.ctx, 'on')['ok'])
        self.assertEqual(len(calls), 1)
        self.assertIn('operation=uncertain', (directory/'bt-control.ini').read_text())

    def test_native_reconnect_budget_does_not_retry_forever_or_reset_on_poll(self):
        source = Path(self.temp.name)/'budget.c'
        source.write_text('''#include <assert.h>
#include "reconnect-policy.h"
int main(void) {
 struct y2_reconnect_budget b={0};
 assert(y2_reconnect_attempt(&b,100));
 for(int i=0;i<1000;i++) assert(!y2_reconnect_attempt(&b,101+i));
 assert(y2_reconnect_attempt(&b,2000100));
 assert(!y2_reconnect_attempt(&b,999999999));
 /* Restoring the same budget after daemon restart cannot replenish it. */
 struct y2_reconnect_budget restored=b;
 assert(!y2_reconnect_attempt(&restored,1000000000));
 y2_reconnect_reset(&b);
 assert(y2_reconnect_attempt(&b,1000000001));
 assert(!y2_reconnect_attempt(&b,1025000001));
 return 0;
}''')
        binary = source.with_suffix('')
        subprocess.run(['cc','-Wall','-Wextra','-Werror','-I',str(ROOT/'tools/connectivity'),str(source),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True,timeout=2)


if __name__ == '__main__': unittest.main()
