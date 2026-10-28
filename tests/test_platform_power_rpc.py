"""Power group cases the shared differential checks cannot run (tests/platform_cases/power.py):

- EXTRA_CASES: the daemon ending on an exception main.py does not catch (a Python traceback reads source files
  outside the fixture root, which the shared host-access check refuses), Python against Python and against the
  Rust host build (`Y2_PLATFORM_RUST`).
- The power socket conversation: a live `power-daemon` (Python or Rust) on a fixture root, `y2-platform shutdown`
  clients (Python or Rust) and Reborn-style acknowledgements, the intent executed below the fixture root (every
  step's command gets the fixture answer `fixture`, no process is signalled), SIGTERM ending the daemon. The daemon
  serves only root peers, so each conversation runs in a user namespace (`unshare -r`); skipped without one.
  Every combination of daemon and client must give the same answers, records and trail.
"""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_differential import (LAUNCHER, TRAIL_CLOCK, VOLATILE_KEYS, base_environment, compare, differences,  # noqa: E402
                                   normalise_json, python_side, run_case, rust_binary, rust_side, snapshot, RUST_ENV)
from platform_cases import power  # noqa: E402
import test_platform_differential  # noqa: E402

VOLATILE = VOLATILE_KEYS | {'wall_time', 'collector_cpu_seconds', 'requested_monotonic_ns', 'deadline_monotonic_ns',
                            'pid', 'start_ticks'}
PATTERNS = [power.UUID]


def user_namespace():
    try:
        return subprocess.run(['unshare', '-r', 'true'], capture_output=True, timeout=10).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def command(kind, root, argv, scratch):
    """argv and environment of one platform run: 'python' (the launcher) or the Rust binary's path."""
    if kind == 'python':
        return ([sys.executable, '-I', str(LAUNCHER), '--root', str(root), '--commands', str(scratch / 'commands.json'),
                 '--calls', str(scratch / 'calls.jsonl'), '--', *argv], base_environment({}))
    return ([kind, *argv], base_environment({
        'Y2_ROOT': str(root), 'Y2_FIXTURE_COMMANDS': str(scratch / 'commands.json'),
        'Y2_FIXTURE_CALLS': str(scratch / 'calls.jsonl'), 'Y2_PLATFORM_PYTHON': sys.executable,
        'Y2_PLATFORM_MAIN': str(LAUNCHER)}))


def raw(root, request):
    """One request on the socket as Reborn sends it (power.rs: compact JSON and a newline)."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(2)
        client.connect(str(root / 'run/y2/power.sock'))
        client.sendall(request if isinstance(request, bytes) else (json.dumps(request) + '\n').encode())
        answer = b''
        while b'\n' not in answer:
            part = client.recv(4096)
            if not part:
                break
            answer += part
    return json.loads(answer) if answer else None


def wait_for(condition, seconds=15):
    deadline = time.monotonic() + seconds
    while not condition():
        if time.monotonic() > deadline:
            raise TimeoutError('condition not reached')
        time.sleep(0.05)


def conversation(daemon_kind, client_kind, fixture, scratch):
    """Run inside the user namespace (uid 0): the whole exchange, normalised."""
    root = scratch / 'root'
    shutil.copytree(fixture, root, symlinks=True)
    for name, text in power.battery(3850000).items():
        (root / name.lstrip('/')).write_text(text)
    # This process plays Reborn: its pid in /run/reborn/process.pid, its exe is the fixture's /usr/bin/reborn.
    pid = os.getpid()
    (root / 'run/reborn').mkdir(parents=True, exist_ok=True)
    (root / 'run/reborn/process.pid').write_text('%d\n' % pid)
    (root / 'proc' / str(pid)).mkdir(parents=True)
    (root / 'proc' / str(pid) / 'exe').write_text('')
    (root / 'proc' / str(pid) / 'stat').write_text('%d (reborn) S 1 %d %d 0 -1 0 0 0 0 0 5 3 0 0 20 0 3 0 777 1 1\n'
                                                   % (pid, pid, pid))
    reborn = root / 'usr/bin/reborn'
    reborn.unlink()
    reborn.symlink_to(os.path.relpath(root / 'proc' / str(pid) / 'exe', reborn.parent))
    (root / 'dev/watchdog').write_text('')
    (scratch / 'commands.json').write_text(json.dumps(power.answers(['/bin/busybox', 'sync'])))
    (scratch / 'calls.jsonl').touch()
    argv, env = command(daemon_kind, root, ['power-daemon'], scratch)
    daemon = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    results = []
    try:
        wait_for(lambda: (root / 'run/y2/power.sock').exists() and (root / 'run/y2/power.json').exists())

        def client(*argv):
            line, environment = command(client_kind, root, ['shutdown', *argv], scratch)
            done = subprocess.run(line, env=environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
            text = done.stdout.decode()
            results.append({'client': list(argv), 'exit': done.returncode, 'stdout': json.loads(text) if text else None,
                            'order': list(json.loads(text)) if text.startswith('{') else None})
            return results[-1]['stdout']

        intent = client('reboot', '--reason', 'update')
        client('poweroff')
        client('reboot')
        for request in ({'operation': 'ack', 'id': 'other', 'outcome': 'Ready'},
                        {'operation': 'ack', 'id': intent['id'], 'outcome': 'Maybe'},
                        {'operation': 'ack', 'outcome': 'Ready'}, [1], b'{"action": "halt"}\n', b'x' * 3000,
                        {'action': 'reboot', 'reason': None}):
            results.append({'raw': repr(request), 'answer': raw(root, request)})
        results.append({'raw': 'ack Ready', 'answer': raw(root, {'operation': 'ack', 'id': intent['id'],
                                                                 'outcome': 'Ready'})})
        wait_for(lambda: json.loads((root / 'run/y2/shutdown.json').read_text()).get('state') == 'Failed')
        client('reboot', '--reason', 'service')  # a Failed intent is returned, never retried
    finally:
        daemon.send_signal(signal.SIGTERM)
        out, _ = daemon.communicate(timeout=30)
    results.append({'daemon_exit': daemon.returncode, 'daemon_stdout': out.decode()})
    calls = [json.loads(line) for line in (scratch / 'calls.jsonl').read_text().splitlines() if line]
    found = normalise_json({'results': results, 'calls': [[c['argv'], float(c['timeout'])] for c in calls],
                            'tree': snapshot(root, VOLATILE, [(TRAIL_CLOCK, '<clock> '), power.UUID])}, VOLATILE)
    text = re.sub(power.UUID[0], power.UUID[1], json.dumps(found).replace(str(root), '<root>'))
    return json.loads(re.sub(r'\b%d\b' % pid, '4242', text))  # the acknowledging process' pid


def run_conversation(daemon_kind, client_kind, fixture):
    with tempfile.TemporaryDirectory(prefix='y2-rpc-') as scratch:
        done = subprocess.run(['unshare', '-r', sys.executable, '-I', __file__, 'conversation', daemon_kind,
                               client_kind, str(fixture), scratch], capture_output=True, timeout=120)
        if done.returncode:
            raise AssertionError(done.stderr.decode()[-3000:])
        return json.loads(done.stdout.decode().replace(scratch, '<scratch>'))


class PowerExtraCases(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('power',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_python_against_python_is_identical(self):
        for name, argv, case in power.EXTRA_CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, **case), [])

    def test_crashes_end_the_daemon_without_output(self):
        for name, argv, case in power.EXTRA_CASES:
            with self.subTest(name):
                result = run_case(python_side, self.fixture, argv, **case)
                self.assertEqual((result['exit'], result['stdout']), (1, []))
                self.assertIn('/run/y2/power.sock', result['tree'])  # serve() never reached its unlink
                if name == 'power-daemon-resume-bad-ack-due':
                    # The steps before the acknowledgement check ran, fed the watchdog, then the daemon ended.
                    self.assertEqual(result['tree']['/dev/watchdog']['content'], ['.'])
                    self.assertEqual(result['tree']['/dev/pmsg0']['content'][-1],
                                     '<clock> shutdown journal_pending fixture')

    @unittest.skipUnless(rust_binary(), RUST_ENV + ' (host build of the Rust y2-platform) not given')
    def test_rust_matches_python(self):
        rust = rust_side(rust_binary())
        for name, argv, case in power.EXTRA_CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, right=rust, **case), [])


@unittest.skipUnless(user_namespace(), 'no user namespace (unshare -r): the daemon serves only root peers')
class PowerConversation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('power',))
        cls.python = run_conversation('python', 'python', cls.fixture)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_the_python_conversation_covers_each_answer(self):
        results = self.python['results']
        first, refused, same = results[0], results[1], results[2]
        self.assertEqual((first['exit'], first['stdout']['state'], first['stdout']['reason']),
                         (0, 'ShutdownPending', 'update'))
        self.assertEqual(first['order'], ['schema', 'id', 'boot_id', 'state', 'action', 'reason',
                                          'requested_monotonic_ns', 'deadline_monotonic_ns', 'client',
                                          'acknowledgement', 'failure'])
        self.assertEqual(first['stdout']['client'], {'pid': '<volatile>', 'start_ticks': '<volatile>'})
        self.assertEqual((refused['exit'], refused['stdout']['reason']), (1, 'different_shutdown_already_pending'))
        self.assertEqual(same['stdout'], first['stdout'])
        answers = [r['answer'] for r in results if 'raw' in r]
        self.assertEqual([a.get('error') for a in answers],
                         ['stale_shutdown_ack', 'invalid_ack_outcome', "'id'", "'list' object has no attribute 'get'",
                          'invalid_shutdown_request', 'invalid_power_request', 'invalid_shutdown_request', None])
        self.assertEqual(answers[-1]['result']['acknowledgement']['outcome'], 'Ready')
        failed = results[-2]
        self.assertEqual((failed['exit'], failed['stdout']['state'], failed['stdout']['failure']),
                         (0, 'Failed', 'init_shutdown_request_failed'))
        self.assertEqual(results[-1], {'daemon_exit': 0, 'daemon_stdout': ''})
        trail = self.python['tree']['/dev/pmsg0']['content']
        self.assertEqual(trail[0], '<clock> shutdown execute reboot update')
        self.assertEqual(trail[-1], '<clock> shutdown init_reboot fixture')

    @unittest.skipUnless(rust_binary(), RUST_ENV + ' (host build of the Rust y2-platform) not given')
    def test_rust_daemon_and_clients_match_python(self):
        rust = str(rust_binary())
        for daemon, client in ((rust, rust), (rust, 'python'), ('python', rust)):
            with self.subTest(daemon=daemon, client=client):
                self.assertEqual(differences(self.python, run_conversation(daemon, client, self.fixture)), [])


if __name__ == '__main__':
    if sys.argv[1:2] == ['conversation']:
        daemon_kind, client_kind, fixture, scratch = sys.argv[2:6]
        print(json.dumps(conversation(daemon_kind, client_kind, Path(fixture), Path(scratch))))
    else:
        unittest.main()
