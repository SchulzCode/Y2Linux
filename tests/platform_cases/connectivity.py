"""Differential cases of port group `connectivity` (see tests/platform_cases/__init__.py):
`wifi-power-policy`, `bluetooth-power on|off` and `connectivity-daemon` (ended after Y2_DAEMON_TICKS loop
iterations; `stop_after_ticks` stops the Python daemon at the same point)."""
import json
import time

EPOCH = 'b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d'
BOOT = 'c0ffee00-1111-4222-8333-444455556666'
GET = ['/usr/sbin/iw', 'dev', 'wlan0', 'get', 'power_save']
SET = ['/usr/sbin/iw', 'dev', 'wlan0', 'set', 'power_save']
POWER_SAVE = '/data/network/power-save.json'
WIFI_POWER = '/run/y2/wifi-power.json'
IFINDEX = '/sys/class/net/wlan0/ifindex'
INTENT = [(r'sequence=\d+', 'sequence=<us>'), (r'deadline_us=\d+', 'deadline_us=<us>')]
GLOBAL = ['wpa_cli', '-g', '/run/wpa_supplicant/global']
ADD = GLOBAL + ['interface_add', 'wlan0', '/data/network/wpa_supplicant.conf', 'nl80211', '/run/wpa_supplicant']
AUTOSCAN = ['wpa_cli', '-i', 'wlan0', 'autoscan', 'exponential:2:120']
ACTION = ['wpa_cli', '-i', 'wlan0', '-a', '/usr/libexec/y2/wpa-event', '-B', '-P', '/run/y2/wifi-action.pid']


def answer(argv, output='', ok=True):
    return {'argv': argv, 'ok': ok, 'reason': None if ok else 'command_failed', 'output': output if ok else None}


def readback(state):
    return answer(GET, 'Power save: ' + state)


def daemon(ticks=1, **options):
    """A connectivity-daemon case: the loop ends after `ticks` iterations on both sides."""
    env = {'Y2_DAEMON_TICKS': str(ticks), 'Y2_LAUNCHER_PATCH': 'platform_cases.connectivity:stop_after_ticks'}
    return {'ported': True, 'env': env, 'volatile': ['wall_time'], **options}


def network(association, boot='boot-a'):
    """/run/y2/network.json as the service daemon writes it, made when the case runs (fresh then)."""
    return lambda: json.dumps({'schema': 1, 'boot_id': boot, 'monotonic_s': time.monotonic(),
                               'association': association, 'radio_present': True}, separators=(',', ':'))


ATTACHED = [answer(ADD, 'OK'), answer(AUTOSCAN, 'OK'), answer(ACTION)]

CASES = [
    ('wifi-power-policy', ['wifi-power-policy'], {'ported': True, 'commands': [readback('off'), answer(SET + ['off'])]}),
    ('wifi-power-policy-initialized', ['wifi-power-policy'], {'ported': True, 'commands': [readback('off')], 'files': {
        IFINDEX: '3\n', WIFI_POWER: json.dumps({'state': 'Ready', 'boot_id': 'boot-a', 'epoch': EPOCH,
                                                'ifindex': '3', 'requested': False})}}),
    ('wifi-power-policy-other-epoch', ['wifi-power-policy'], {'ported': True, 'commands': [readback('off'), answer(
        SET + ['off'])], 'files': {WIFI_POWER: json.dumps({'state': 'Ready', 'boot_id': 'boot-a', 'epoch': 'old',
                                                           'ifindex': None, 'requested': False})}}),
    ('wifi-power-policy-standard-unconfirmed', ['wifi-power-policy'], {'ported': True, 'commands': [
        readback('off'), answer(SET + ['on'])], 'files': {POWER_SAVE: '{"schema": 1, "mode": "standard"}'}}),
    ('wifi-power-policy-automatic-standard', ['wifi-power-policy'], {'ported': True, 'commands': [
        readback('on'), answer(SET + ['on'])], 'files': {
            POWER_SAVE: '{"schema": true, "mode": "automatic", "automatic_standard": true}'}}),
    ('wifi-power-policy-no-iw', ['wifi-power-policy'], {'ported': True}),
    ('wifi-power-policy-set-fails', ['wifi-power-policy'], {'ported': True, 'commands': [
        readback('on'), answer(SET + ['off'], ok=False)]}),
    ('wifi-power-policy-off', ['wifi-power-policy'], {'ported': True, 'commands': [readback('off')], 'files': {
        POWER_SAVE: '{"schema": 1, "mode": "off"}', WIFI_POWER: '[]'}}),
    ('wifi-power-policy-invalid', ['wifi-power-policy'], {'ported': True, 'files': {POWER_SAVE: '{"schema": 2, "mode": "off"}'}}),
    ('wifi-power-policy-bad-mode', ['wifi-power-policy'], {'ported': True, 'files': {POWER_SAVE: '{"schema": 1, "mode": "turbo"}'}}),
    ('wifi-power-policy-not-object', ['wifi-power-policy'], {'ported': True, 'commands': [readback('off')],
                                                             'files': {POWER_SAVE: '[1]'}}),
    ('wifi-power-policy-help', ['wifi-power-policy', '--help'], {'ported': False}),
    ('bluetooth-power-on', ['bluetooth-power', 'on'], {'ported': True, 'patterns': INTENT, 'commands': [
        answer(['bluetoothctl', 'power', 'on'], 'Changing power on succeeded')], 'files': {
            '/proc/sys/kernel/random/boot_id': BOOT + '\n'}}),
    ('bluetooth-power-off', ['bluetooth-power', 'off'], {'ported': True, 'patterns': INTENT, 'commands': [
        answer(['bluetoothctl', 'power', 'off'])], 'files': {'/proc/sys/kernel/random/boot_id': BOOT + '\n'}}),
    ('bluetooth-power-fails', ['bluetooth-power', 'on'], {'ported': True, 'patterns': INTENT, 'files': {
        '/proc/sys/kernel/random/boot_id': BOOT + '\n', '/run/y2/bt-intent/bt-control.ini': 'old\n'}}),
    ('bluetooth-power-no-boot-identity', ['bluetooth-power', 'off'], {'ported': True}),
    ('bluetooth-power-intent-not-directory', ['bluetooth-power', 'on'], {'ported': True, 'files': {
        '/proc/sys/kernel/random/boot_id': BOOT + '\n', '/run/y2/bt-intent': 'file\n'}}),
    ('bluetooth-power-invalid', ['bluetooth-power', 'maybe'], {'ported': False}),
    ('connectivity-daemon-cold', ['connectivity-daemon'], daemon()),
    ('connectivity-daemon-attach', ['connectivity-daemon'], daemon(3, commands=ATTACHED, files={IFINDEX: '3\n'})),
    ('connectivity-daemon-reuse', ['connectivity-daemon'], daemon(2, files={IFINDEX: '3\n'}, commands=[
        answer(ADD, 'FAIL'), answer(GLOBAL + ['interface_list'], 'p2p0\nwlan0'), answer(AUTOSCAN, 'OK'),
        answer(ACTION, ok=False)])),
    ('connectivity-daemon-not-listed', ['connectivity-daemon'], daemon(2, files={IFINDEX: '3\n'}, commands=[
        answer(GLOBAL + ['interface_list'], 'p2p0')])),
    ('connectivity-daemon-stale-pids', ['connectivity-daemon'], daemon(files={
        '/run/y2/wpa.pid': '1\n', '/run/y2/bluealsa.pid': 'x\n', '/run/y2/reconnect.pid': '999999\n',
        '/run/y2/calibration.pid': '0042\n', '/run/y2/calibration-ready': ''})),
    ('connectivity-daemon-no-codec-policy', ['connectivity-daemon'], daemon(files={
        '/data/bluetooth/codec-policy.json': '{"schema": 2}'})),
    ('connectivity-daemon-codec-crash', ['connectivity-daemon'], daemon(files={
        '/etc/y2linux/bluetooth-codecs.json': '{"schema": 1, "codecs": []}'})),
    ('connectivity-daemon-calibration-error', ['connectivity-daemon'], daemon(files={
        '/run/y2/calibration-ready/held': ''})),
    ('connectivity-daemon-completed', ['connectivity-daemon'], daemon(2, commands=ATTACHED, files={
        IFINDEX: '3\n', '/run/y2/network.json': network('COMPLETED')})),
    ('connectivity-daemon-disabled', ['connectivity-daemon'], daemon(3, commands=ATTACHED, files={
        IFINDEX: '3\n', '/run/y2/network.json': network('INTERFACE_DISABLED')})),
    ('connectivity-daemon-disabled-other-boot', ['connectivity-daemon'], daemon(2, commands=ATTACHED, files={
        IFINDEX: '3\n', '/run/y2/network.json': network('INTERFACE_DISABLED', boot='boot-z')})),
    ('connectivity-daemon-help', ['connectivity-daemon', '--help'], {'ported': False}),
    ('connectivity-daemon-extra', ['connectivity-daemon', 'now'], {'ported': False}),
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path): none, each case brings its own."""


def stop_after_ticks():
    """Y2_LAUNCHER_PATCH for connectivity-daemon cases: as the Rust daemon below a fixture root, the loop
    does not wait, sees no netlink event (a socket that never becomes readable) and ends after
    Y2_DAEMON_TICKS iterations (its wait is the last step of one)."""
    import os
    import types
    from y2_platform import connectivity

    ticks = int(os.environ['Y2_DAEMON_TICKS'])
    state = {'running': None, 'selects': 0}

    class Wake(connectivity.SignalWake):
        def __init__(self, running):
            super().__init__(running)
            state['running'] = running

    class Events:
        def __init__(self):
            self.sock = object()

        def wait(self, timeout, wake_fd):
            return False

        def close(self):
            self.sock = None

    def select(readers, writers, errors, timeout=None):
        state['selects'] += 1
        if state['selects'] >= ticks:
            state['running'][0] = False
        return [], [], []

    connectivity.SignalWake = Wake
    connectivity.AddressEvents = Events
    connectivity.select = types.SimpleNamespace(select=select)
