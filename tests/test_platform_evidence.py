"""Real readonly ALSA query, precision bytes and bounded previous-boot retention."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import wave
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools/platform'))
from audio_precision import generate
from y2_platform.common import Context
from y2_platform.boot import capture_previous, evidence_status, loader_boot, mark, pmic_power_status


class EvidenceTests(unittest.TestCase):
    def test_precision_files_preserve_low_bits_and_independent_channels(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)/'fixtures'
            manifest = generate(path)
            raw = (path/'reference-s32le.raw').read_bytes()
            low = (path/'reference-s24le.raw').read_bytes()
            words = struct.unpack('<'+'i'*(len(raw)//4), raw)
            self.assertEqual(words, tuple(v*256 for v in struct.unpack('<'+'i'*(len(low)//4), low)))
            self.assertTrue(any(v & 0xffff for v in words))
            self.assertNotEqual(words[::2], words[1::2])
            for rate in (44100,48000,88200,96000):
                with wave.open(str(path/f'precision-s24-{rate}.wav')) as stream:
                    self.assertEqual((stream.getsampwidth(),stream.getnchannels(),stream.getframerate()), (3,2,rate))
                    packed = stream.readframes(stream.getnframes())
                self.assertEqual(tuple(int.from_bytes(packed[i:i+3],'little',signed=True)*256
                                       for i in range(0,len(packed),3)), words)
            self.assertFalse(manifest['physical_playback'])
            with self.assertRaises(FileExistsError): generate(path)

    def test_actual_alsa_null_constraints_do_not_claim_hardware(self):
        if not shutil.which('pkg-config') or subprocess.run(['pkg-config','--exists','alsa']).returncode:
            self.skipTest('native ALSA development dependency; separate host/ARM query receipt required')
        with tempfile.TemporaryDirectory() as root:
            executable = Path(root)/'query'
            flags = subprocess.check_output(['pkg-config','--cflags','--libs','alsa'],text=True).split()
            subprocess.run(['cc','-Wall','-Wextra','-Werror',str(ROOT/'tools/connectivity/audio-contract.c'),
                            *flags,'-o',str(executable)],check=True)
            result = json.loads(subprocess.check_output([str(executable),'--device','null'],timeout=3))
            self.assertEqual(len(result['combinations']),16)
            self.assertTrue(all(c['accepted_constraints'] for c in result['combinations']))
            self.assertFalse(result['physical_qualification'])
            self.assertFalse(result['pcm_started'])
            self.assertFalse(result['constraint_bits_are_hardware_precision'])
            self.assertTrue(all(c['meaningful_bits'] is None for c in result['combinations']))
            self.assertEqual({c['format'] for c in result['combinations']}, {'S16_LE', 'S24_LE', 'S32_LE', 'S24_3LE'})
            self.assertEqual(subprocess.run([str(executable),'--device','default'],capture_output=True).returncode,2)

    def test_previous_boot_logs_are_private_bounded_once_only_and_not_reset_diagnosis(self):
        with tempfile.TemporaryDirectory() as root:
            ctx = Context(root)
            def put(name,value):
                path=ctx.path(name);path.parent.mkdir(parents=True,exist_ok=True);path.write_text(value);return path
            put('/data/.y2data-schema','1')
            put('/proc/self/mountinfo','3 1 179:2 / /data rw - ext4 /dev/data rw\n')
            put('/run/y2-data-device','/dev/data')
            put('/proc/sys/kernel/random/boot_id','new-boot')
            put('/data/system/platform/boot.json',json.dumps({'schema':1,'boot_id':'previous','last_stage':'application_ready','orderly_shutdown':False}))
            log=put('/data/logs/system.log', ('private previous message '+'x'*600+'\n')*1000)
            result=capture_previous(ctx)
            self.assertEqual(result['previous_boot_id'],'previous')
            self.assertIsNone(result['reset_cause'])
            self.assertEqual(result['loader'], {'boot_reason': None, 'reset': None})
            self.assertFalse(result['kernel_panic_retention_guaranteed'])
            self.assertLessEqual(len(result['logs'][0]['tail_lines']),32)
            path=ctx.path('/data/system/platform/previous-boot-evidence.json')
            self.assertEqual(path.stat().st_mode&0o777,0o600)
            self.assertLessEqual(path.stat().st_size,96000)
            self.assertNotIn('private previous message',json.dumps(evidence_status(ctx)))
            log.write_text('new boot message')
            self.assertEqual(capture_previous(ctx),result)
            mark(ctx,'platform_start')
            path.unlink()
            with self.assertRaisesRegex(ValueError,'missed_early_boundary'): capture_previous(ctx)
            put('/proc/sys/kernel/random/boot_id','next-boot')
            log.unlink();log.symlink_to('/etc/passwd')
            with self.assertRaises(OSError): capture_previous(ctx)

    def test_pmic_long_press_record_is_decoded_only_when_it_changed(self):
        # F56/F57: LK leaves STRUP_CON8 bit 4 set after clearing a long-press
        # reset; a set bit proves a new one only after a boot that had it clear.
        status = lambda strup: ('valid=1 strup_con8=0x%04x top_rst_misc=0x015b chr_con16=0x0005 '
                                'strup_con5=0x0000 chrstatus=0x0000\n' % strup)
        fresh = pmic_power_status(status(0x8010), False)
        self.assertEqual((fresh['long_press_reset'], fresh['long_press_reset_reason']),
                         (True, 'lk_cleared_long_press_reset'))
        self.assertEqual((fresh['long_press_reset_enabled'], fresh['long_press_reset_seconds']), (True, 11))
        self.assertEqual((fresh['uvlo_selector'], fresh['thermal_status'], fresh['power_key_down']), (1, 0, True))
        self.assertEqual(fresh['raw']['strup_con8'], '0x8010')
        self.assertEqual(pmic_power_status(status(0x8010), True)['long_press_reset_reason'], 'record_set_before')
        self.assertIsNone(pmic_power_status(status(0x8010))['long_press_reset'])
        clear = pmic_power_status(status(0x8000), True)
        self.assertEqual((clear['long_press_reset'], clear['lk_long_press_record']), (False, False))
        for text in (None, 'valid=0\n', 'valid=1 strup_con8=error\n'):
            self.assertEqual(pmic_power_status(text, False)['state'], 'Unavailable')
        with tempfile.TemporaryDirectory() as root:
            ctx = Context(root)
            def put(name,value):
                path=ctx.path(name);path.parent.mkdir(parents=True,exist_ok=True);path.write_text(value);return path
            put('/data/.y2data-schema','1')
            put('/proc/self/mountinfo','3 1 179:2 / /data rw - ext4 /dev/data rw\n')
            put('/run/y2-data-device','/dev/data')
            put('/proc/sys/kernel/random/boot_id','new-boot')
            put('/data/system/platform/boot.json',json.dumps({'schema':1,'boot_id':'previous','orderly_shutdown':False}))
            put('/data/system/platform/previous-boot-evidence.json',json.dumps(
                {'captured_on_boot_id':'previous','pmic':pmic_power_status(status(0x8000))}))
            put('/sys/firmware/y2_pmic/power_status',status(0x8010))
            put('/sys/firmware/y2_boot/metadata','valid=1 boot_mode=0 boot_reason=0 offline=0 recovery_image=0 panel=st7701s\n')
            self.assertTrue(capture_previous(ctx)['pmic']['long_press_reset'])
            self.assertTrue(evidence_status(ctx)['pmic']['long_press_reset'])
            self.assertEqual(evidence_status(ctx)['loader'], {'boot_reason': 0, 'reset': 'cold'})

    def test_loader_start_tells_a_watchdog_reset_from_a_cut(self):
        # F56: the RGU status reads 0 by the time Linux probes; LK's boot
        # reason survives. 4 = SoC reset past the Power key (watchdog, restart).
        meta = 'valid=1 boot_mode=0 boot_reason=%s offline=0 recovery_image=0 panel=gc9503v\n'
        self.assertEqual(loader_boot(meta % 4), {'boot_reason': 4, 'reset': 'warm'})
        self.assertEqual(loader_boot(meta % 3), {'boot_reason': 3, 'reset': 'warm'})
        self.assertEqual(loader_boot(meta % 1), {'boot_reason': 1, 'reset': 'cold'})
        for text in (None, 'valid=0 boot_reason=4\n', meta % 'x'):
            self.assertEqual(loader_boot(text), {'boot_reason': None, 'reset': None})


if __name__ == '__main__': unittest.main()
