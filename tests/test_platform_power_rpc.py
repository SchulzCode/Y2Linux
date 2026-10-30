"""Power group goldens the plain run of tests/test_platform_differential.py cannot give (tests/platform_cases/power.py),
run by the Rust host build (`Y2_PLATFORM_RUST`) and compared with tests/platform_goldens/power/<case>.json:

- EXTRA_CASES: odd input the Python daemon ended on (an uncaught exception); the daemon now keeps running.
- The power socket conversation (power.CONVERSATION): a live `power-daemon` on a fixture root, `y2-platform
  shutdown` clients and Reborn-style acknowledgements, the intent executed below the fixture root (every step's
  command gets the fixture answer `fixture`, no process is signalled), SIGTERM ending the daemon. The daemon serves
  only root peers, so the conversation runs in a user namespace (`unshare -r`); skipped without one.
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
from platform_golden import (SKIP, TRAIL_CLOCK, VOLATILE_KEYS, base_environment, check, check_all,  # noqa: E402
                             normalise_json, read_golden, run_case, rust_binary, snapshot, touched, tree_difference)
from platform_cases import power  # noqa: E402
import test_platform_differential  # noqa: E402

VOLATILE = VOLATILE_KEYS | {'wall_time', 'collector_cpu_seconds', 'requested_monotonic_ns', 'deadline_monotonic_ns',
                            'pid', 'start_ticks'}


def user_namespace():
    try:
        return subprocess.run(['unshare', '-r', 'true'], capture_output=True, timeout=10).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def command(binary, root, argv, scratch):
    """argv and environment of one platform run on the conversation's root."""
    return ([binary, *argv], base_environment({
        'Y2_ROOT': str(root), 'Y2_FIXTURE_COMMANDS': str(scratch / 'commands.json'),
        'Y2_FIXTURE_CALLS': str(scratch / 'calls.jsonl')}))


def raw(root, request):
    """One request on the socket as Reborn sends it (power.rs: compact JSON and a newline)."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(10)
        client.connect(str(root / 'run/y2/power.sock'))
        client.sendall(request if isinstance(request, bytes) else (json.dumps(request) + '\n').encode())
        answer = b''
        while b'\n' not in answer:
            part = client.recv(4096)
            if not part:
                break
            answer += part
    return json.loads(answer) if answer else None


def wait_for(condition, seconds=30):
    deadline = time.monotonic() + seconds
    while not condition():
        if time.monotonic() > deadline:
            raise TimeoutError('condition not reached')
        time.sleep(0.05)


def conversation(binary, fixture, scratch):
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
    patterns = [(TRAIL_CLOCK, '<clock> '), power.UUID]
    before = snapshot(root, VOLATILE, patterns)
    argv, env = command(binary, root, ['power-daemon'], scratch)
    daemon = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    results = []
    try:
        wait_for(lambda: (root / 'run/y2/power.sock').exists() and (root / 'run/y2/power.json').exists())

        def client(*argv):
            line, environment = command(binary, root, ['shutdown', *argv], scratch)
            done = subprocess.run(line, env=environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=60)
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
                            'tree': tree_difference(before, snapshot(root, VOLATILE, patterns))}, VOLATILE)
    text = re.sub(power.UUID[0], power.UUID[1], json.dumps(found).replace(str(root), '<root>'))
    return json.loads(re.sub(r'\b%d\b' % pid, '4242', text))  # the acknowledging process' pid


def run_conversation(fixture):
    with tempfile.TemporaryDirectory(prefix='y2-rpc-') as scratch:
        done = subprocess.run(['unshare', '-r', sys.executable, '-I', __file__, 'conversation', str(rust_binary()),
                               str(fixture), scratch], capture_output=True, timeout=240)
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

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [(('power', name), lambda argv=argv, case=case: run_case(self.fixture, argv, **case))
                         for name, argv, case in power.EXTRA_CASES])

    def test_odd_input_never_ends_the_daemon(self):
        for name, _, _ in power.EXTRA_CASES:
            with self.subTest(name):
                result = read_golden('power', name)
                self.assertEqual((result['exit'], result['stdout']), (0, []))
                self.assertNotIn('/run/y2/power.sock', result['tree']['added'])  # serve() ended normally
                self.assertIn('/run/y2/power.json', result['tree']['added'])
                if name == 'power-daemon-receipt-pairs':
                    self.assertNotIn('/data/system/platform/power-journal.jsonl', result['tree']['added'])
                if name == 'power-daemon-resume-bad-ack':
                    self.assertNotIn('/dev/pmsg0', touched(result))  # not due before its deadline
                if name == 'power-daemon-resume-bad-ack-due':
                    # Unacknowledged: every step ran, the init request (no fixture answer) failed.
                    trail = touched(result)['/dev/pmsg0']['content']
                    self.assertIn('<clock> shutdown S90reborn fixture', trail)
                    self.assertEqual(trail[-1], '<clock> shutdown init_poweroff fixture')
                    intent = touched(result)['/run/y2/shutdown.json']['content']
                    self.assertEqual((intent['state'], intent['acknowledgement']), ('Failed', ['Ready']))
                    self.assertIn('backlight_early', intent['stop_results'])


class PowerConversation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = test_platform_differential.player_root(Path(cls.directory.name) / 'fixture', ('power',))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_the_conversation_golden_covers_each_answer(self):
        golden = read_golden('power', power.CONVERSATION)
        results = golden['results']
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
                         ['stale_shutdown_ack', 'invalid_ack_outcome', 'missing_id', 'invalid_request',
                          'invalid_shutdown_request', 'invalid_power_request', 'invalid_shutdown_request', None])
        self.assertEqual(answers[-1]['result']['acknowledgement']['outcome'], 'Ready')
        failed = results[-2]
        self.assertEqual((failed['exit'], failed['stdout']['state'], failed['stdout']['failure']),
                         (0, 'Failed', 'init_shutdown_request_failed'))
        self.assertEqual(results[-1], {'daemon_exit': 0, 'daemon_stdout': ''})
        trail = golden['tree']['changed']['/dev/pmsg0']['content']
        self.assertEqual(trail[0], '<clock> shutdown execute reboot update')
        self.assertEqual(trail[-1], '<clock> shutdown init_reboot fixture')

    @unittest.skipUnless(rust_binary(), SKIP)
    @unittest.skipUnless(user_namespace(), 'no user namespace (unshare -r): the daemon serves only root peers')
    def test_daemon_and_clients_match_the_golden(self):
        check(self, 'power', power.CONVERSATION, run_conversation(self.fixture))


if __name__ == '__main__':
    if sys.argv[1:2] == ['conversation']:
        binary, fixture, scratch = sys.argv[2:5]
        print(json.dumps(conversation(binary, Path(fixture), Path(scratch))))
    else:
        unittest.main()
