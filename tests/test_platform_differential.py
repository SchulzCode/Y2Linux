"""Golden cases of the Rust platform: the base cases below and every group's CASES (tests/platform_cases), each
run by the Rust host build (`Y2_PLATFORM_RUST`) on the player-shaped fixture root and compared with
tests/platform_goldens/<base|group>/<case>.json (tests/platform_golden.py; `Y2_RECORD_GOLDENS=1` re-records).
Also: every golden has a case, the harness reports each kind of difference, and the contract inventory and the
y2p-contract goldens agree with the parser's table and these goldens."""
from collections import defaultdict
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from platform_golden import (REPOSITORY, SKIP, check, check_all, differences, prune, read_golden, recording,  # noqa: E402
                             run_case, rust_binary, touched)

UUID = (r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', '<uuid>')
EPOCH = 'b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d'
LEASE = {'interface': 'wlan0', 'Y2_DHCP_GENERATION': EPOCH, 'ip': '192.168.1.40', 'subnet': '255.255.255.0',
         'router': '192.168.1.1', 'dns': '192.168.1.1 9.9.9.9', 'lease': '86400'}
FLUSH = [['ip', '-4', 'address', 'flush', 'dev', 'wlan0', 'scope', 'global'], ['ip', '-4', 'route', 'flush', 'dev', 'wlan0']]
LINK = [['ip', 'link', 'set', 'dev', 'wlan0', 'up'], ['ip', '-4', 'address', 'add', '192.168.1.40/24', 'dev', 'wlan0']]
# The record `dhcp-event bound` publishes for LEASE, and its DNS probe result.
SAME_LEASE = json.dumps({'address': '192.168.1.40', 'boot_id': 'boot-a', 'dns_servers': ['192.168.1.1', '9.9.9.9'],
                         'epoch': EPOCH, 'error': None, 'lease_seconds': '86400', 'monotonic_s': 1.0,
                         'prefix_length': 24, 'routes': [['default', '192.168.1.1']], 'schema': 1,
                         'state': 'Authenticated'}, separators=(',', ':'))
SAME_DNS = json.dumps({'address': '192.168.1.40', 'boot_id': 'boot-a', 'epoch': EPOCH, 'ok': True})


def answers(*argvs, ok=True):
    return [{'argv': argv, 'ok': ok, 'reason': None if ok else 'command_failed', 'output': ''} for argv in argvs]


IP_OK = answers(*FLUSH, *LINK, ['ip', '-4', 'route', 'replace', 'default', 'via', '192.168.1.1', 'dev', 'wlan0',
                                'metric', '100'])
STATIC_OK = answers(*FLUSH, *LINK, ['ip', '-4', 'route', 'replace', '10.0.0.0/8', 'via', '192.168.1.2', 'dev', 'wlan0',
                                    'metric', '100'],
                    ['ip', '-4', 'route', 'replace', '0.0.0.0/0', 'dev', 'wlan0', 'metric', '100'])


def anchor(source='ntp', boot='boot-a', offset=0.0):
    """/run/y2/time.json as ntp-event writes it, made when the case runs (coherent with the clocks then)."""
    return lambda: json.dumps({'schema': 1, 'boot_id': boot, 'epoch': time.time() + offset,
                               'monotonic_s': time.monotonic(), 'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME),
                               'source': source}, sort_keys=True, separators=(',', ':'))


POLICY = '/data/bluetooth/codec-policy.json'
SLEEP = '/data/system/platform/suspend-last.json'
CPU = '/sys/devices/system/cpu/cpu0/cpuidle/state2'
WIFI = {'patterns': [UUID]}
# (name, argv, options for run_case).
CASES = [
    ('capabilities', ['capabilities'], {}),
    ('capabilities-json', ['capabilities', '--json'], {}),
    ('capabilities-help', ['capabilities', '--help'], {}),
    ('capabilities-no-template', ['capabilities'], {'files': {'/etc/y2linux/capabilities.json': None}}),
    ('capabilities-experimental', ['capabilities'], {'files': {
        POLICY: json.dumps({'schema': 1, 'experimental': True, 'sbc_quality': 'xq+'}),
        '/data/network/enabled': '1\n', '/run/y2/wifi-power.json': '{"enabled": true}',
        '/sys/module/cpu_dvfs/parameters/qualification_max_khz': '1300000\n'}}),
    ('capabilities-bad-policy', ['capabilities'], {'files': {POLICY: '{"schema": 2}'}}),
    ('capabilities-broken-template', ['capabilities'], {'files': {
        '/etc/y2linux/capabilities.json': '{"schema": "org.y2linux.capabilities/v1", "capabilities": {"wifi": 3}}'}}),
    # Templates and inventories that are refused: invalid_capabilities_template, invalid_codec_inventory, or the
    # codec policy's own refusal first.
    *[('capabilities-template-' + name, ['capabilities'], {'files': {
        '/etc/y2linux/capabilities.json': json.dumps({'capabilities': caps}), **extra}}) for name, caps, extra in [
        ('null', None, {}), ('list-wifi', ['wifi'], {}), ('list', ['dvfs'], {}), ('text', 'bluetooth', {}),
        ('text-other', 'dvfs', {}), ('list-bad-policy', [], {POLICY: '{"schema": 2}'}),
        ('null-entry', {'wifi': None}, {}), ('codec-entry', {'bluetooth_aac': 3}, {}),
        ('codec-entry-list', {'bluetooth_sbc': [1]}, {}), ('codec-entry-false', {'bluetooth_ldac': False}, {}),
        ('xq-null', {'bluetooth_sbc_xq': None}, {}), ('dvfs-text', {'dvfs': 'on'}, {}),
        ('codecs-list', {'bluetooth_aac': {}}, {'/etc/y2linux/bluetooth-codecs.json': '{"schema": 1, "codecs": []}'}),
        ('codec-null', {'bluetooth_aac': {}}, {'/etc/y2linux/bluetooth-codecs.json':
                                               '{"schema": 1, "codecs": {"AAC": null}}'})]],
    ('health', ['health'], {}),
    ('status', ['status'], {}),
    ('status-wifi', ['status', 'wifi'], {}),
    ('sleep-status', ['sleep', 'status'], {}),
    ('sleep-status-none', ['sleep', 'status'], {'files': {SLEEP: None}}),
    ('sleep-status-other-boot', ['sleep', 'status'], {'files': {SLEEP: json.dumps(
        {'product': {'state': 'sleeping', 'request_boot_id': 'boot-old', 'reason': None, 'kernel_completed': True}})}}),
    ('sleep-status-refused-before', ['sleep', 'status'], {'files': {SLEEP: json.dumps(
        {'product': {'state': 'refused', 'request_boot_id': 'boot-old', 'reason': 'usb'}})}}),
    ('sleep-status-list', ['sleep', 'status'], {'files': {SLEEP: '{"product": [["state", "idle"]]}'}}),
    ('time', ['time'], {'files': {'/run/y2/time.json': anchor()}}),
    ('time-no-anchor', ['time'], {}),
    ('time-other-boot', ['time'], {'files': {'/run/y2/time.json': anchor(boot='x')}}),
    ('time-remember', ['time-remember'], {'files': {'/run/y2/time.json': anchor()}}),
    ('time-remember-manual', ['time-remember'], {
                                                 'files': {'/run/y2/time.json': anchor('manual')}}),
    ('time-remember-unchanged', ['time-remember'], {'files': {'/run/y2/time.json': anchor('build_floor')}}),
    ('time-remember-drifted', ['time-remember'], {'files': {'/run/y2/time.json': anchor(offset=-600)}}),
    ('ntp-step', ['ntp-event', 'step'], {'env': {'stratum': '2'},
                                         'commands': answers(['/bin/busybox', 'hwclock', '-w', '-u'])}),
    ('ntp-periodic-recent', ['ntp-event', 'periodic'], {'env': {'stratum': '3'},
                                                        'files': {'/data/system/platform/time-last.json': anchor()}}),
    ('ntp-unsync', ['ntp-event', 'unsync'], {'env': {'stratum': '2'}}),
    ('ntp-stratum-16', ['ntp-event', 'stratum'], {}),
    ('ntp-bad-stratum', ['ntp-event', 'step'], {'env': {'stratum': 'x'}}),
    # int() of the stratum as Python reads it: whitespace, other decimal digits, underscores, any size.
    *[('ntp-stratum-' + name, ['ntp-event', 'step'], {'env': {'stratum': text}, 'commands': answers(
        ['/bin/busybox', 'hwclock', '-w', '-u'])}) for name, text in [
        ('spaces', ' 3\n'), ('arabic', '٣'), ('underscore', '1_0'), ('huge', '9' * 30), ('negative', '-1'),
        ('empty', ''), ('double-underscore', '1__0'), ('plus', '+2'), ('float', '2.0'),
        ('too-long', '1' * 4301), ('too-long-bad', '1' * 4301 + 'x'), ('quote', "x'")]],
    ('boot-stage', ['boot-stage', 'platform_start'], {}),
    ('boot-stage-same-boot', ['boot-stage', 'services_started'], {'files': {
        '/data/system/platform/boot.json': json.dumps({'schema': 1, 'boot_id': 'boot-a', 'boot_count': 7,
                                                       'consecutive_unclean': True, 'previous_boot_id': 'boot-z',
                                                       'history': []})}}),
    ('boot-stage-ready', ['boot-stage', 'application_ready'], {}),
    ('boot-stage-ready-override', ['boot-stage', 'application_ready'], {'files': {
        '/data/system/platform/power-features.json': json.dumps({'dis_pdn': False, 'golden_apply': 3, 'md1_gate': 7,
                                                                 'pmic_sleep': 99, 'panel_power_down': True}),
        '/sys/firmware/y2_boot/metadata': 'lcm=0-st7701\n', '/data/system/platform/panel.json': '{"panel": "gc9503v"}'}}),
    ('boot-stage-shutdown', ['boot-stage', 'shutdown_complete'], {}),
    ('boot-stage-unknown', ['boot-stage', 'no_such_stage'], {}),
    ('boot-stage-option', ['boot-stage', '-x'], {}),
    ('boot-stage-no-data', ['boot-stage', 'platform_start'], {'files': {'/data/.y2data-schema': None}}),
    ('boot-stage-wrong-mount', ['boot-stage', 'platform_start'], {'files': {'/run/y2-data-device': '/dev/sda1'}}),
    ('codec-runtime', ['codec-runtime'], {}),
    ('codec-runtime-record', ['codec-runtime', '--record'], {}),
    ('codec-runtime-experimental', ['codec-runtime'], {'files': {POLICY: json.dumps(
        {'schema': 1, 'experimental': True, 'aac_quality': 'standard', 'ldac_abr': False, 'sbc_quality': 'xq'})}}),
    ('codec-runtime-bad-quality', ['codec-runtime'], {'files': {POLICY: '{"schema": 1, "ldac_quality": "max"}'}}),
    ('codec-runtime-no-inventory', ['codec-runtime'], {'files': {'/etc/y2linux/bluetooth-codecs.json': None}}),
    *[('codec-runtime-inventory-' + name, ['codec-runtime', '--record'], {'files': {
        '/etc/y2linux/bluetooth-codecs.json': json.dumps({'schema': 1, 'codecs': codecs})}}) for name, codecs in [
        ('null', None), ('text', 'AAC'), ('aac-null', {'AAC': None}), ('ldac-list', {'AAC': {}, 'LDAC': [1]})]],
    ('wifi-event', ['wifi-event', 'CONNECTED'], {**WIFI, 'commands': answers(*FLUSH)}),
    ('wifi-event-reconnect', ['wifi-event', 'CONNECTED'], {**WIFI, 'files': {'/run/y2/wifi-events.json': json.dumps(
        {'boot_id': 'boot-a', 'last_event': 'DISCONNECTED', 'connected': 2, 'reconnects': True, 'association_active': False})}}),
    ('wifi-event-disconnected', ['wifi-event', 'DISCONNECTED'], WIFI),
    ('wifi-event-asleep', ['wifi-event', 'DISCONNECTED'], {**WIFI, 'files': {'/sys/class/rfkill/rfkill0/soft': '1\n'}}),
    ('wifi-event-restart', ['wifi-event', 'SERVICE_RESTART'], {**WIFI, 'files': {'/run/y2/wifi-events.json': '{"boot_id": "old"}'}}),
    ('dhcp-bound', ['dhcp-event', 'bound'], {'env': LEASE, 'commands': IP_OK}),
    ('dhcp-renew-prefix', ['dhcp-event', 'renew'], {'env': {**LEASE, 'subnet': '24', 'dns': ''},
                                                    'commands': IP_OK}),
    # A renew of the lease wlan0 carries runs no ip command and keeps resolv.conf and dns.json; another
    # route (or prefix, address, DNS server) configures the lease again.
    *[('dhcp-renew-' + name, ['dhcp-event', 'renew'], {'env': {**LEASE, **env, 'lease': '43200'}, 'commands': ok,
                                                         'files': {'/run/y2/dhcp.json': SAME_LEASE,
                                                                   '/run/y2/dns.json': SAME_DNS}})
      for name, env, ok in [('same', {}, IP_OK), ('other-route', {'router': '192.168.1.254'}, answers(
          *FLUSH, *LINK, ['ip', '-4', 'route', 'replace', 'default', 'via', '192.168.1.254', 'dev', 'wlan0',
                          'metric', '100']))]],
    ('dhcp-bound-static', ['dhcp-event', 'bound'], {'commands': STATIC_OK, 'env': {
        **LEASE, 'staticroutes': '10.1.2.3/8 192.168.1.2 0.0.0.0/0 0.0.0.0'}}),
    ('dhcp-bound-odd-routes', ['dhcp-event', 'bound'], {'env': {**LEASE, 'staticroutes': '10.0.0.0/8'}}),
    ('dhcp-bound-fails', ['dhcp-event', 'bound'], {'env': LEASE, 'commands': answers(*FLUSH)}),
    ('dhcp-bound-loopback', ['dhcp-event', 'bound'], {'env': {**LEASE, 'ip': '127.0.0.5'}}),
    ('dhcp-bound-bad-address', ['dhcp-event', 'bound'], {'env': {**LEASE, 'ip': '192.168.1'}}),
    # ipaddress's own errors (AddressValueError, NetmaskValueError) and the forms it also takes.
    *[('dhcp-bound-' + name, ['dhcp-event', 'bound'], {'env': {**LEASE, **env}, 'commands': IP_OK})
      for name, env in [
        ('leading-zero', {'ip': '192.168.001.1'}), ('no-ip', {'ip': ''}), ('octet', {'ip': '192.168.1.256'}),
        ('slash', {'ip': '192.168.1.40/24'}), ('letters', {'ip': '192.168.1.x'}), ('long-octet', {'ip': '1.2.3.0040'}),
        ('hostmask', {'subnet': '0.0.0.255'}), ('mixed-mask', {'subnet': '255.0.255.0'}),
        ('mask-slash', {'subnet': '24/8'}), ('mask-33', {'subnet': '33'}), ('mask-zero-padded', {'subnet': '024'}),
        ('no-subnet', {'subnet': ''}), ('bad-dns', {'dns': '9.9.9.9 1.1.1'}), ('bad-router', {'router': '192.168.1.1/24'}),
        ('bad-route', {'staticroutes': '10.0.0.0/33 192.168.1.2'}), ('bad-gateway', {'staticroutes': '10.0.0.0/8 x'}),
        ('route-slashes', {'staticroutes': '10.0.0.0/8/8 192.168.1.2'}), ('route-hostmask', {
            'staticroutes': '10.1.2.3/0.255.255.255 192.168.1.2'}), ('unicode-space', {'dns': '9.9.9.9\x1f1.1.1.1'})]],
    # Bytes that are not UTF-8: environment values read as surrogate escapes (shown as \\udcXX), an argument
    # is refused by the parser (exit 2).
    ('dhcp-bound-not-utf8', ['dhcp-event', 'bound'], {'env': {**LEASE, 'ip': '192.168.1.\udcff'}}),
    ('dhcp-interface-not-utf8', ['dhcp-event', 'bound'], {'env': {**LEASE, 'interface': 'wlan\udcff'}}),
    ('ntp-stratum-not-utf8', ['ntp-event', 'step'], {'env': {'stratum': '\udce2\udc822'}}),
    ('boot-stage-not-utf8', ['boot-stage', 'platform_\udcff'], {}),
    ('dhcp-leasefail', ['dhcp-event', 'leasefail'], {'env': LEASE}),
    ('dhcp-nak', ['dhcp-event', 'nak'], {'env': LEASE}),
    ('dhcp-deconfig', ['dhcp-event', 'deconfig'], {'env': LEASE}),
    ('dhcp-stale', ['dhcp-event', 'bound'], {'env': {**LEASE, 'Y2_DHCP_GENERATION': 'old'}}),
    ('dhcp-foreign-interface', ['dhcp-event', 'bound'], {'env': {**LEASE, 'interface': 'usb0'}}),
    ('cpu-idle-start', ['cpu-idle-policy', 'start'], {}),
    ('cpu-idle-start-again', ['cpu-idle-policy', 'start'], {'files': {
        '/run/y2/cpu-idle-policy.lock': '', '/run/y2/cpu-idle-policy.json': '{"state": "Enabled"}'}}),
    ('cpu-idle-start-tainted', ['cpu-idle-policy', 'start'], {'files': {
        '/proc/sys/kernel/tainted': '4097\n', '/sys/devices/platform/10006000.power-controller/dormant_preflight':
        'prerequisite_spm=1\n'}}),
    ('cpu-idle-start-owner', ['cpu-idle-policy', 'start'], {'files': {CPU + '/disable': '0\n'}}),
    ('cpu-idle-start-no-sysfs', ['cpu-idle-policy', 'start'], {'files': {
        '/sys/module/spm/parameters/dormant_budget': None}}),
    ('cpu-idle-stop', ['cpu-idle-policy', 'stop'], {}),
    ('argument-error', ['sleep', 'no_such_action'], {}),
]


import importlib  # noqa: E402

GROUPS = [importlib.import_module('platform_cases.' + name)
          for name in importlib.import_module('platform_cases').GROUPS]
# A group's cases run on the base root plus that group's fixture only: two groups may model the same
# device (both storage and observation describe the SD card) in different states.
CASE_GROUP = {case[0]: group.__name__.rsplit('.', 1)[1] for group in GROUPS for case in group.CASES}
CASES += [case for group in GROUPS for case in group.CASES]


def player_root(root, groups=()):
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
        'rootfs_version': 'v1.0', 'platform_api_version': 1, 'feature_contract_version': 2,
        'kernel_version': '6.18.0-fixture'}))
    put('/etc/y2linux/build-id', 'fixture-build\n')
    put('/etc/y2linux/platform-contract', 'y2-platform-v1\n')
    # The image's capabilities template, kept with the cases (an input of the goldens).
    shutil.copy(Path(__file__).resolve().parent / 'platform_cases/capabilities.json',
                root / 'etc/y2linux/capabilities.json')
    put('/sys/class/power_supply/BAT0/type', 'Battery\n')
    put('/sys/class/power_supply/BAT0/status', 'Charging\n')
    put('/sys/class/power_supply/BAT0/voltage_now', '4149316\n')
    put('/sys/class/power_supply/y2-usb-presence/type', 'USB\n')
    put('/sys/class/power_supply/y2-usb-presence/online', '1\n')
    put('/etc/y2linux/build-epoch', '1788000000\n')
    put('/etc/y2linux/rtc-policy.json', json.dumps({'write_enabled': True, 'source': 'fixture'}))
    put('/etc/y2linux/bluetooth-codecs.json', json.dumps({'schema': 1, 'codecs': {
        'SBC': {'compiled_locally': True, 'platform_qualified': True},
        'AAC': {'compiled_locally': True, 'distribution_approved': True, 'platform_qualified': True},
        'LDAC': {'compiled_locally': True, 'owner_private_experiment': True},
        'aptX': {'compiled_locally': False}}}))
    put('/data/system/platform/suspend-last.json', json.dumps({'product': {
        'state': 'restored', 'request_boot_id': 'boot-a', 'reason': None, 'kernel_completed': True,
        'wake_reason': 'power'}}), 0o600)
    put('/data/system/platform/time-last.json', json.dumps({'epoch': 1788000500.5, 'source': 'ntp'}), 0o600)
    put('/data/system/platform/boot.json', json.dumps({
        'schema': 1, 'boot_id': 'boot-z', 'boot_count': 41, 'consecutive_unclean': 2, 'orderly_shutdown': False,
        'last_stage': 'application_ready', 'identity': {'release_version': '1.0.0-old'},
        'history': [{'boot_id': 'boot-%d' % i, 'last_stage': 'shutdown_complete'} for i in range(8)]}), 0o600)
    (root / 'data/system').chmod(0o700)
    (root / 'data/system/platform').chmod(0o700)
    put('/run/y2/wifi-events.json', json.dumps({'boot_id': 'boot-a', 'last_event': 'CONNECTED', 'connected': 1,
                                                'real_disconnects': 0}), 0o600)
    put('/sys/class/rfkill/rfkill0/type', 'wlan\n')
    put('/sys/class/rfkill/rfkill0/soft', '0\n')
    put('/sys/firmware/y2_pm/reset_status', 'valid=1 cause=software_reset raw=0x00000004\n')
    put('/sys/firmware/y2_boot/metadata', 'panel=st7701s lcm=1-st7701\n')
    put('/sys/firmware/y2_boot/lcm', '1-st7701\n')
    put('/dev/pmsg0', '')
    for name in ('spm/parameters/dis_pdn', 'spm/parameters/md1_gate', 'spm/parameters/pwrap_gate',
                 'panel/parameters/stock_suspend', 'panel/parameters/power_down', 'clocks/parameters/deep_idle_audio'):
        put('/sys/module/' + name, 'N\n')
    put('/sys/module/spm/parameters/golden_apply', '0\n')
    put('/sys/module/pwrap/parameters/pmic_sleep', '0\n')
    put('/sys/module/panel/parameters/panel_hint', '0\n')
    put('/sys/module/spm/parameters/dormant_budget', '0\n')
    put(CPU + '/name', 'DORMANT\n')
    put(CPU + '/disable', '1\n')
    put('/sys/devices/system/cpu/cpuidle/current_driver', 'mt6582-idle\n')
    put('/sys/devices/platform/10006000.power-controller/dormant_preflight', ''.join(
        'prerequisite_%s=1\n' % n for n in ('system_running', 'boot_policy', 'spm', 'local_events', 'cirq',
                                            'linux_context', 'timer_context', 'broken')))
    for group in GROUPS:
        if group.__name__.rsplit('.', 1)[1] in groups:
            group.fixture(root)
    return root





def group_of(name):
    """The golden folder of a case of CASES: its group, or `base`."""
    return CASE_GROUP.get(name, 'base')


def inventory():
    """{golden folder: case names}: every case of every golden module (tests/test_platform_*.py)."""
    names = defaultdict(set)
    for name, _, _ in CASES:
        names[group_of(name)].add(name)
    for group in GROUPS:
        folder = group.__name__.rsplit('.', 1)[1]
        for case in [*getattr(group, 'EXTRA_CASES', ()), *getattr(group, 'SERVED_CASES', ())]:
            names[folder].add(case[0])
        names[folder].update(getattr(group, 'GOLDEN_NAMES', ()))
    return names


class PlatformGoldens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = player_root(Path(cls.directory.name) / 'fixture')
        cls.fixtures = {None: cls.fixture}
        for name in sorted(set(CASE_GROUP.values())):
            cls.fixtures[name] = player_root(Path(cls.directory.name) / ('fixture-' + name), (name,))

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_cases_match_their_goldens(self):
        check_all(self, [((group_of(name), name), lambda name=name, argv=argv, options=options: run_case(
            self.fixtures[CASE_GROUP.get(name)], argv, **options)) for name, argv, options in CASES])

    def test_every_golden_has_a_case(self):
        # A removed or renamed case leaves its golden behind (Y2_RECORD_GOLDENS=1 removes it).
        names = inventory()
        self.assertEqual(sum(map(len, names.values())), len({(g, n) for g, ns in names.items() for n in ns}))
        self.assertEqual(prune(names), [])

    def test_goldens_exercise_exit_codes_files_and_subprocesses(self):
        results = {name: read_golden('base', name) for name, _, _ in CASES if name not in CASE_GROUP}
        self.assertEqual(results['capabilities']['exit'], 0)
        self.assertEqual(results['capabilities']['stdout']['schema'], 'org.y2linux.capabilities/v1')
        self.assertEqual(results['boot-stage-unknown']['exit'], 1)
        self.assertEqual(results['boot-stage-unknown']['stdout']['reason'], 'unknown_boot_stage')
        self.assertEqual(results['argument-error']['exit'], 2)
        self.assertEqual(results['dhcp-foreign-interface']['stdout']['reason'], 'dhcp_interface_not_owned')
        tree = touched(results['boot-stage'])
        self.assertEqual(tree['/data/system/platform/boot.json']['mode'], '0o600')
        self.assertIn('/data/system/platform/.boot.lock', results['boot-stage']['tree']['added'])
        self.assertEqual(tree['/run/y2/boot-stages.jsonl']['content'],
                         {'stage': 'platform_start', 'monotonic_s': '<volatile>'})
        bound = results['dhcp-bound']
        self.assertEqual(bound['stdout']['state'], 'Authenticated')
        self.assertEqual([c['argv'] for c in bound['calls']], [c['argv'] for c in IP_OK])
        self.assertEqual(touched(bound)['/run/y2/resolv.conf']['content'],
                         ['nameserver 192.168.1.1', 'nameserver 9.9.9.9', 'options timeout:2 attempts:1'])
        # The epoch is a new uuid (normalised as the fixture's), so only the events record shows the change.
        self.assertIn('/run/y2/wifi-events.json', results['wifi-event']['tree']['changed'])

    @unittest.skipUnless(rust_binary(), SKIP)
    def test_harness_reports_each_kind_of_difference(self):
        def altered(change):
            def wrap(side):
                def run(root, argv, commands_file, calls_file, timeout, env):
                    result = side(root, argv, commands_file, calls_file, timeout, env)
                    return change(root, result) or result
                return run
            return wrap

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
        golden = read_golden('base', 'boot-stage')
        for change, where in [(extra_file, '/tree/added//run/y2/unexpected.json: only in run'),
                              (loose_mode, "/tree/changed//data/system/platform/boot.json/mode: '0o600' != '0o644'"),
                              (other_exit, '/exit: 0 != 3'),
                              (other_call, '/calls: [] != ')]:
            with self.subTest(change.__name__):
                found = differences(golden, run_case(self.fixture, argv, wrap=altered(change)))
                self.assertTrue(found and found[0].startswith(where), found)
        found = differences(golden, run_case(self.fixture, ['boot-stage', 'services_started']))
        self.assertIn("/stdout/last_stage: 'platform_start' != 'services_started'", found)
        if not recording():
            with self.assertRaisesRegex(AssertionError, 'no golden tests/platform_goldens/base/no-such-case.json'):
                check(self, 'base', 'no-such-case', {})

    def test_contract_inventory_covers_the_cli(self):
        # platform/CONTRACT.md: the argument table is the parser's (cli.rs); every subcommand
        # and action has an inventory entry, so a new one cannot be added unseen.
        found = subprocess.run([sys.executable, str(REPOSITORY / 'tools/development/platform-contract.py'),
                                '--check'], capture_output=True, text=True)
        self.assertEqual(found.returncode, 0, found.stdout + found.stderr)
        contract = json.loads((REPOSITORY / 'platform/contract.json').read_text())
        for name, arguments in contract['arguments'].items():
            entries = [e for e in contract['entries'] if e['command'] == name]
            with self.subTest(name):
                self.assertTrue(entries)
                actions = [a['choices'] for a in arguments if a['name'] == 'action' and a['choices']]
                if actions:
                    self.assertEqual(set(actions[0]) - {e['action'] for e in entries}, set())

    def test_contract_goldens_are_the_platform_output(self):
        # y2p-contract's golden files (read by its cargo test) must stay what these goldens record.
        found = subprocess.run([sys.executable, str(REPOSITORY / 'tools/development/platform-goldens.py'),
                                '--check'], capture_output=True, text=True)
        self.assertEqual(found.returncode, 0, found.stdout + found.stderr)


if __name__ == '__main__':
    unittest.main()
