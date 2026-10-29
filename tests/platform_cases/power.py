"""Golden cases of port group `power` (see tests/platform_cases/__init__.py): `power-daemon` for one or a
few loop iterations (Y2_DAEMON_TICKS) and the `shutdown` client without a daemon. The client against a live
daemon (which needs the peer to be root) is tests/test_platform_power_rpc.py."""
import json
import os
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
OVERLAY = REPOSITORY / 'buildroot/board/y2/production-overlay/etc/y2linux'
UUID = (r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', '<uuid>')
BAT0 = '/sys/class/power_supply/BAT0/'
UI = '/data/system/platform/battery-ui.json'
SHUTDOWN = '/run/y2/shutdown.json'
METADATA = '/sys/firmware/y2_boot/metadata'
REBORN_PID = 4242
# A Reborn process as process_identity reads it (/usr/bin/reborn links to this exe: fixture()).
REBORN = {'/run/reborn/process.pid': '%d\n' % REBORN_PID, '/proc/%d/exe' % REBORN_PID: '',
          '/proc/%d/stat' % REBORN_PID: '%d (reborn) S 1 %d %d 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 3 0 4242 1000 50\n'
          % (REBORN_PID, REBORN_PID, REBORN_PID)}


def ticks(n=1):
    return {'Y2_DAEMON_TICKS': str(n)}


def stock():
    """The stock battery profile and power policy (read when a case is built)."""
    return ((OVERLAY / 'battery-profile.json').read_text(),
            json.loads((OVERLAY / 'power-policy.json').read_text()))


def battery(voltage, status='Discharging', present='1', **extra):
    """BAT0 as the charger driver publishes it, the stock profile and policy installed."""
    profile, policy = stock()
    files = {BAT0 + 'voltage_now': '%d\n' % voltage, BAT0 + 'status': status + '\n',
             '/etc/y2linux/battery-profile.json': profile, '/etc/y2linux/power-policy.json': json.dumps(policy)}
    if present is not None:
        files[BAT0 + 'present'] = present + '\n'
    files.update({BAT0 + name: value for name, value in extra.items()})
    return files


def intent(state='Executing', action='poweroff', reason='user', boot='boot-a', deadline=0, ack=None, **extra):
    """/run/y2/shutdown.json as the daemon publishes it (sorted keys)."""
    value = {'schema': 1, 'id': 'b6a2f0de-6a1c-4b7e-9c0e-000000000001', 'boot_id': boot, 'state': state,
             'action': action, 'reason': reason, 'requested_monotonic_ns': 0, 'deadline_monotonic_ns': deadline,
             'client': None, 'acknowledgement': ack, 'failure': None, **extra}
    return json.dumps(value, sort_keys=True, separators=(',', ':'))


def answers(*argvs, ok=True):
    return [{'argv': list(argv), 'ok': ok, 'reason': None if ok else 'command_failed', 'output': ''} for argv in argvs]


STAGE = ['/usr/sbin/y2-platform', 'boot-stage']
STEPS_OK = answers(STAGE + ['shutdown_pending'], ['/etc/init.d/S90reborn', 'stop'],
                   ['/usr/sbin/y2-platform', 'time-remember'], ['/bin/busybox', 'sync'],
                   *[['/etc/init.d/S41y2-connectivity', 'stop-' + n] for n in ('wifi', 'bluetooth', 'daemon')],
                   STAGE + ['shutdown_complete'])
# What a shutdown leaves: the watchdog, the PMIC long-press switch and a backlight to switch off.
SHUTDOWN_NODES = {'/dev/watchdog': '', '/sys/firmware/y2_pmic/long_press_reset': '1\n',
                  '/sys/class/backlight/panel/bl_power': '0\n'}
DAEMON = {'patterns': [UUID],
          'volatile': ['wall_time', 'collector_cpu_seconds', 'requested_monotonic_ns', 'deadline_monotonic_ns']}


def daemon(n=1, files=None, **options):
    return {**DAEMON, 'env': ticks(n), 'files': files or {}, **options}


def cases():
    """The cases, built when the harness asks for CASES (PEP 562)."""
    policy = stock()[1]
    critical_once = json.dumps({**policy, 'critical_samples': 1, 'grace_seconds': 3})
    return [
        # power-daemon: battery states
        ('power-daemon-no-policy', ['power-daemon'], daemon()),
        ('power-daemon-discharging', ['power-daemon'], daemon(2, battery(3850000))),
        ('power-daemon-charging-oam', ['power-daemon'], daemon(1, battery(
            4120000, 'Charging', charging_state='phase=CONSTANT_CURRENT active=1 sense_current_est_ua=450000\n'))),
        ('power-daemon-full', ['power-daemon'], daemon(2, battery(4175000, 'Full', charging_state='phase=FULL active=0\n'))),
        ('power-daemon-not-charging', ['power-daemon'], daemon(1, battery(3990000, 'Not charging'))),
        ('power-daemon-low', ['power-daemon'], daemon(1, battery(3480000))),
        ('power-daemon-critical-soc', ['power-daemon'], daemon(1, battery(3505000))),
        ('power-daemon-no-pack', ['power-daemon'], daemon(1, battery(3850000, present='0'))),
        ('power-daemon-no-present', ['power-daemon'], daemon(1, battery(3850000, present=None))),
        ('power-daemon-fuel-gauge', ['power-daemon'], daemon(1, battery(3850000, capacity='77\n', current_now='-120000\n',
                                                                         temp='251\n', charge_counter='800000\n'))),
        ('power-daemon-bad-profile', ['power-daemon'], daemon(1, {**battery(3850000),
                                                                  '/data/system/platform/battery-profile.json': '[1]'})),
        ('power-daemon-policy-disabled', ['power-daemon'], daemon(1, {
            **battery(3850000), '/data/system/platform/power-policy.json': json.dumps({**policy, 'enabled': False})})),
        ('power-daemon-policy-invalid', ['power-daemon'], daemon(1, {
            **battery(3850000), '/data/system/platform/power-policy.json': json.dumps({**policy, 'grace_seconds': 99})})),
        # Saved percentage (POWER-FINAL-28), warm restart (F54), charger (F36)
        ('power-daemon-saved-percent', ['power-daemon'], daemon(1, {
            **battery(3850000), UI: json.dumps({'schema': 1, 'ui_percent': 60, 'anchor_percent': 97.5})})),
        ('power-daemon-warm-restart', ['power-daemon'], daemon(1, {
            **battery(4020000, voltage_ocv='3700000\n'), METADATA: 'valid=1 boot_reason=4 lcm=1-st7701\n',
            UI: json.dumps({'schema': 1, 'ui_percent': 95, 'anchor_percent': None})})),
        ('power-daemon-cold-start', ['power-daemon'], daemon(1, {
            **battery(4020000), METADATA: 'valid=1 boot_reason=0\n', UI: json.dumps({'schema': 1, 'ui_percent': 30})})),
        ('power-daemon-charged-boot', ['power-daemon'], daemon(1, {
            **battery(4027000, 'Charging'), METADATA: 'valid=1 offline=1 boot_reason=0\n',
            UI: json.dumps({'schema': 1, 'ui_percent': 5})})),
        ('power-daemon-low-saved-on-charger', ['power-daemon'], daemon(1, {
            **battery(3720000, 'Charging'), UI: json.dumps({'schema': 1, 'ui_percent': 5})})),
        ('power-daemon-ui-unwritable', ['power-daemon'], daemon(1, {**battery(3850000), UI: None,
                                                                    '/data/system/platform/battery-ui.json/x': ''})),
        # Low battery: the intent, Reborn as its client, the journal's state_change row
        ('power-daemon-low-battery-shutdown', ['power-daemon'], daemon(1, {
            **battery(3390000), **REBORN, '/data/system/platform/power-policy.json': critical_once})),
        ('power-daemon-low-battery-no-reborn', ['power-daemon'], daemon(2, {
            **battery(3390000), '/data/system/platform/power-policy.json': critical_once})),
        ('power-daemon-critical-counting', ['power-daemon'], daemon(2, battery(3390000))),
        # Records that cannot be written (a directory in their place) are noted on stderr; the daemon goes on
        # (the Python one ended with a failure record): power.json, and the low-battery intent, which stays
        # pending in memory and runs after its grace.
        ('power-daemon-power-json-unwritable', ['power-daemon'], daemon(1, {
            **battery(3850000), '/run/y2/power.json/x': ''})),
        ('power-daemon-low-battery-intent-unwritable', ['power-daemon'], daemon(1, {
            **battery(3390000), '/data/system/platform/power-policy.json': critical_once,
            '/run/y2/shutdown.json/x': ''})),
        # Journal and diagnostics inputs
        ('power-daemon-no-data-mount', ['power-daemon'], daemon(1, {**battery(3850000), '/run/y2-data-device': '/dev/sda1\n'})),
        ('power-daemon-journal-inputs', ['power-daemon'], daemon(1, {
            **battery(3850000), '/sys/class/udc/11200000.usb/state': 'configured\n',
            '/sys/bus/platform/drivers/y2-usb/11200000.usb/status': 'stage=CONFIGURED error=0 irqs=12 other=1\n',
            '/sys/bus/platform/drivers/y2-usb/11200000.usb/rxdiag': 'out=1 mode=1\nsecond\n',
            '/sys/class/net/usb0/operstate': 'up\n', '/sys/class/net/usb0/statistics/rx_packets': '17\n',
            '/proc/net/route': 'Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n'
                               'usb0\t0007A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n',
            '/proc/net/fib_trie': 'Main:\n  +-- 192.168.7.0/24 2 0 2\n     |-- 192.168.7.2\n        /32 host LOCAL\n',
            '/sys/class/backlight/panel/bl_power': '0\n', '/run/y2/cpu-idle-policy.json': '{"state": "Enabled", "a": 1}',
            '/sys/devices/system/cpu/cpu0/cpuidle/state0/name': 'WFI\n',
            '/sys/devices/system/cpu/cpu0/cpuidle/state0/usage': '99\n',
            '/sys/bus/platform/devices/11200000.usb/power/runtime_status': 'active\n',
            '/sys/bus/platform/devices/99000000.x/power/runtime_status': 'active\n',
            '/sys/module/idle/parameters/rejections': '3\n', '/sys/module/clocks/parameters/deep_blockers': 'audio\n',
            '/sys/firmware/y2_pm/state': 'stage=EXIT\n',
            '/data/system/platform/suspend-last.json': json.dumps({
                'mode': 'product', 'cycles': 3, 'maintenance_wakes': 1,
                'product': {'state': 'refused', 'request_boot_id': 'boot-a', 'reason': 'usb'}})})),
        ('power-daemon-bad-route', ['power-daemon'], daemon(1, {
            **battery(3850000), '/proc/net/route': 'Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n'
                                                   'usb0\tzz\t0\t0\t0\t0\t0\t00FFFFFF\n'})),
        ('power-daemon-receipt-not-a-dict', ['power-daemon'], daemon(1, {
            **battery(3850000), '/data/system/platform/suspend-last.json': '{"product": 5}'})),
        # Shutdown intents from before a daemon restart
        ('power-daemon-resume-poweroff', ['power-daemon'], daemon(1, {
            **battery(3850000), **SHUTDOWN_NODES, **REBORN, SHUTDOWN: intent()}, commands=STEPS_OK)),
        ('power-daemon-resume-poweroff-init-ok', ['power-daemon'], daemon(1, {
            **battery(3850000), **SHUTDOWN_NODES, SHUTDOWN: intent(reason='low_battery')},
            commands=STEPS_OK + answers(['/bin/busybox', 'poweroff']))),
        ('power-daemon-resume-reboot-acknowledged', ['power-daemon'], daemon(1, {
            **battery(3850000), '/sys/class/backlight/panel/bl_power': '0\n',
            SHUTDOWN: intent(state='ShutdownPending', action='reboot', reason='update',
                             deadline=2**62, ack={'monotonic_ns': 5, 'outcome': 'Ready'})},
            commands=answers(STAGE + ['shutdown_pending'], ['/bin/busybox', 'reboot']))),
        ('power-daemon-resume-no-powers-off', ['power-daemon'], daemon(1, {
            **battery(3850000), '/dev/watchdog': '', SHUTDOWN: intent(),
            BAT0 + 'watchdog_powers_off/x': ''})),
        ('power-daemon-resume-wifi-stays', ['power-daemon'], daemon(1, {
            **battery(3850000), SHUTDOWN: intent(action='reboot'), '/sys/class/net/wlan0/operstate': 'up\n'},
            commands=STEPS_OK)),
        ('power-daemon-resume-acknowledged-failed', ['power-daemon'], daemon(2, {
            **battery(3850000), SHUTDOWN: intent(state='ShutdownPending', deadline=2**62,
                                                 ack={'monotonic_ns': 5, 'outcome': 'Failed'})})),
        ('power-daemon-failed-intent-kept', ['power-daemon'], daemon(1, {
            **battery(3850000), SHUTDOWN: intent(state='Failed', failure='init_shutdown_request_failed')})),
        ('power-daemon-intent-other-boot', ['power-daemon'], daemon(1, {**battery(3850000), SHUTDOWN: intent(boot='boot-z')})),
        ('power-daemon-intent-incomplete', ['power-daemon'], daemon(1, {
            **battery(3850000), SHUTDOWN: json.dumps({'boot_id': 'boot-a', 'state': 'Executing', 'action': 'poweroff',
                                                      'deadline_monotonic_ns': 0, 'id': 'x', 'client': None})})),
        ('power-daemon-unsafe-socket', ['power-daemon'], daemon(1, {'/run/y2/power.sock': ''})),
        ('power-daemon-help', ['power-daemon', '--help'], {}),
        # shutdown without a daemon
        ('shutdown-no-daemon', ['shutdown', 'poweroff'], {}),
        ('shutdown-refused-connection', ['shutdown', 'reboot', '--reason', 'update'],
         {'files': {'/run/y2/power.sock': ''}}),
        ('shutdown-reason-first', ['shutdown', '--reason', 'service', 'poweroff'], {}),
        ('shutdown-reason-equals', ['shutdown', 'reboot', '--reason=user'], {}),
        ('shutdown-low-battery-reason', ['shutdown', 'poweroff', '--reason', 'low_battery'], {}),
        ('shutdown-halt', ['shutdown', 'halt'], {}),
        ('shutdown-abbreviated', ['shutdown', 'poweroff', '--rea', 'user'], {}),
    ]



def extra_cases():
    """Odd input the Python daemon ended on (an uncaught exception): the daemon now keeps running
    (tests/test_platform_power_rpc.py runs them)."""
    return [
        # A list of pairs as the receipt's product: the journal row is dropped (as for any other non-dict).
        ('power-daemon-receipt-pairs', ['power-daemon'], daemon(1, {
            **battery(3850000), '/data/system/platform/suspend-last.json': '{"product": [["state", "idle"]]}'})),
        # A resumed acknowledgement that is not a dict is no acknowledgement: before the deadline nothing
        # happens ...
        ('power-daemon-resume-bad-ack', ['power-daemon'], daemon(1, {
            **battery(3850000), SHUTDOWN: intent(state='ShutdownPending', deadline=2**62, ack='Ready')})),
        # ... after it the shutdown runs as an unacknowledged one (the panel blanked before Reborn stops).
        ('power-daemon-resume-bad-ack-due', ['power-daemon'], daemon(1, {
            **battery(3850000), '/dev/watchdog': '', SHUTDOWN: intent(ack=['Ready'])})),
    ]


def __getattr__(name):
    if name == 'CASES':
        return cases()
    if name == 'EXTRA_CASES':
        return extra_cases()
    raise AttributeError(name)

# tests/test_platform_power_rpc.py's power socket conversation (one golden).
CONVERSATION = 'power-conversation'
GOLDEN_NAMES = (CONVERSATION,)


def fixture(root):
    """/usr/bin/reborn resolves to the exe of the Reborn process the cases name (REBORN): process_identity
    compares the two after Path.resolve()."""
    reborn = root / 'usr/bin/reborn'
    reborn.parent.mkdir(parents=True, exist_ok=True)
    reborn.symlink_to(os.path.relpath(root / 'proc' / str(REBORN_PID) / 'exe', reborn.parent))
