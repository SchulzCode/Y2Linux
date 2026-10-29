"""The BlueALSA build manifest (codec_manifest.py, a host build tool) and the native observer/reconnect C
helpers. The platform's Bluetooth observation and power action are tested in Rust
(platform/crates/y2-platform/src/commands/connectivity/bluetooth_tests.rs, bt_control_tests.rs)."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))
from codec_manifest import manifest


class BluetoothObservation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)

    def test_build_manifest_rejects_accidental_optional_encoder_enablement(self):
        root = Path(self.temp.name)
        (root/'src').mkdir(); (root/'src/bluealsad').touch()
        (root/'config.h').write_text('#define PACKAGE_VERSION "v5.0.0"\n')
        self.assertFalse(manifest(root)['codecs']['AAC']['compiled_locally'])
        (root/'config.h').write_text('#define PACKAGE_VERSION "v5.0.0"\n#define ENABLE_AAC 1\n')
        with self.assertRaisesRegex(ValueError, 'not_approved'):
            manifest(root)

    def test_private_integration_scope_never_claims_physical_or_public_approval(self):
        root = Path(self.temp.name)
        (root/'src/dbus').mkdir(parents=True)
        (root/'src/bluealsad').touch()
        (root/'src/dbus/org.bluealsa.xml').write_text('<property name="EncoderStats"/>')
        (root/'config.h').write_text('#define PACKAGE_VERSION "v5.0.0"\n#define WITH_LIBFREEAPTX 1\n' +
            ''.join('#define ENABLE_' + codec + ' 1\n' for codec in ('AAC', 'APTX', 'APTX_HD', 'LDAC')))
        result = manifest(root, 'owner-private-integration')
        self.assertTrue(result['private_integration_enabled'])
        self.assertTrue(result['live_bitrate_observation'])
        for name, codec in result['codecs'].items():
            self.assertTrue(codec['default_runtime_enabled'])
            self.assertFalse(codec['platform_qualified'])
            self.assertEqual(codec['distribution_approved'], name in ('SBC', 'AAC', 'LDAC'))

    def test_public_profile_is_sbc_aac_ldac_without_aptx(self):
        # RELEASE-49 owner decision: aptX/aptX HD never in a public image.
        root = Path(self.temp.name)
        (root/'src').mkdir(); (root/'src/bluealsad').touch()
        config = '#define PACKAGE_VERSION "v5.0.0"\n#define ENABLE_AAC 1\n#define ENABLE_LDAC 1\n'
        (root/'config.h').write_text(config)
        result = manifest(root, 'public')
        compiled = {n for n, c in result['codecs'].items() if c['compiled_locally']}
        self.assertEqual(compiled, {'SBC', 'AAC', 'LDAC'})
        self.assertTrue(all(c['distribution_approved'] for n, c in result['codecs'].items() if n in compiled))
        from tools.production.privacy import public_distribution
        self.assertTrue(public_distribution({'radio_firmware': {'included': False}}, result)['public_distribution_ready'])
        for extra in ('#define ENABLE_APTX 1\n#define WITH_LIBFREEAPTX 1\n', '#define ENABLE_OPUS 1\n'):
            (root/'config.h').write_text(config + extra)
            with self.assertRaisesRegex(ValueError, 'not_approved'):
                manifest(root, 'public')
        (root/'config.h').write_text(config.replace('#define ENABLE_LDAC 1\n', ''))
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            manifest(root, 'public')

    def test_private_profile_requires_complete_encoder_set_and_keeps_auto_closed(self):
        root = Path(self.temp.name)
        (root/'src').mkdir(); (root/'src/bluealsad').touch()
        config = '#define PACKAGE_VERSION "v5.0.0"\n#define WITH_LIBFREEAPTX 1\n'
        config += ''.join('#define ENABLE_' + codec + ' 1\n'
                          for codec in ('AAC', 'APTX', 'APTX_HD', 'LDAC'))
        (root/'config.h').write_text(config)
        result = manifest(root, 'owner-private-experiments')
        self.assertEqual(set(result['optional_encoder_provenance']), {'fdk-aac', 'libfreeaptx', 'libldac'})
        for name, codec in result['codecs'].items():
            self.assertTrue(codec['compiled_locally'])
            self.assertFalse(codec['platform_qualified'])
            self.assertEqual(codec['default_runtime_enabled'], name == 'SBC')
            self.assertEqual(codec['distribution_approved'], name in ('SBC', 'AAC', 'LDAC'))
        args = root/'daemon.args'
        subprocess.run([sys.executable, str(ROOT/'tools/platform/codec_manifest.py'), str(root),
                        '--profile', 'owner-private-experiments', '--daemon-args', str(args)],
                       check=True, capture_output=True)
        self.assertEqual(args.read_text().split(),
                         ['--codec=-AAC', '--codec=-aptX', '--codec=-aptX-HD', '--codec=-LDAC'])
        for invalid, message in ((config.replace('#define ENABLE_LDAC 1', ''), 'incomplete'),
                                 (config + '#define ENABLE_OPUS 1\n', 'not_approved'),
                                 (config.replace('WITH_LIBFREEAPTX', 'WITH_LIBOPENAPTX'), 'library_mismatch')):
            (root/'config.h').write_text(invalid)
            with self.assertRaisesRegex(ValueError, message):
                manifest(root, 'owner-private-experiments')

    def test_native_observer_private_bus_reports_absent_owners_as_stable_none(self):
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
        finally:
            daemon.terminate();daemon.communicate(timeout=3)

    def test_observer_numeric_manufacturer_keys_and_64_bit_counters_are_valid_json(self):
        flags = subprocess.run(['pkg-config', '--cflags', '--libs', 'gio-2.0'], capture_output=True, text=True)
        if flags.returncode:
            self.skipTest('native GIO unavailable; run this case on packaging host and verify fresh ARM observer')
        binary = Path(self.temp.name) / 'observer-json'
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', str(ROOT/'tests/native/bt-observe-json.c'), *flags.stdout.split(), '-o', str(binary)], check=True)
        result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=3, check=True)
        observed = json.loads(result.stdout)
        self.assertEqual(observed['ManufacturerData'], {'117': [0, 1, 76]})
        self.assertEqual(observed['Counter'], 9223372036854775813)
        self.assertEqual(observed['Signed'], -9223372036854775807)
        self.assertEqual(observed['Finite'], 1.5)
        self.assertIsNone(observed['Nonfinite'])
        self.assertEqual(observed['Alias'], 'escaped "name"')

    def test_reconnect_signal_cache_intents_and_bounded_recovery(self):
        flags = subprocess.run(['pkg-config', '--cflags', '--libs', 'gio-2.0', 'gio-unix-2.0'], capture_output=True, text=True)
        if flags.returncode:
            self.skipTest('native GIO unavailable; run real fake-peer reconnect on packaging host and verify fresh ARM build')
        binary = Path(self.temp.name) / 'reconnect-events'
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', str(ROOT/'tests/native/bt-reconnect-events.c'), *flags.stdout.split(), '-o', str(binary)], check=True)
        result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=25, check=True)
        self.assertIn('PASS: idle snapshots=1', result.stdout)

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
