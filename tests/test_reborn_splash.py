"""Protocol/boot-policy tests. No DRM device or running Reborn is touched."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
REBORN = ROOT.parent / 'Y2Reborn'


class Splash(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix='reborn-splash-build-')
        cls.binary = Path(cls.build.name) / 'splash'
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'libdrm'], text=True).split()
        subprocess.run(['cc', '-DRB_SPLASH_TEST', '-Os', '-Wall', '-Wextra', '-Werror',
                        str(ROOT / 'tools/graphics/reborn-splash.c'), *flags, '-o', str(cls.binary)], check=True)
        driver = Path(cls.build.name) / 'client.c'
        header = REBORN / 'crates/reborn-graphics/native/splash-handoff.h'
        driver.write_text('''#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static double now(void) {struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
''' + '#include "' + str(header) + '"\n' + '''
int main(int argc,char **argv) {
 if(argc!=2)return 2;
 int fd=rb_splash_begin(argv[1]);
 if(fd < -1)return 3;
 if(fd == -1){puts("absent");return 0;}
 puts("released");rb_splash_presented(fd);return 0;
}
''')
        cls.client_binary = Path(cls.build.name) / 'client'
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', str(driver), '-o', str(cls.client_binary)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reborn-splash-')
        self.root = Path(self.temp.name)
        self.sock = self.root / 'control.sock'
        self.process = subprocess.Popen([str(self.binary), '--test', str(self.root), str(self.sock)])
        self.wait(lambda: self.sock.exists())

    def tearDown(self):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=3)
        self.temp.cleanup()

    def wait(self, predicate, seconds=2):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if predicate(): return
            time.sleep(.01)
        self.fail('deadline expired')

    def connect(self):
        s = socket.socket(socket.AF_UNIX); s.settimeout(2); s.connect(str(self.sock)); return s

    def events(self):
        return [json.loads(s)['event'] for s in (self.root / 'events.jsonl').read_text().splitlines()]

    def test_native_client_explicit_ready_then_presented(self):
        self.assertEqual(self.sock.stat().st_mode & 0o777, 0o600)
        p = subprocess.run([str(self.client_binary), str(self.sock)], capture_output=True, timeout=3)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(p.stdout.strip(), b'released')
        self.assertEqual(self.process.wait(timeout=3), 0)
        e = self.events()
        self.assertLess(e.index('splash_test_release'), e.index('splash_presented'))
        self.assertTrue((self.root / 'done').exists())

    def test_timeout_is_static_failure_but_late_ready_still_works(self):
        self.wait(lambda: 'splash_timeout' in self.events())
        self.assertIsNone(self.process.poll())
        self.assertFalse((self.root / 'done').exists())
        p = subprocess.run([str(self.client_binary), str(self.sock)], capture_output=True, timeout=3)
        self.assertEqual(p.returncode, 0)
        self.assertEqual(self.process.wait(timeout=3), 0)

    def test_disconnect_during_handoff_reclaims_display(self):
        with self.connect() as s:
            s.sendall(b'READY 1\n'); self.assertEqual(s.recv(32), b'RELEASED 1\n')
        self.wait(lambda: 'splash_test_claim' in self.events())
        self.assertIn('splash_handoff_aborted', self.events())
        self.assertFalse((self.root / 'done').exists())

    def test_malformed_and_oversized_requests_do_not_release_kms(self):
        for message in (b'PRESENTED 1\n', b'exec reboot\n', b'x'*100):
            with self.connect() as s:
                s.sendall(message)
                try: self.assertEqual(s.recv(32), b'')
                except ConnectionResetError: pass
        self.assertNotIn('splash_test_release', self.events())

    def test_slow_client_deadline_and_singleton(self):
        with self.connect() as s:
            s.settimeout(4); s.sendall(b'R')
            self.assertEqual(s.recv(32), b'')
        p = subprocess.run([str(self.binary), '--test', str(self.root), str(self.sock)], timeout=2)
        self.assertEqual(p.returncode, 0)
        self.assertIsNone(self.process.poll())

    def test_no_service_is_a_compatible_renderer_fallback(self):
        p = subprocess.run([str(self.client_binary), str(self.root / 'missing')], capture_output=True, timeout=3)
        self.assertEqual((p.returncode, p.stdout.strip()), (0, b'absent'))

    def test_client_rejects_wrong_reply(self):
        address = self.root / 'wrong'
        server = socket.socket(socket.AF_UNIX); server.bind(str(address)); server.listen(1)
        def answer():
            with server.accept()[0] as s:
                self.assertEqual(s.recv(32), b'READY 1\n'); s.sendall(b'NOTREADY 1\n')
        thread = threading.Thread(target=answer); thread.start()
        p = subprocess.run([str(self.client_binary), str(address)], capture_output=True, timeout=3)
        thread.join(timeout=3); server.close(); self.assertNotEqual(p.returncode, 0)

    def test_early_quiet_precedes_display_and_splash_follows_charger_gate(self):
        init = (ROOT / 'initramfs/production/init').read_text()
        self.assertLess(init.index('--quiet-if-normal'), init.index('/sbin/y2-platform-start'))
        self.assertLess(init.index("/sbin/y2-offline-charge || rescue"), init.index('/sbin/reborn-splash --start'))
        self.assertLess(init.index('/sbin/reborn-splash --start'), init.index('y2_find_partition'))
        source = (ROOT / 'tools/graphics/reborn-splash.c').read_text()
        self.assertIn('valid==1 && offline==0', source)
        self.assertNotIn('KDSETMODE,KD_TEXT', source)
        self.assertNotIn('system(', source)
        display = (ROOT / 'buildroot/board/y2/overlay/etc/init.d/S25y2-display').read_text()
        self.assertLess(display.index('reborn-splash'), display.index('echo 0'))


if __name__ == '__main__': unittest.main()
