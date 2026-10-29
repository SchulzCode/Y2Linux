"""Golden cases of the suspend group that need a live peer: Reborn's control socket (a fake that records every
request in the tree), a held flock, the product sleep helper's inherited lock (stdin: the case's own
/run/y2/suspend.lock) or a PM node that refuses writes (read-only), and the hidden programmes. The cases are
tests/platform_cases/suspend.py's SERVED_CASES, run by the Rust host build (`Y2_PLATFORM_RUST`) and compared with
tests/platform_goldens/suspend/<case>.json (tests/platform_golden.py, through its `wrap` interface)."""
import fcntl
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import SKIP, check_all, read_golden, run_case, rust_binary, touched  # noqa: E402
from platform_cases import suspend  # noqa: E402
import test_platform_differential as base  # noqa: E402


class Reborn:
    """Reborn's control socket: one answer line per `op` (None: close without answering)."""

    def __init__(self, root, answers):
        self.path = root / 'run/reborn/control.sock'
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.log = root / 'run/reborn/requests.jsonl'
        self.answers = answers
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(self.path))
        self.server.listen(4)
        self.server.settimeout(0.1)
        self.running = True
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self):
        while self.running:
            try:
                connection, _ = self.server.accept()
            except (socket.timeout, OSError):
                continue
            with connection:
                connection.settimeout(5)
                data = b''
                while b'\n' not in data:
                    chunk = connection.recv(4096)
                    if not chunk:
                        break
                    data += chunk
                request = json.loads(data.split(b'\n')[0])
                with open(self.log, 'a') as log:
                    log.write(json.dumps(request) + '\n')
                answer = self.answers.get(request['command']['op'])
                if answer is not None:
                    connection.sendall(answer.encode() + b'\n')

    def stop(self):
        self.running = False
        self.thread.join()
        self.server.close()
        self.path.unlink()


def served(side, setup):
    """`side` with the case's peers set up on its own fixture copy."""
    def run(root, argv, commands_file, calls_file, timeout, env):
        argv = [a.replace('{root}', str(root)) for a in argv]
        stdin = None
        if setup.get('inherit'):
            # The run's stdin: product-sleep's inherited open file description, which it flocks. The case's
            # own file (never a shared one such as /dev/null, whose flock any other process could hold).
            stdin = root / 'run/y2/suspend.lock'
            stdin.unlink(missing_ok=True)
            stdin.touch(0o600)
        for name in setup.get('readonly', ()):
            (root / name.lstrip('/')).chmod(0o444)  # read as before, every write refused (EACCES)
        held = []
        for name in setup.get('hold', ()):
            path = root / name.lstrip('/')
            path.parent.mkdir(parents=True, exist_ok=True)
            fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
            fcntl.flock(fd, fcntl.LOCK_EX)
            held.append(fd)
        reborn = Reborn(root, setup['reborn']) if 'reborn' in setup else None
        try:
            return side(root, argv, commands_file, calls_file, timeout, env, stdin=stdin)
        finally:
            for fd in held:
                os.close(fd)
            if reborn:
                reborn.stop()
    return run


def options(case, setup):
    return {**case, 'volatile': (*case.get('volatile', ()), *suspend.VOLATILE),
            'wrap': lambda side: served(side, setup)}


def runnable(setup):
    # A read-only node refuses writes only to an unprivileged user (uid 0 overrides the mode).
    return not (setup.get('readonly') and os.geteuid() == 0)


class SuspendGoldens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = base.player_root(Path(cls.directory.name) / 'fixture')

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [(('suspend', name), lambda argv=argv, case=case, setup=setup: run_case(
            self.fixture, argv, **options(case, setup))) for name, argv, case, setup in suspend.SERVED_CASES
            if runnable(setup)])

    def test_goldens_are_the_contract(self):
        results = {name: read_golden('suspend', name) for name, _, _, _ in suspend.SERVED_CASES}
        slept = results['product-sleep']
        self.assertEqual(slept['exit'], 0)
        tree = touched(slept)
        receipt = tree[suspend.SLEEP]['content']
        self.assertEqual(receipt['product']['state'], 'restored')
        self.assertEqual(tree[suspend.GUARD]['content']['pending'], False)
        self.assertEqual(tree['/sys/power/state']['content'], ['mem'])
        self.assertEqual(tree['/sys/firmware/y2_pm/backstop_s']['content'], 0)  # "0\n" reads as JSON
        self.assertEqual(tree['/sys/firmware/y2_pm/stage']['content'], ['COMPLETE'])
        self.assertEqual([c['argv'] for c in slept['calls']], suspend.RADIOS[:3])
        refused = results['product-sleep-kernel-prepare-refused']
        self.assertEqual(refused['exit'], 2)
        self.assertIn('/run/y2/product-sleep-retry.json', touched(refused))
        # F47: a refused sleep restores dark.
        requests = touched(refused)['/run/reborn/requests.jsonl']['content']
        self.assertEqual(json.loads(requests[-1])['command'], {'op': 'sleep_restore', 'dark': True})
        failed = results['product-sleep-kernel-error']
        self.assertEqual((failed['exit'], touched(failed)[suspend.GUARD]['content']['pending']), (1, True))
        self.assertEqual(results['sleep-request-busy']['stdout']['reason'], 'sleep_operation_in_progress')
        self.assertEqual(touched(results['product-sleep-activity-busy'])[suspend.SLEEP]['content']['product']['reason'],
                         'activity_operation')
        self.assertEqual(results['power-command-flags']['exit'], 2)
        self.assertEqual(results['power-audit']['exit'], 0)
        self.assertEqual(touched(results['power-audit'])['/audit.json']['content']['schema'],
                         'org.y2linux.power-audit/v2')

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_rust_answers_to_the_script_names(self):
        binary = rust_binary()
        with tempfile.TemporaryDirectory() as directory:
            for name in ('reboot', 'poweroff', 'y2-power-command'):
                link = Path(directory) / name
                link.symlink_to(binary)
                result = subprocess.run([str(link), '-f'], capture_output=True, text=True,
                                        env={'PATH': '/usr/bin:/bin', 'Y2_ROOT': directory})
                with self.subTest(name):
                    self.assertEqual(result.returncode, 2)
                    self.assertEqual(result.stdout, '')
                    if name != 'y2-power-command':
                        self.assertIn('emergency flags are not supported', result.stderr)


if __name__ == '__main__':
    unittest.main()
