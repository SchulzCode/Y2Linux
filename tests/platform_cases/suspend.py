"""Differential cases of port group `suspend` (see tests/platform_cases/__init__.py).

`CASES` run on the plain harness: `sleep request` (every admission refusal, the fixture launcher's refusal after
the `requested` receipt, the quarantine guard and its recovery, the Python fallbacks) and `sleep prepare` without
Reborn. The cases that need a live peer (Reborn's control socket, a lock holder, the helper's inherited lock) or
the hidden programmes (`__product-sleep`, `__suspend-record`, `__power-command`, `__power-audit`) are in
`SERVED_CASES` and run by tests/test_platform_suspend.py.

`launcher()` is the Python side of the hidden programmes (`Y2_LAUNCHER_PATCH=platform_cases.suspend:launcher`):
it runs product-sleep.py, suspend-record.py, y2-power-command and y2-power-audit on the fixture root the way the
installed programmes run on the player.
"""
import json
import os
from pathlib import Path
import sys
import time
import traceback

PATCH = {'Y2_LAUNCHER_PATCH': 'platform_cases.suspend:launcher'}
SLEEP = '/data/system/platform/suspend-last.json'
GUARD = '/data/system/platform/product-sleep-guard.json'
SPM = '/sys/bus/platform/devices/10006000.power-controller/state'
# Wall clock readings in the receipts, journals and guard (inputs, not behaviour).
VOLATILE = ('started', 'updated', 'wall_time', 'collector_cpu_seconds', 'not_before_ns')
IDENTITY = {'build_id': None, 'build_git_commit': 'linux-commit', 'reborn_source_commit': 'reborn',
            'kernel': '6.18.0-fixture'}
POLICY = json.dumps({'schema': 2, 'enabled': True, 'critical_uv': 3400000, 'low_uv': 3500000,
                     'recover_uv': 3600000, 'critical_samples': 3, 'grace_seconds': 10,
                     'qualification_reference': 'fixture', 'warning_soc': 20, 'critical_soc': 10,
                     'shutdown_soc': 5})


def power(boot='boot-a', age_s=0.0, state='Normal', **battery):
    """/run/y2/power.json as the power daemon writes it, made when the case runs."""
    battery = {'status': 'Discharging', 'percent': 60, 'confidence': 'hardware',
               'filtered_voltage_uv': 3900000, **battery}
    return lambda: json.dumps({'schema': 1, 'boot_id': boot, 'state': state, 'battery': battery,
                               'monotonic_ns': time.monotonic_ns() - int(age_s * 1e9)},
                              sort_keys=True, separators=(',', ':'))


# Everything admission and the battery/cable gate need to let a sleep through.
READY = {
    '/sys/class/power_supply/BAT0/status': 'Discharging\n',
    '/sys/class/power_supply/y2-usb-presence/online': '0\n',
    SPM: 'system_sleep_abi=1 broken=0 timer_wake=0 ticks32k=98304\n',
    '/sys/power/state': 'freeze mem\n',
    '/sys/power/pm_test': '[none] core processors platform devices freezer\n',
    '/sys/firmware/y2_pm/backstop_s': 'provider=1 armed_s=0 running=0\n',
    '/etc/y2linux/power-policy.json': POLICY,
    '/run/y2/power.json': power(),
    '/proc/interrupts': '           CPU0\n 160:         12  mt6397  14 Edge  mt6397-rtc\n'
                        ' 161:          4  mt6397  15 Edge  mtk-pmic-keys\n',
}


def ready(**changes):
    return {'ported': True, 'volatile': VOLATILE, 'files': {**READY, **changes}}


def guard(pending=True, updated=1.0, **extra):
    return json.dumps({'identity': IDENTITY, 'pending': pending, 'updated': updated, 'boot_id': 'boot-z', **extra})


REQUEST = ['sleep', 'request']
CASES = [
    ('sleep-request-charging', REQUEST, {'ported': True, 'volatile': VOLATILE}),
    ('sleep-request-no-sleep-abi', REQUEST, ready(**{SPM: 'system_sleep_abi=0 broken=0\n'})),
    ('sleep-request-two-spm', REQUEST, ready(**{'/sys/bus/platform/devices/10006000.other/state': 'x=1\n'})),
    ('sleep-request-broken', REQUEST, ready(**{SPM: 'system_sleep_abi=1 broken=1\n'})),
    # F26: a WARN taint alone still sleeps; any other bit refuses.
    ('sleep-request-warn-taint', REQUEST, ready(**{'/proc/sys/kernel/tainted': '512\n'})),
    ('sleep-request-tainted', REQUEST, ready(**{'/proc/sys/kernel/tainted': '4608\n'})),
    ('sleep-request-no-mem', REQUEST, ready(**{'/sys/power/state': 'freeze\n'})),
    ('sleep-request-pm-test', REQUEST, ready(**{'/sys/power/pm_test': 'none [core]\n'})),
    ('sleep-request-no-backstop', REQUEST, ready(**{'/sys/firmware/y2_pm/backstop_s': 'provider=0\n'})),
    ('sleep-request-backstop-armed', REQUEST, ready(**{'/sys/firmware/y2_pm/backstop_s':
                                                       'provider=1 armed_s=30 running=1\n'})),
    ('sleep-request-retry-wait', REQUEST, ready(**{'/run/y2/product-sleep-retry.json': json.dumps(
        {'boot_id': 'boot-a', 'not_before_ns': 9 * 10**18})})),
    ('sleep-request-retry-other-boot', REQUEST, ready(**{'/run/y2/product-sleep-retry.json': json.dumps(
        {'boot_id': 'boot-z', 'not_before_ns': 9 * 10**18})})),
    ('sleep-request-quarantine', REQUEST, ready(**{GUARD: guard()})),
    # 12: a pending guard written before kernel entry is no quarantine.
    ('sleep-request-pending-before-kernel', REQUEST, ready(**{GUARD: guard(kernel_entry=False)})),
    ('sleep-request-quarantine-other-image', REQUEST, ready(**{GUARD: json.dumps(
        {'identity': {**IDENTITY, 'kernel': 'other'}, 'pending': True})})),
    # An explicit owner trial (no product mode) restored on this boot recovers the quarantine.
    ('sleep-request-quarantine-recovered', REQUEST, ready(**{GUARD: guard(), SLEEP: json.dumps(
        {'current_boot_id': 'boot-a', 'updated': 2.0, 'product': {
            'state': 'restored', 'kernel_completed': True, 'same_boot': True, 'request_boot_id': 'boot-a'}})})),
    ('sleep-request-quarantine-product-mode', REQUEST, ready(**{GUARD: guard(), SLEEP: json.dumps(
        {'mode': 'product', 'current_boot_id': 'boot-a', 'updated': 2.0, 'product': {
            'state': 'restored', 'kernel_completed': True, 'same_boot': True, 'request_boot_id': 'boot-a'}})})),
    # POWER-FINAL-16 Defect C: VBUS with an idle charger and no host may sleep.
    ('sleep-request-cable-full', REQUEST, ready(**{'/sys/class/power_supply/BAT0/status': 'Full\n',
                                                   '/sys/class/power_supply/y2-usb-presence/online': '1\n'})),
    ('sleep-request-cable-host', REQUEST, ready(**{'/sys/class/power_supply/BAT0/status': 'Not charging\n',
                                                   '/sys/class/power_supply/y2-usb-presence/online': '1\n',
                                                   '/sys/class/udc/11200000.usb/state': 'configured\n'})),
    ('sleep-request-cable-unknown', REQUEST, ready(**{'/sys/class/power_supply/y2-usb-presence/online': None})),
    ('sleep-request-battery-unknown', REQUEST, ready(**{'/sys/class/power_supply/BAT0/status': 'Unknown\n'})),
    ('sleep-request-stale-power', REQUEST, ready(**{'/run/y2/power.json': power(age_s=11)})),
    ('sleep-request-power-other-boot', REQUEST, ready(**{'/run/y2/power.json': power(boot='boot-z')})),
    ('sleep-request-power-shutdown', REQUEST, ready(**{'/run/y2/power.json': power(state='Shutdown')})),
    # F55: Low and Critical may sleep; the filtered voltage decides, down to critical_uv / shutdown_soc.
    ('sleep-request-low', REQUEST, ready(**{'/run/y2/power.json': power(state='Low', percent=15,
                                                                         filtered_voltage_uv=3550000)})),
    ('sleep-request-loaded-sample', REQUEST, ready(**{'/run/y2/power.json': power(filtered_voltage_uv=3500000),
                                                      '/sys/class/power_supply/BAT0/voltage_now': '3390000\n'})),
    ('sleep-request-critical-voltage', REQUEST, ready(**{'/run/y2/power.json': power(filtered_voltage_uv=3400000)})),
    ('sleep-request-unfiltered', REQUEST, ready(**{'/run/y2/power.json': power(filtered_voltage_uv=None),
                                                   '/sys/class/power_supply/BAT0/voltage_now': '3399999\n'})),
    ('sleep-request-empty-soc', REQUEST, ready(**{'/run/y2/power.json': power(state='Critical', percent=5)})),
    ('sleep-request-estimated-soc', REQUEST, ready(**{'/run/y2/power.json': power(percent=5, confidence='voltage')})),
    ('sleep-request-no-policy', REQUEST, ready(**{'/etc/y2linux/power-policy.json': None})),
    ('sleep-request-bad-policy', REQUEST, ready(**{'/data/system/platform/power-policy.json': json.dumps(
        {**json.loads(POLICY), 'grace_seconds': 2})})),
    ('sleep-request-no-voltage', REQUEST, ready(**{'/sys/class/power_supply/BAT0/voltage_now': None})),
    ('sleep-request-shutting-down', REQUEST, ready(**{'/run/y2/shutdown.json': '{"state": "ShutdownPending"}'})),
    ('sleep-request-no-power-battery', REQUEST, ready(**{'/run/y2/power.json': lambda: json.dumps(
        {'boot_id': 'boot-a', 'state': 'Normal', 'monotonic_ns': time.monotonic_ns()})})),
    # The journal row crashes on a battery that is no dict (AttributeError), after the receipt was written.
    ('sleep-request-battery-null', REQUEST, ready(**{'/run/y2/power.json': lambda: json.dumps(
        {'boot_id': 'boot-a', 'state': 'Normal', 'monotonic_ns': time.monotonic_ns(), 'battery': None})})),
    ('sleep-request-battery-list', REQUEST, ready(**{'/run/y2/power.json': lambda: json.dumps(
        {'boot_id': 'boot-a', 'state': 'Normal', 'monotonic_ns': time.monotonic_ns(), 'battery': []})})),
    ('sleep-request-data-unverified', REQUEST, ready(**{'/data/.y2data-schema': None})),
    ('sleep-request-no-receipt', REQUEST, ready(**{SLEEP: None})),
    # The dispatcher's argparse prints the help itself.
    ('sleep-request-help', ['sleep', 'request', '--help'], {'ported': True}),
    # Without Reborn's control socket: the refusal receipt, then main.py's error record.
    ('sleep-prepare-no-reborn', ['sleep', 'prepare'], {'ported': True, 'volatile': VOLATILE}),
    ('sleep-prepare-list-product', ['sleep', 'prepare'], {'ported': True, 'volatile': VOLATILE,
                                                          'files': {SLEEP: '{"product": [["state", "idle"]]}'}}),
    ('sleep-prepare-battery-null', ['sleep', 'prepare'], ready(**{'/run/y2/power.json': lambda: json.dumps(
        {'boot_id': 'boot-a', 'battery': None})})),
]
# `sleep status` of receipts whose product is no dict: Python's dict() of it (a list of pairs builds one;
# anything else is its ValueError record or TypeError traceback), unhashable states and keys that are no str.
STATUS = ['sleep', 'status']
for name, product in [
        ('pairs', [['state', 'restored'], ['request_boot_id', 'boot-a'], ['kernel_completed', True]]),
        ('pairs-other-boot', [['state', 'sleeping'], ['request_boot_id', 'boot-old']]),
        ('pair-texts', ['ab', {'state': 'refused'}, ['reason', 'usb']]),
        ('pair-dicts', [{'state': 'x', 'y': 1}]),
        ('text', 'restored'),
        ('number', 3),
        ('float', 2.5),
        ('true', True),
        ('empty-list', []),
        ('false', False),
        ('triple', [['state', 'idle', 'x']]),
        ('short-text', ['a']),
        ('number-item', [['state', 'restored'], 5]),
        ('null-item', [None]),
        ('list-key', [[['state'], 'restored']]),
        ('dict-key', [[{}, 'restored']]),
        ('number-key', [[1, 'x'], ['state', 'restored'], ['request_boot_id', 'boot-a']]),
        ('number-key-idle', [[1, 'x'], ['state', 'nap']]),
        ('null-key', [[None, 'x'], ['state', 'waking']]),
        ('list-state', [['state', ['restored']]]),
        ('dict-state', {'state': {'a': 1}}),
        ('number-state', {'state': 7}),
]:
    CASES.append(('sleep-status-product-' + name, STATUS, {'ported': True, 'files': {
        SLEEP: json.dumps({'product': product})}}))
CASES.append(('sleep-status-receipt-list', STATUS, {'ported': True, 'files': {SLEEP: '[["product", 1]]'}}))


# Cases that need tests/test_platform_suspend.py's sides: (name, argv, options, setup); `setup` keys:
# 'reborn' {op: answer line (str) or None to close without answering}, 'hold' [lock paths held during the run],
# 'inherit' True to make /run/y2/suspend.lock the run's stdin (/dev/null) as product-sleep's inherited lock.
OK = '{"id": 6582, "ok": true, "result": {"sleep_ready": true, "screen_off": true}}'
RESTORED = '{"id": 6582, "ok": true, "result": {"sleep_ready": true, "screen_off": false}}'
DARK = OK
REBORN = {'sleep_prepare': OK, 'sleep_restore': RESTORED}
RADIOS = [['/usr/sbin/y2-radio', 'wifi', 'off-runtime'], ['/usr/sbin/y2-radio', 'bluetooth', 'off-runtime'],
          ['/bin/sync'], ['/usr/sbin/y2-radio', 'wifi', 'on-runtime']]
COMMANDS = [{'argv': argv, 'ok': True, 'reason': None, 'output': ''} for argv in RADIOS]
KERNEL = ('valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=596 wake=0x4 '
          'failed_stage=NONE\n')
HELPER = {**READY, '/sys/power/wakeup_count': '7\n', '/sys/firmware/y2_pm/state': KERNEL,
          '/sys/firmware/y2_pm/stage': '', '/sys/module/workload/parameters/leases': 'Idle remaining_ms=0 min_khz=0\n',
          # The receipt `sleep request` wrote before launching the helper.
          SLEEP: json.dumps({'schema': 1, 'requested_mode': 'deep', 'request_boot_id': 'boot-a', 'pm_test': 'none',
                             'mode': 'product', 'stage': 'requested', 'result': 0, 'stages': [],
                             'product': {'state': 'requested', 'request_boot_id': 'boot-a', 'reason': None,
                                         'kernel_completed': False, 'wake_reason': 'unknown'}}),
          '/sys/devices/platform/18070000.connectivity/status': 'powered=0 functions=0 refs=0\n'}
PRODUCT = ['__product-sleep', '0']
SERVED_CASES = [
    ('sleep-prepare', ['sleep', 'prepare'], {'ported': True}, {'reborn': {'sleep_prepare': OK}}),
    ('sleep-prepare-not-dark', ['sleep', 'prepare'], {'ported': True}, {'reborn': {'sleep_prepare': RESTORED}}),
    ('sleep-prepare-refused', ['sleep', 'prepare'], {'ported': True}, {'reborn': {
        'sleep_prepare': '{"id": 6582, "ok": false, "error": "player_busy"}'}}),
    ('sleep-prepare-other-id', ['sleep', 'prepare'], {'ported': True}, {'reborn': {
        'sleep_prepare': '{"id": 6582.0, "ok": true, "result": []}'}}),
    ('sleep-prepare-closed', ['sleep', 'prepare'], {'ported': True}, {'reborn': {'sleep_prepare': None}}),
    ('sleep-prepare-garbage', ['sleep', 'prepare'], {'ported': True}, {'reborn': {'sleep_prepare': '{"id": 6582,'}}),
    ('sleep-request-busy', REQUEST, ready(), {'hold': ['/run/y2/suspend.lock']}),
    # Python's own crashes in the recovery check (AttributeError, TypeError): a traceback, exit 1. ('traceback':
    # printing it opens '<unknown>', which the host-access audit would count.)
    ('sleep-request-quarantine-text-product', REQUEST, ready(**{GUARD: guard(), SLEEP: '{"product": "x"}'}),
     {'traceback': True}),
    ('sleep-request-quarantine-text-updated', REQUEST, ready(**{GUARD: guard(), SLEEP: json.dumps(
        {'current_boot_id': 'boot-a', 'updated': 'late', 'product': {
            'state': 'restored', 'kernel_completed': True, 'same_boot': True}})}), {'traceback': True}),
    ('product-sleep', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-wifi-on', PRODUCT, {**ready(**{**HELPER, '/data/network/enabled': '1\n'}), 'commands': COMMANDS},
     {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-no-reborn', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS}, {'inherit': True}),
    ('product-sleep-reborn-refuses', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': {'sleep_prepare': '{"id": 6582, "ok": false, "error": "player_busy"}'}, 'inherit': True}),
    ('product-sleep-charging', PRODUCT, ready(**{**HELPER, '/sys/class/power_supply/BAT0/status': 'Charging\n'}),
     {'inherit': True}),
    ('product-sleep-activity-busy', PRODUCT, ready(**HELPER), {'inherit': True, 'hold': ['/run/y2/activity.lock']}),
    ('product-sleep-update-running', PRODUCT, ready(**{**HELPER, '/data/updates/state.json': '{"schema": 2, '
                                                                 '"state": "Staging"}'}), {'inherit': True}),
    ('product-sleep-maintenance-pending', PRODUCT, ready(**{**HELPER, 
        '/data/system/platform/maintenance-pending': ''}), {'inherit': True}),
    ('product-sleep-radios-on', PRODUCT, {**ready(**{**HELPER, 
        '/sys/devices/platform/18070000.connectivity/status': 'powered=1 functions=0x3 \n'}), 'commands': COMMANDS},
     {'reborn': REBORN, 'inherit': True, 'slow': True}),
    ('product-sleep-no-count', PRODUCT, {**ready(**{**HELPER, '/sys/power/wakeup_count': None}),
                                         'commands': COMMANDS}, {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-workload', PRODUCT, {**ready(**{**HELPER, '/sys/module/workload/parameters/leases':
                                                              'Playback remaining_ms=500 min_khz=-1\n'}),
                                         'commands': COMMANDS}, {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-kernel-unproven', PRODUCT, {**ready(**{**HELPER, '/sys/firmware/y2_pm/state': 'valid=0\n'}),
                                                'commands': COMMANDS}, {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-sync-fails', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS[:2]},
     {'reborn': REBORN, 'inherit': True}),
    ('product-sleep-restore-unanswered', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': {'sleep_prepare': OK}, 'inherit': True}),
    # The kernel write fails: a failed attempt stays quarantined (lit restore), or, when no task froze, the
    # 120-s retry gate and a dark restore (F47).
    ('product-sleep-kernel-error', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': REBORN, 'inherit': True, 'readonly': ['/sys/power/state']}),
    ('product-sleep-kernel-prepare-refused', PRODUCT, {**ready(**{**HELPER,
        '/sys/firmware/y2_pm/devices': 'ring=valid cycle=4\n1 phase=0 mark result=0 ms=1 stage=HELPER_REQUEST\n'
                                       '2 phase=0 mark result=0 ms=2 stage=SUSPEND_REQUEST\n'
                                       '3 phase=0 mark result=-1 ms=3 stage=EXIT\n',
        '/sys/firmware/y2_pm/state': 'valid=1 stage=EXIT failed_stage=EXIT error=-1\n'}), 'commands': COMMANDS},
     {'reborn': {'sleep_prepare': OK, 'sleep_restore': DARK}, 'inherit': True, 'readonly': ['/sys/power/state']}),
    # Archives 155/156: a stale count is retried with a fresh one twice, then refused as a busy wakeup source.
    ('product-sleep-stale-count', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': {'sleep_prepare': OK, 'sleep_restore': DARK}, 'inherit': True,
      'readonly': ['/sys/power/wakeup_count']}),
    # The backstop cannot be armed: no kernel entry, the failure is not a refusal.
    ('product-sleep-no-backstop', PRODUCT, {**ready(**HELPER), 'commands': COMMANDS},
     {'reborn': REBORN, 'inherit': True, 'readonly': ['/sys/firmware/y2_pm/backstop_s']}),
    ('product-sleep-wrong-lock', PRODUCT, ready(**HELPER), {}),
    ('product-sleep-stdout', ['__product-sleep', '1'], ready(**HELPER), {'inherit': True}),
    ('product-sleep-no-fd', ['__product-sleep'], {'ported': True}, {}),
    ('product-sleep-text-fd', ['__product-sleep', 'x'], {'ported': True}, {}),
    ('suspend-record-requested', ['__suspend-record', 'requested', '0'], {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-kernel-suspend', ['__suspend-record', 'kernel_suspend', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {**READY, '/sys/firmware/y2_pm/state': KERNEL}}, {}),
    ('suspend-record-kernel-resumed', ['__suspend-record', 'kernel_resumed', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {**READY, '/sys/firmware/y2_pm/state': KERNEL, SLEEP: json.dumps(
         {'pm_test': 'none', 'cycles': 1, 'suspended_seconds': 1.5, 'wake_irqs_before': {'rtc': 12, 'power': 2},
          'product': {'state': 'sleeping', 'request_boot_id': 'boot-a'}, 'stages': [{'stage': 'x'}] * 40})}}, {}),
    ('suspend-record-maintenance', ['__suspend-record', 'maintenance_wake', '0'],
     {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-failure', ['__suspend-record', 'product_failure', '1', 'radio_power_down'],
     {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-complete', ['__suspend-record', 'complete_result_0', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {'/sys/firmware/y2_pm/stage': '', SLEEP: json.dumps(
         {'pm_test': 'none', 'product': {'state': 'waking', 'request_boot_id': 'boot-a', 'kernel_completed': True}})}},
     {'reborn': {'sleep_restore': RESTORED}}),
    ('suspend-record-complete-no-reborn', ['__suspend-record', 'complete_result_0', '0'],
     {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-pm-test', ['__suspend-record', 'complete_result_0', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: '{"pm_test": "core"}'}},
     {'reborn': {'sleep_restore': RESTORED}}),
    # Python's IndexError / int()'s ValueError: a traceback, nothing written.
    ('suspend-record-bad-result', ['__suspend-record', 'syncing', 'x'], {'ported': True}, {}),
    ('suspend-record-no-stage', ['__suspend-record'], {'ported': True}, {}),
    ('suspend-record-float-result', ['__suspend-record', 'syncing', '1.0'], {'ported': True}, {}),
    # int() also reads whitespace, underscores and other decimal digits.
    ('suspend-record-odd-results', ['__suspend-record', 'syncing', ' +1_0 '],
     {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-arabic-result', ['__suspend-record', 'product_failure', '\u0663', 'why', 'extra'],
     {'ported': True, 'volatile': VOLATILE}, {}),
    ('suspend-record-odd-receipt', ['__suspend-record', 'syncing', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: '{"stages": "none"}'}}, {}),
    ('suspend-record-null-stages', ['__suspend-record', 'syncing', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: '{"stages": null}'}}, {}),
    ('suspend-record-list-receipt', ['__suspend-record', 'syncing', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: '[1, 2]'}}, {}),
    ('suspend-record-pairs-product', ['__suspend-record', 'syncing', '0'], {'ported': True, 'volatile': VOLATILE,
     'files': {SLEEP: json.dumps({'product': [['state', 'requested'], ['request_boot_id', 'boot-a']]})}}, {}),
    ('suspend-record-text-product', ['__suspend-record', 'syncing', '0'], {'ported': True, 'volatile': VOLATILE,
     'files': {SLEEP: json.dumps({'product': 'requested'})}}, {}),
    ('suspend-record-number-product', ['__suspend-record', 'syncing', '0'], {'ported': True, 'volatile': VOLATILE,
     'files': {SLEEP: json.dumps({'product': 5})}}, {}),
    # A key that is no str: the marker is written, then the sorted receipt dump raises (nothing published).
    ('suspend-record-number-key', ['__suspend-record', 'complete_result_0', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {'/sys/firmware/y2_pm/stage': '', SLEEP: json.dumps(
         {'pm_test': 'none', 'product': [[1, 'x'], ['state', 'waking'], ['request_boot_id', 'boot-a'],
                                         ['kernel_completed', True]]})}},
     {'reborn': {'sleep_restore': RESTORED}}),
    ('suspend-record-text-cycles', ['__suspend-record', 'kernel_resumed', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {**READY, '/sys/firmware/y2_pm/state': KERNEL, SLEEP: json.dumps(
         {'pm_test': 'none', 'cycles': '1', 'product': {'state': 'sleeping', 'request_boot_id': 'boot-a'}})}}, {}),
    ('suspend-record-text-seconds', ['__suspend-record', 'kernel_resumed', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {**READY, '/sys/firmware/y2_pm/state': KERNEL, SLEEP: json.dumps(
         {'pm_test': 'none', 'cycles': True, 'suspended_seconds': [1],
          'product': {'state': 'sleeping', 'request_boot_id': 'boot-a'}})}}, {}),
    ('suspend-record-text-wakes', ['__suspend-record', 'maintenance_wake', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: '{"maintenance_wakes": null}'}}, {}),
    ('suspend-record-list-pm-test', ['__suspend-record', 'complete_result_0', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {SLEEP: json.dumps({'pm_test': ['none']})}},
     {'reborn': {'sleep_restore': RESTORED}}),
    # The IRQ counts before the sleep as Python's `in` reads them: a list or str is searched, then indexed.
    *[('suspend-record-irqs-' + name, ['__suspend-record', 'kernel_resumed', '0'],
       {'ported': True, 'volatile': VOLATILE, 'files': {**READY, '/sys/firmware/y2_pm/state': KERNEL,
                                                        SLEEP: json.dumps({'pm_test': 'none', 'wake_irqs_before': before,
                                                                           'product': {'state': 'sleeping',
                                                                                       'request_boot_id': 'boot-a'}})}},
       {}) for name, before in [('list', ['rtc']), ('list-other', ['x', 1]), ('text', 'rtc power'),
                                ('text-other', 'none'), ('number', 3), ('null', None),
                                ('text-count', {'rtc': '1', 'power': 2}), ('null-count', {'power': None}),
                                ('bool-count', {'rtc': False, 'power': True})]],
    ('suspend-record-irqs-unproven', ['__suspend-record', 'kernel_resumed', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {**READY, SLEEP: json.dumps(
         {'pm_test': 'none', 'wake_irqs_before': 3, 'product': {'state': 'sleeping', 'request_boot_id': 'boot-a'}})}},
     {}),
    # A battery that is no dict: the receipt is written, then the journal row's battery.get raises.
    ('suspend-record-battery-null', ['__suspend-record', 'syncing', '0'],
     {'ported': True, 'volatile': VOLATILE, 'files': {'/run/y2/power.json': '{"battery": null}'}}, {}),
    ('power-command-flags', ['__power-command', 'reboot', '-f'], {'ported': True}, {}),
    ('power-command-name', ['__power-command', 'y2-power-command'], {'ported': True}, {}),
    ('power-command-poweroff', ['__power-command', 'poweroff'], {}, {}),
    ('power-audit-bounds', ['__power-audit', '--seconds', '0', '--state', 'x', '--output', '/x'],
     {'ported': True}, {}),
    # The script's argparse parser: help, abbreviations, `=`, its errors (exit 2), then the window bounds.
    *[('power-audit-' + name, ['__power-audit', *argv], {'ported': True}, {}) for name, argv in [
        ('help', ['--help']), ('h', ['-h']), ('help-late', ['--state', 'x', '--h']), ('h-joined', ['-hx']),
        ('help-after-error', ['--seconds', 'x', '-h']), ('error-after-help', ['-h', '--seconds', 'x']),
        ('help-value', ['--help=x']), ('h-value', ['-h=x']), ('nothing', []),
        ('no-output', ['--state', 'x']), ('ambiguous', ['--s', '1', '--state', 'x', '--output', '/x']),
        ('abbreviated', ['--sec=0', '--st', 'x', '--o', '/x']), ('settle', ['--settle', '121', '--state', 'x',
                                                                           '--output', '/x']),
        ('nan', ['--seconds', 'nan', '--state', 'x', '--output', '/x']),
        ('inf', ['--seconds', 'inf', '--state', 'x', '--output', '/x']),
        ('negative', ['--seconds', '-5', '--state', 'x', '--output', '/x']),
        ('bad-float', ['--seconds', 'x', '--state', 'x', '--output', '/x']),
        ('empty-float', ['--seconds=', '--state', 'x', '--output', '/x']),
        ('missing-value', ['--state']), ('option-value', ['--state', '-x', '--output', '/x']),
        ('dashes', ['--state', 'x', '--output', '/x', '--', 'y']), ('dash-value', ['--state', '--', 'x']),
        ('unknown', ['--state', 'x', '--output', '/x', '--x']), ('positional', ['y', '--state', 'x', '--output', '/x']),
        ('unknown-first', ['--x', '--seconds', '0']), ('single-dash', ['-s', '1']),
        ('space', ['--state', 'x', '--output', '/x', '-a b']),
        ('underscores', ['--seconds', '1_0_0_00', '--state', 'x', '--output', '/x']),
    ]],
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path): none; each case brings its own."""


# ---- the Python side of the hidden programmes (Y2_LAUNCHER_PATCH) ------------------------------------------------

def launcher():
    from y2_platform import cli
    original = cli.main

    def main():
        argv = sys.argv[1:]
        programmes = {'__product-sleep': product_sleep, '__suspend-record': suspend_record,
                      '__power-command': lambda a: power_command(a, original), '__power-audit': power_audit}
        if argv and argv[0] in programmes:
            return programmes[argv[0]](argv[1:])
        return original()
    cli.main = main


def crashed():
    traceback.print_exc()
    return 1


def product_sleep(argv):
    """tools/platform/product-sleep.py (product_sleep.main) on the fixture's Context."""
    import signal
    from y2_platform import cli, product_sleep
    try:
        fd = int(argv[0])

        def stop(signum, frame):
            raise RuntimeError('sleep_helper_interrupted')
        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        try:
            return product_sleep.execute(cli.Context(), fd)
        finally:
            os.close(fd)
    except Exception:
        return crashed()


def suspend_record(argv):
    """tools/platform/suspend-record.py (suspend_record.main) on the fixture's Context."""
    from y2_platform import cli, suspend_record
    try:
        value = suspend_record.record(cli.Context(), argv[0], int(argv[1]) if len(argv) > 1 else None,
                                      argv[2] if len(argv) > 2 else None)
    except Exception:
        return crashed()
    return 1 if value.get('error') else 0


def power_command(argv, original):
    """tools/platform/y2-power-command, installed as /sbin/poweroff and /sbin/reboot ($0 is argv[0])."""
    if not argv or argv[0] not in ('poweroff', 'reboot'):
        return 2
    if len(argv) > 1:
        print('Use y2-platform shutdown for normal shutdown; emergency flags are not supported.', file=sys.stderr)
        return 2
    sys.argv = ['y2-platform', 'shutdown', argv[0], '--reason', 'user']
    return original()


def power_audit(argv):
    """tools/platform/y2-power-audit with every absolute path below the fixture root."""
    import glob as host_glob
    import importlib.machinery
    import importlib.util
    import pathlib
    import types
    from y2_platform import cli, common
    root = str(cli.Context().root)
    script = Path(__file__).resolve().parents[2] / 'tools/platform/y2-power-audit'
    loader = importlib.machinery.SourceFileLoader('y2_power_audit', str(script))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader('y2_power_audit', loader))
    loader.exec_module(module)
    rooted = lambda p: p if str(p).startswith(root) else root + str(p)  # noqa: E731
    original_read = module.read
    module.read = lambda p: original_read(rooted(p))
    module.Path = lambda p: pathlib.Path(rooted(p)) if str(p).startswith('/') else pathlib.Path(p)
    module.glob = types.SimpleNamespace(glob=lambda pattern: [p[len(root):] for p in host_glob.glob(root + pattern)])
    rooted_context = cli.Context()
    common.Context = lambda *_: rooted_context
    sys.argv = ['y2-power-audit', *argv]
    try:
        module.main()
    except SystemExit:
        raise
    except Exception:
        return crashed()
    return 0
