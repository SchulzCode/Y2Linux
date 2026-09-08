"""PTY tests only: these never establish a Y2 electrical connection or boot proof."""
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


class Receiver(unittest.TestCase):
    def run_capture(self, payload=b'', max_bytes=65536, disconnect=False):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            setup = root/'setup.txt'
            setup.write_text('HOST PTY FIXTURE ONLY; no device, wiring or voltage evidence.\n')
            master, slave = pty.openpty()
            proc = None
            try:
                proc = subprocess.Popen([sys.executable, str(ROOT/'tools/observation/capture.py'),
                    '--device', os.ttyname(slave), '--output', str(root/'capture'),
                    '--setup', str(setup), '--seconds', '0.7', '--max-bytes', str(max_bytes)],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                manifest = root/'capture/capture.json'
                for _ in range(200):
                    if manifest.exists():
                        try:
                            if json.loads(manifest.read_text())['status'] == 'capturing': break
                        except json.JSONDecodeError: pass
                    if proc.poll() is not None:
                        self.fail(manifest.read_text() if manifest.exists() else str(proc.communicate()))
                    time.sleep(.01)
                else: self.fail('capture never became ready')
                os.close(slave); slave = None
                if payload: os.write(master, payload)
                if disconnect:
                    time.sleep(.1)
                    os.close(master); master = None
                stdout, stderr = proc.communicate(timeout=5)
                self.assertEqual(stderr, b'')
                report = json.loads(manifest.read_text())
                raw = (root/'capture/raw.bin').read_bytes()
                chunks = [json.loads(s) for s in (root/'capture/chunks.jsonl').read_text().splitlines()]
                self.assertEqual(report['sha256'], hashlib.sha256(raw).hexdigest())
                self.assertEqual(report['bytes'], len(raw))
                self.assertEqual(report['exit_code'], proc.returncode)
                self.assertFalse(report['hardware_proof'])
                if master is not None:
                    # No echo or writes into the PTY master while capture ran.
                    if select.select([master],[],[],0)[0]:
                        try: self.assertEqual(os.read(master,65536), b'')
                        except OSError: pass  # PTY slave closed
                return report, raw, chunks
            finally:
                if proc is not None:
                    if proc.poll() is None: proc.kill()
                    proc.communicate()
                for fd in (master, slave):
                    if fd is not None: os.close(fd)

    def test_byte_exact_and_no_tx(self):
        payload = b'fixture LK\r\nLinux\x00\xff\x13\x11\x1b[2J\r\n'
        report, raw, chunks = self.run_capture(payload)
        self.assertEqual(raw, payload)
        self.assertEqual(report['status'], 'data-captured')
        self.assertEqual(report['exit_code'], 0)
        offset = 0
        for chunk in chunks:
            self.assertEqual(chunk['offset'], offset)
            self.assertGreaterEqual(chunk['elapsed_ns'], 0)
            offset += chunk['bytes']
        self.assertEqual(offset, len(payload))

    def test_empty_is_not_success(self):
        report, raw, _ = self.run_capture()
        self.assertEqual((report['status'], report['exit_code'], raw), ('empty',2,b''))

    def test_bound_is_incomplete(self):
        report, raw, _ = self.run_capture(b'0123456789', max_bytes=5)
        self.assertEqual((report['status'],report['exit_code'],raw),('limit-reached-incomplete',2,b'01234'))

    def test_disconnect_preserves_evidence(self):
        report, raw, _ = self.run_capture(b'prior bytes', disconnect=True)
        self.assertEqual(raw,b'prior bytes')
        self.assertEqual(report['status'],'error')
        self.assertEqual(report['exit_code'],2)


if __name__ == '__main__': unittest.main()
