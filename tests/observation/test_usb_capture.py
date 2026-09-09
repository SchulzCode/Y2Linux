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
    def capture(self,payload,limit=4096):
        with tempfile.TemporaryDirectory() as d:
            output=Path(d);master,slave=pty.openpty();command=[]
            os.set_blocking(slave,False)
            def peer():
                if select.select([master],[],[],2)[0]:
                    command.append(os.read(master,128));os.write(master,payload)
            thread=threading.Thread(target=peer);thread.start()
            try:
                report=receive(slave,output,.3,limit,{'fixture':'PTY'})
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
