"""Differential harness for the platform port: Python against Python now, Python against the Rust host build
(`Y2_PLATFORM_RUST`) once it exists. See tests/platform_differential.py and PLATFORM-RUST-PORT-PLAN.md."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_differential import (REPOSITORY, compare, differences, host_access, python_side, run_case,
                                   rust_binary, rust_side, RUST_ENV)

UUID = (r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', '<uuid>')
EPOCH = 'b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d'
LEASE = {'interface': 'wlan0', 'Y2_DHCP_GENERATION': EPOCH, 'ip': '192.168.1.40', 'subnet': '24',
         'router': '192.168.1.1', 'dns': '192.168.1.1', 'lease': '86400'}
IP_OK = [{'argv': argv, 'ok': True, 'reason': None, 'output': ''} for argv in (
    ['ip', '-4', 'address', 'flush', 'dev', 'wlan0', 'scope', 'global'],
    ['ip', '-4', 'route', 'flush', 'dev', 'wlan0'],
    ['ip', 'link', 'set', 'dev', 'wlan0', 'up'],
    ['ip', '-4', 'address', 'add', '192.168.1.40/24', 'dev', 'wlan0'],
    ['ip', '-4', 'route', 'replace', 'default', 'via', '192.168.1.1', 'dev', 'wlan0', 'metric', '100'])]

# (name, argv, options for run_case). Read-only commands, state writers, failures and argument errors.
CASES = [
    ('capabilities', ['capabilities'], {}),
    ('health', ['health'], {}),
    ('status', ['status'], {}),
    ('status-wifi', ['status', 'wifi'], {}),
    ('sleep-status', ['sleep', 'status'], {}),
    ('time', ['time'], {'volatile': {'epoch'}}),
    ('update-status', ['update', 'status'], {}),
    ('report-status', ['report', 'status'], {}),
    ('media-status', ['media', 'status'], {}),
    ('boot-stage', ['boot-stage', 'platform_start'], {}),
    ('boot-stage-unknown', ['boot-stage', 'no_such_stage'], {}),
    ('wifi-event', ['wifi-event', 'CONNECTED'], {'patterns': [UUID], 'volatile': {'link_counts_since_s'}}),
    ('dhcp-bound', ['dhcp-event', 'bound'], {'env': LEASE, 'commands': IP_OK}),
    ('dhcp-leasefail', ['dhcp-event', 'leasefail'], {'env': LEASE}),
    ('dhcp-foreign-interface', ['dhcp-event', 'bound'], {'env': {**LEASE, 'interface': 'usb0'}}),
    ('argument-error', ['sleep', 'no_such_action'], {}),
]


def player_root(root):
    """A small player-shaped tree: identity, verified /data, a Wi-Fi epoch, nothing mounted on the SD slot."""
    def put(name, content, mode=0o644):
        path = root / name.lstrip('/')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        path.chmod(mode)
    put('/proc/sys/kernel/random/boot_id', 'boot-a\n')
    put('/proc/sys/kernel/osrelease', '6.18.0-fixture\n')
    put('/proc/sys/kernel/tainted', '0\n')
    put('/proc/uptime', '2730.79 4378.47\n')
    put('/proc/stat', 'cpu  100 0 50 1000 5 0 1 0 0 0\ncpu0 100 0 50 1000 5 0 1 0 0 0\n')
    put('/proc/meminfo', 'MemTotal: 245760 kB\nMemFree: 120000 kB\nMemAvailable: 160000 kB\n')
    put('/proc/self/mountinfo', '22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
                                '30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n')
    put('/run/y2-data-device', '/dev/mmcblk0p9\n')
    put('/data/.y2data-schema', '1\n')
    put('/data/network/enabled', '0\n')
    put('/data/bluetooth/enabled', '0\n')
    put('/run/y2/wifi-epoch', EPOCH + '\n', 0o600)
    (root / 'run/y2').chmod(0o700)
    put('/etc/y2linux/versions.json', json.dumps({
        'release_version': '1.0.0-fixture', 'build_git_commit': 'linux-commit', 'reborn_source_commit': 'reborn',
        'rootfs_version': 'v1.0', 'platform_api_version': 1, 'feature_contract_version': 2}))
    put('/etc/y2linux/build-id', 'fixture-build\n')
    put('/etc/y2linux/platform-contract', 'y2-platform-v1\n')
    shutil.copy(REPOSITORY / 'tools/platform/capabilities.json', root / 'etc/y2linux/capabilities.json')
    put('/sys/class/power_supply/BAT0/type', 'Battery\n')
    put('/sys/class/power_supply/BAT0/status', 'Charging\n')
    put('/sys/class/power_supply/BAT0/voltage_now', '4149316\n')
    put('/sys/class/power_supply/y2-usb-presence/type', 'USB\n')
    put('/sys/class/power_supply/y2-usb-presence/online', '1\n')
    return root


class PlatformDifferential(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = player_root(Path(cls.directory.name) / 'fixture')

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_python_against_python_is_identical(self):
        # Two runs of one implementation on two copies must agree, or the harness
        # would report noise (clock readings, root paths, random names) as differences.
        for name, argv, options in CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, **options), [])

    def test_cases_stay_inside_the_fixture_root(self):
        # A path the Python side opens without ctx.path, or a process it starts without
        # ctx.command, would make both sides read the host and agree by accident.
        for name, argv, options in CASES:
            with self.subTest(name):
                self.assertEqual(host_access(self.fixture, argv, env=options.get('env')), [])

    def test_cases_exercise_exit_codes_files_and_subprocesses(self):
        results = {name: run_case(python_side, self.fixture, argv, **options) for name, argv, options in CASES}
        self.assertEqual(results['capabilities']['exit'], 0)
        self.assertEqual(results['capabilities']['stdout']['schema'], 'org.y2linux.capabilities/v1')
        self.assertEqual(results['boot-stage-unknown']['exit'], 1)
        self.assertEqual(results['boot-stage-unknown']['stdout']['reason'], 'unknown_boot_stage')
        self.assertEqual(results['argument-error']['exit'], 2)
        self.assertEqual(results['dhcp-foreign-interface']['stdout']['reason'], 'dhcp_interface_not_owned')
        tree = results['boot-stage']['tree']
        self.assertEqual(tree['/data/system/platform/boot.json']['mode'], '0o600')
        self.assertIn('/data/system/platform/.boot.lock', tree)
        self.assertEqual(tree['/run/y2/boot-stages.jsonl']['content'],
                         {'stage': 'platform_start', 'monotonic_s': '<volatile>'})
        bound = results['dhcp-bound']
        self.assertEqual(bound['stdout']['state'], 'Authenticated')
        self.assertEqual([c['argv'] for c in bound['calls']], [c['argv'] for c in IP_OK])
        self.assertEqual(bound['tree']['/run/y2/resolv.conf']['content'],
                         ['nameserver 192.168.1.1', 'options timeout:2 attempts:1'])
        self.assertEqual(results['wifi-event']['tree']['/run/y2/wifi-epoch']['content'], ['<uuid>'])

    def test_harness_reports_each_kind_of_difference(self):
        def altered(change):
            def side(root, argv, commands_file, calls_file, timeout, env):
                result = python_side(root, argv, commands_file, calls_file, timeout, env)
                return change(root, result) or result
            return side

        def extra_file(root, result):
            (root / 'run/y2/unexpected.json').write_text('{}')

        def loose_mode(root, result):
            (root / 'data/system/platform/boot.json').chmod(0o644)

        def other_exit(root, result):
            result.returncode = 3

        def other_call(root, result):
            with open(root.parent / 'calls.jsonl', 'a') as calls:
                calls.write(json.dumps({'argv': ['true'], 'timeout': 1}) + '\n')

        argv = ['boot-stage', 'platform_start']
        for change, where in [(extra_file, '/tree//run/y2/unexpected.json: only in right'),
                              (loose_mode, "/tree//data/system/platform/boot.json/mode: '0o600' != '0o644'"),
                              (other_exit, '/exit: 0 != 3'),
                              (other_call, '/calls: [] != ')]:
            with self.subTest(change.__name__):
                found = compare(self.fixture, argv, right=altered(change))
                self.assertTrue(found and found[0].startswith(where), found)
        found = differences(run_case(python_side, self.fixture, argv),
                            run_case(python_side, self.fixture, ['boot-stage', 'services_started']))
        self.assertIn("/stdout/last_stage: 'platform_start' != 'services_started'", found)

    def test_contract_inventory_covers_the_cli(self):
        # platform/CONTRACT.md: the argument table is the parser's; every subcommand
        # and action has an inventory entry, so a new one cannot be ported unseen.
        check = subprocess.run([sys.executable, str(REPOSITORY / 'tools/development/platform-contract.py'),
                                '--check'], capture_output=True, text=True)
        self.assertEqual(check.returncode, 0, check.stdout)
        contract = json.loads((REPOSITORY / 'platform/contract.json').read_text())
        for name, arguments in contract['arguments'].items():
            entries = [e for e in contract['entries'] if e['command'] == name]
            with self.subTest(name):
                self.assertTrue(entries)
                actions = [a['choices'] for a in arguments if a['name'] == 'action' and a['choices']]
                if actions:
                    self.assertEqual(set(actions[0]) - {e['action'] for e in entries}, set())

    def test_contract_goldens_are_the_python_output(self):
        # y2p-contract's golden files must stay what the Python platform prints and writes.
        check = subprocess.run([sys.executable, str(REPOSITORY / 'tools/development/platform-goldens.py'),
                                '--check'], capture_output=True, text=True)
        self.assertEqual(check.returncode, 0, check.stdout + check.stderr)

    @unittest.skipUnless(rust_binary(), RUST_ENV + ' (host build of the Rust y2-platform) not given')
    def test_rust_matches_python(self):
        rust = rust_side(rust_binary())
        for name, argv, options in CASES:
            with self.subTest(name):
                self.assertEqual(compare(self.fixture, argv, right=rust, **options), [])


if __name__ == '__main__':
    unittest.main()
