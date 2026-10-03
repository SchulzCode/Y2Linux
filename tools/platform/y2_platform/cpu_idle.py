"""One boot-time admission of the physically qualified CPU0 dormant policy.

This only selects existing cpuidle/SPM controls. Every entry still passes the
kernel's topology, workload, OPP, deadline, UART ACK and restoration guards.
"""
# SPDX-License-Identifier: GPL-2.0-only
import os

from .common import atomic_json

STATE = '/sys/devices/system/cpu/cpu0/cpuidle/state2'
BUDGET = '/sys/module/spm/parameters/dormant_budget'
PREFLIGHT = '/sys/devices/platform/10006000.power-controller/dormant_preflight'
RECORD = '/run/y2/cpu-idle-policy.json'
LOCK = '/run/y2/cpu-idle-policy.lock'
# Other prerequisites describe transient activity and remain entry-time guards.
FOUNDATIONS = ('system_running', 'boot_policy', 'spm', 'local_events', 'cirq',
               'linux_context', 'timer_context', 'broken')


def _write(ctx, path, value):
    with ctx.path(path).open('w') as stream:
        stream.write(value + '\n')


def _disable(ctx):
    errors = []
    # Close admission first, then quarantine the state. Do not change C1/C2.
    for path, value in ((BUDGET, '0'), (STATE + '/disable', '1')):
        try:
            _write(ctx, path, value)
            if ctx.read(path) != value:
                errors.append(path + ':readback')
        except OSError:
            errors.append(path + ':write')
    return errors


def apply(ctx, action='start'):
    """Fail closed, run once per boot, and never override later manual policy."""
    if action not in ('start', 'stop'):
        raise ValueError('invalid CPU idle policy action')
    ctx.path('/run/y2').mkdir(parents=True, exist_ok=True)
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    result = {'schema': 'org.y2linux.cpu-idle-policy/v1', 'boot_id': boot,
              'policy': 'qualified_CPU0_DORMANT', 'state': 'Skipped', 'reasons': []}
    if action == 'start':
        try:
            fd = os.open(ctx.path(LOCK), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
        except FileExistsError:
            return {**result, 'reasons': ['already_applied_this_boot'],
                    'previous': ctx.json(RECORD, {})}
        os.close(fd)
        versions = ctx.json('/etc/y2linux/versions.json', {})
        if not boot or versions.get('kernel_version') != ctx.read('/proc/sys/kernel/osrelease'):
            result['reasons'].append('installed_kernel_identity')
        if ctx.read('/proc/sys/kernel/tainted') != '0':
            result['reasons'].append('kernel_tainted_or_unknown')
        if ctx.read('/sys/devices/system/cpu/cpuidle/current_driver') != 'mt6582-idle':
            result['reasons'].append('cpuidle_driver')
        if ctx.read(STATE + '/name') != 'DORMANT':
            result['reasons'].append('dormant_state_unavailable_or_boot_disabled')
        preflight = (ctx.read(PREFLIGHT) or '').splitlines()
        result['reasons'].extend('foundation:' + name for name in FOUNDATIONS
                                 if 'prerequisite_' + name + '=1' not in preflight)
        if result['reasons']:
            # Initial controls remain quarantined; respect explicit boot options.
            atomic_json(ctx.path(RECORD), result)
            return result
        if ctx.read(BUDGET) != '0' or ctx.read(STATE + '/disable') != '1':
            result['reasons'] = ['existing_owner_runtime_policy']
            atomic_json(ctx.path(RECORD), result)
            return result
        try:
            # State remains disabled until the qualified budget is read back.
            _write(ctx, BUDGET, '-1')
            if ctx.read(BUDGET) != '-1':
                raise OSError('dormant budget readback')
            _write(ctx, STATE + '/disable', '0')
            if ctx.read(STATE + '/disable') != '0':
                raise OSError('cpuidle state readback')
            result['state'] = 'Enabled'
        except OSError as error:
            result.update(state='Failed', reasons=[str(error)], rollback_errors=_disable(ctx))
    else:
        # Reserve boot ownership so a subsequent start cannot undo a manual stop.
        fd = os.open(ctx.path(LOCK), os.O_WRONLY | os.O_CREAT | os.O_CLOEXEC, 0o600)
        os.close(fd)
        errors = _disable(ctx)
        result.update(state='Failed' if errors else 'Disabled', reasons=errors)
    result['budget'] = ctx.read(BUDGET)
    result['disabled'] = ctx.read(STATE + '/disable')
    try:
        atomic_json(ctx.path(RECORD), result)
    except OSError:
        if result['state'] == 'Enabled':
            _disable(ctx)
        raise
    return result
