"""PTY and fake sysfs fixtures; no Y2 access or hardware proof."""
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import tempfile
import threading
import unittest
from tools.observation.usb_log_capture import HEADER,receive,usb_identity


class UsbCapture(unittest.TestCase):
    def capture(self,payload,limit=4096,**kwargs):
        with tempfile.TemporaryDirectory() as d:
            output=Path(d);master,slave=pty.openpty();command=[]
            os.set_blocking(slave,False)
            def peer():
                if select.select([master],[],[],2)[0]:
                    command.append(os.read(master,128));os.write(master,payload)
            thread=threading.Thread(target=peer);thread.start()
            try:
                report=receive(slave,output,.3,limit,{'fixture':'PTY'},**kwargs)
                thread.join(2)
                self.assertEqual(command,[b'LOG1\n'])
                raw=(output/'raw.bin').read_bytes()
                self.assertEqual(report,json.loads((output/'capture.json').read_text()))
                self.assertEqual(report['sha256'],hashlib.sha256(raw).hexdigest())
                self.assertFalse(report['hardware_qualified'])
                offset=0
                for line in (output/'chunks.jsonl').read_text().splitlines():
                    chunk=json.loads(line);self.assertEqual(chunk['offset'],offset);offset+=chunk['bytes']
                self.assertEqual(offset,len(raw))
                return report,raw
            finally: os.close(master);os.close(slave)

    def test_exact_protocol_and_bytes(self):
        payload=HEADER+b'6,1,12,-;kernel\nPID1 live\n\x00\xff\x11\x13'
        report,raw=self.capture(payload)
        self.assertEqual(raw,payload);self.assertEqual(report['exit_code'],0)

    def test_wrong_protocol_is_not_success(self):
        report,raw=self.capture(b'unrelated serial output\n')
        self.assertFalse(report['protocol_header']);self.assertEqual(report['exit_code'],2)

    def test_explicit_previous_build(self):
        header=b'Y2LOG1 M2-USBACM-03\n'
        report,raw=self.capture(header+b'PID1 previous build\n',expected_header=header)
        self.assertTrue(report['protocol_header']);self.assertEqual(report['exit_code'],0)

    def test_byte_limit_is_incomplete(self):
        report,raw=self.capture(HEADER+b'overflow',len(HEADER))
        self.assertEqual(raw,HEADER);self.assertEqual(report['status'],'byte-limit')
        self.assertEqual(report['exit_code'],2)

    def test_identity_gate(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);usb=root/'devices/usb1/1-1';interface=usb/'1-1:1.0'
            tty=interface/'tty/ttyACM0';tty.mkdir(parents=True)
            cls=root/'class/tty/ttyACM0';cls.mkdir(parents=True);(cls/'device').symlink_to(tty)
            for name,value in {'idVendor':'0525','idProduct':'a4a7','descriptors':'fixture'}.items():
                (usb/name).write_text(value)
            for name,value in {'bInterfaceClass':'02','bInterfaceSubClass':'02','bInterfaceProtocol':'01'}.items():
                (interface/name).write_text(value)
            device=Path('/dev/ttyACM0')
            self.assertEqual(usb_identity(device,root)['idProduct'],'a4a7')
            (usb/'idVendor').write_text('0e8d')
            with self.assertRaises(ValueError): usb_identity(device,root)
            (usb/'idVendor').write_text('0525');(interface/'bInterfaceClass').write_text('ff')
            with self.assertRaises(ValueError): usb_identity(device,root)

    def test_read_pause_retains_bytes_and_timing(self):
        with tempfile.TemporaryDirectory() as d:
            output=Path(d);master,slave=pty.openpty();os.set_blocking(slave,False)
            def peer():
                import time
                self.assertTrue(select.select([master],[],[],2)[0])
                self.assertEqual(os.read(master,128),b'LOG1\n')
                os.write(master,HEADER)
                time.sleep(.2)
                os.write(master,b'PID1 while host is not reading\n')
            thread=threading.Thread(target=peer);thread.start()
            try:
                report=receive(slave,output,.7,4096,{'fixture':'PTY'},pause_after=.1,pause_seconds=.3)
                thread.join(2)
                self.assertEqual((output/'raw.bin').read_bytes(),HEADER+b'PID1 while host is not reading\n')
                events=report['events']
                self.assertEqual([x['event'] for x in events],['read-pause-start','read-pause-end'])
                chunks=[json.loads(x) for x in (output/'chunks.jsonl').read_text().splitlines()]
                for chunk in chunks:
                    self.assertFalse(events[0]['elapsed_ns']<=chunk['elapsed_ns']<events[1]['elapsed_ns'])
                self.assertEqual(report['exit_code'],0)
            finally: os.close(master);os.close(slave)


class CaptureArguments(unittest.TestCase):
    def test_build_specific_duration_defaults_and_limits(self):
        import contextlib
        import io
        from tools.observation.usb_log_capture import parse_args
        for build in ('M2-BASELINE-01', 'M2-BASELINE-02', 'M2-BASELINE-03'):
            args=['--build',build,'--output','unused-test-output']
            self.assertEqual(parse_args(args).seconds,180)
            self.assertEqual(parse_args(args+['--seconds','60']).seconds,60)
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                parse_args(args+['--seconds','296'])
        self.assertEqual(parse_args(['--output','unused-test-output']).seconds,45)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parse_args(['--output','unused-test-output','--seconds','180'])
