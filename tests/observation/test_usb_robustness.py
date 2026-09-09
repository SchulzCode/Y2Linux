"""Host orchestration fixtures; never enumerate/open a physical device."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from tools.observation import usb_log_robustness as trial


class Robustness(unittest.TestCase):
    def run_case(self,mode):
        class Clock:
            now=0
            def monotonic(self): return self.now
            def monotonic_ns(self): return int(self.now*1e9)
            def sleep(self,seconds): self.now+=seconds
        clock=Clock();calls=[];find_calls=0;first_end=None
        first={'sysfs_path':'/sys/devices/usb5/5-2','devnum':'2','manufacturer':trial.MANUFACTURER}
        second=dict(first,devnum='3')
        if mode=='wrong-port': second['sysfs_path']='/sys/devices/usb5/5-3'
        def find():
            nonlocal find_calls
            find_calls+=1
            if find_calls==1:
                return (Path('/dev/ttyACM4'),first) if mode=='already-present' else None
            if first_end is None:return Path('/dev/ttyACM4'),first
            if mode=='no-reconnect' or clock.now-first_end<5:return None
            return Path('/dev/ttyACM9'),second
        def receive(fd,folder,seconds,limit,identity,**kwargs):
            nonlocal first_end
            calls.append((clock.now,seconds,identity,kwargs))
            if len(calls)==1:
                clock.now+=12.1;first_end=clock.now
                return {'protocol_header':True,'status':'disconnected','events':[
                    {'event':e} for e in ('read-pause-start','read-pause-end','disconnect-prompt')]}
            clock.now+=seconds
            return {'protocol_header':mode!='bad-replay','exit_code':2 if mode=='bad-replay' else 0}
        with tempfile.TemporaryDirectory() as d, patch.object(trial,'time',clock), \
             patch.object(trial,'find_y2',side_effect=find), \
             patch.object(trial,'open_verified',return_value=123), \
             patch.object(trial.os,'close'), patch.object(trial,'receive',side_effect=receive):
            output=Path(d)/'trial';rc=trial.run(output)
            summary=json.loads((output/'robustness.json').read_text())
            self.assertFalse(summary['hardware_qualified'])
            self.assertEqual(summary['exit_code'],rc)
            return rc,summary,calls,clock.now

    def test_combined_timing_and_tty_rediscovery(self):
        rc,summary,calls,elapsed=self.run_case('success')
        self.assertEqual(rc,0);self.assertEqual(len(calls),2)
        self.assertGreaterEqual(calls[0][0],8)
        self.assertEqual(calls[0][3],dict(pause_after=3,pause_seconds=6,disconnect_after=12))
        self.assertEqual(calls[1][2]['devnum'],'3')
        self.assertLessEqual(elapsed,42.2)
        self.assertEqual(summary['status'],'captured-review-required')
        opens=[e['device'] for e in summary['events'] if e['event'].endswith('-open')]
        self.assertEqual(opens,['/dev/ttyACM4','/dev/ttyACM9'])

    def test_incomplete_trials_never_qualify(self):
        for mode in ('already-present','wrong-port','no-reconnect','bad-replay'):
            with self.subTest(mode=mode):
                rc,summary,calls,elapsed=self.run_case(mode)
                self.assertEqual(rc,2);self.assertEqual(summary['status'],'incomplete')
                if mode=='already-present':self.assertFalse(calls)
                self.assertLessEqual(elapsed,42.2)
