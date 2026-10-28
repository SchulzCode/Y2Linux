"""Differential cases of the suspend group that need a live peer: Reborn's control socket (a fake that records
every request in the tree), a held flock, the product sleep helper's inherited lock (stdin /dev/null made
/run/y2/suspend.lock) or a PM node that refuses writes (read-only), and the hidden programmes. The cases are
tests/platform_cases/suspend.py's SERVED_CASES; the harness is tests/platform_differential.py, used through its
`side` interface (nothing of the harness core changes)."""
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
from platform_differential import compare, host_access, python_side, run_case, rust_binary, rust_side  # noqa: E402
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
        if setup.get('inherit'):
            lock = root / 'run/y2/suspend.lock'
            lock.unlink(missing_ok=True)
            lock.symlink_to('/dev/null')  # the run's stdin: product-sleep's inherited open file description
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
            return side(root, argv, commands_file, calls_file, timeout, env)
        finally:
            for fd in held:
                os.close(fd)
            if reborn:
                reborn.stop()
    return run


def options(case):
    return {**case, 'env': {**case.get('env', {}), **suspend.PATCH},
            'volatile': (*case.get('volatile', ()), *suspend.VOLATILE)}


AUDIT = {**suspend.ready(), 'ported': True, 'volatile': (
    *suspend.VOLATILE, 'monotonic', 'collection_seconds', 'cpu_seconds', 'duration_seconds',
    'undisturbed_sleep_seconds', 'pid', 'total_cpu_seconds', 'counter_cpu_seconds', 'counter_collection_seconds',
    'max_rss_kib', 'per_second', 'cpu_percent_one_core', 'context_switches_per_second',
    'common_cpu_lower_bound_per_second', 'percent_wall_time', 'cpu0_idle_exits_per_second', 'kernel'), 'files': {
    '/proc/42/task/42/stat': '42 (y2 player) S 1 42 42 0 -1 4194560 100 0 0 0 7 3 0 0 20 0 2 0 900 0 0\n',
    '/proc/42/task/42/status': 'Name:\ty2\nvoluntary_ctxt_switches:\t10\nnonvoluntary_ctxt_switches:\t2\n',
    '/proc/42/task/42/wchan': 'do_epoll_wait\n',
    '/proc/interrupts': '           CPU0       CPU1\n 160:         12          3  mt6397  14 Edge  mt6397-rtc\n'
                        'IPI0:          5          1  Rescheduling interrupts\n',
    '/proc/stat': 'cpu  1 2 3\nctxt 100\nintr 50 1 2\nprocesses 9\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/name': 'WFI\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/usage': '10\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/time': '1000\n',
    '/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state': '598000 10\n1300000 5\n',
    '/sys/kernel/debug/wakeup_sources': 'name\tactive_count\tevent_count\twakeup_count\nbattery\t1\t2\t0\n',
    '/sys/class/udc/11200000.usb/state': 'not attached\n',
}}
AUDIT_ARGV = ['__power-audit', '--seconds', '1', '--settle', '0', '--state', 'fixture', '--output', '{root}/audit.json']


def hidden_supported(binary):
    """Whether this build's main.rs runs the suspend group's hidden programmes (before its argparse parser)."""
    with tempfile.TemporaryDirectory() as root:
        result = subprocess.run([str(binary), '__power-command', 'y2-power-command'], capture_output=True,
                                env={'PATH': '/usr/bin:/bin', 'Y2_ROOT': root})
    return result.returncode == 2 and not result.stderr


class SuspendDifferential(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = base.player_root(Path(cls.directory.name) / 'fixture')

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def cases(self):
        yield from suspend.SERVED_CASES
        yield ('power-audit', AUDIT_ARGV, AUDIT, {})

    def test_python_against_python_is_identical(self):
        for name, argv, case, setup in self.cases():
            with self.subTest(name):
                side = served(python_side, setup)
                plain = {k: v for k, v in options(case).items() if k != 'ported'}
                self.assertEqual(compare(self.fixture, argv, left=side, right=side, **plain), [])

    def test_cases_stay_inside_the_fixture_root(self):
        for name, argv, case, setup in self.cases():
            if '{root}' in ' '.join(argv) or setup.get('slow'):
                continue
            with self.subTest(name):
                case = options(case)
                found = host_access(self.fixture, argv, env=case['env'], files=case.get('files'))
                # A traceback's source lookup of main.py's code (run by runpy) is no device access.
                self.assertEqual([e for e in found if not e['target'].endswith('/<unknown>')], [])

    def test_python_results_are_the_contract(self):
        results = {name: run_case(served(python_side, setup), self.fixture, argv, **options(case))
                   for name, argv, case, setup in self.cases() if not setup.get('slow')}
        slept = results['product-sleep']
        self.assertEqual(slept['exit'], 0)
        receipt = slept['tree'][suspend.SLEEP]['content']
        self.assertEqual(receipt['product']['state'], 'restored')
        self.assertEqual(slept['tree'][suspend.GUARD]['content']['pending'], False)
        self.assertEqual(slept['tree']['/sys/power/state']['content'], ['mem'])
        self.assertEqual(slept['tree']['/sys/firmware/y2_pm/backstop_s']['content'], 0)  # "0\n" reads as JSON
        self.assertEqual(slept['tree']['/sys/firmware/y2_pm/stage']['content'], ['COMPLETE'])
        self.assertEqual([c['argv'] for c in slept['calls']], suspend.RADIOS[:3])
        refused = results['product-sleep-kernel-prepare-refused']
        self.assertEqual(refused['exit'], 2)
        self.assertIn('/run/y2/product-sleep-retry.json', refused['tree'])
        # F47: a refused sleep restores dark.
        requests = refused['tree']['/run/reborn/requests.jsonl']['content']
        self.assertEqual(json.loads(requests[-1])['command'], {'op': 'sleep_restore', 'dark': True})
        failed = results['product-sleep-kernel-error']
        self.assertEqual((failed['exit'], failed['tree'][suspend.GUARD]['content']['pending']), (1, True))
        self.assertEqual(results['sleep-request-busy']['stdout']['reason'], 'sleep_operation_in_progress')
        self.assertEqual(results['product-sleep-activity-busy']['tree'][suspend.SLEEP]['content']['product']['reason'],
                         'activity_operation')
        self.assertEqual(results['power-command-flags']['exit'], 2)
        self.assertEqual(results['power-audit']['exit'], 0)
        self.assertEqual(results['power-audit']['tree']['/audit.json']['content']['schema'],
                         'org.y2linux.power-audit/v2')

    @unittest.skipUnless(rust_binary(), 'Y2_PLATFORM_RUST (host build of the Rust y2-platform) not given')
    def test_rust_matches_python(self):
        binary = rust_binary()
        hidden = hidden_supported(binary)
        rust = rust_side(binary)
        for name, argv, case, setup in self.cases():
            with self.subTest(name):
                if argv[0].startswith('__') and not hidden:
                    self.skipTest('main.rs does not run the hidden programmes yet (commands::suspend::programme)')
                found = compare(self.fixture, argv, left=served(python_side, setup), right=served(rust, setup),
                                **options(case))
                self.assertEqual(found, [])

    @unittest.skipUnless(rust_binary(), 'Y2_PLATFORM_RUST (host build of the Rust y2-platform) not given')
    def test_rust_answers_to_the_script_names(self):
        binary = rust_binary()
        if not hidden_supported(binary):
            self.skipTest('main.rs does not run commands::suspend::program (argv[0]) yet')
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
