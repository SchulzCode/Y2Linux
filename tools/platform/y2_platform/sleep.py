"""Product sleep contract; kernel qualification remains an explicit owner path."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import os
import re
from .boot import private_directory

SCHEMA = 'org.y2linux.sleep/v1'
RECEIPT = '/data/system/platform/suspend-last.json'
STATES = {'requested', 'refused', 'sleeping', 'restoring', 'restored', 'restore_failed'}


def product(value, boot_id):
    """Interpret durable evidence without promoting a reset into a resume."""
    result = dict(value.get('product') or {})
    if result.get('state') not in STATES:
        result = {'state': 'idle', 'reason': None, 'request_boot_id': None,
                  'kernel_completed': False, 'wake_reason': 'unknown'}
    result['schema'] = SCHEMA
    result['boot_id'] = boot_id
    request_boot = result.get('request_boot_id')
    result['same_boot'] = bool(request_boot == boot_id) if request_boot and boot_id else None
    if result['state'] in {'restored', 'refused'} and result['same_boot'] is False:
        result.update(state='idle', reason=None, request_boot_id=None, same_boot=None,
                      kernel_completed=False, wake_reason='unknown')
    if result['state'] in {'requested', 'sleeping', 'restoring'} and result['same_boot'] is False:
        result.update(state='restore_failed', reason='boot_changed_before_restoration', kernel_completed=False)
    return result


def status(ctx):
    value = ctx.json(RECEIPT, {})
    return product(value if isinstance(value, dict) else {}, ctx.read('/proc/sys/kernel/random/boot_id'))


def wake_irqs(text):
    """Serviced PMIC child IRQs, matching the existing owner harness names.

    Both RTC and Power feed SPM EINT. That shared SPM wake bit is deliberately
    insufficient; only a same-boot child-IRQ delta attributes their source.
    """
    counts = {}
    for line in (text or '').splitlines():
        fields = line.split()
        if not fields or not fields[0].endswith(':'):
            continue
        for label, name in (('mt6397-rtc', 'rtc'), ('mtk-pmic-keys', 'power')):
            if label not in fields:
                continue
            total = 0
            for field in fields[1:]:
                if not field.isdecimal():
                    break
                total += int(field)
            counts[name] = counts.get(name, 0) + total
    return counts


def transition(value, stage, result, boot_id, reason=None):
    p = product(value, boot_id)
    if stage in {'requested', 'quiescing_radios'} or stage.startswith('refused_'):
        p.update(state='requested', reason=None, request_boot_id=boot_id,
                 same_boot=True if boot_id else None, kernel_completed=False, wake_reason='unknown')
    if stage.startswith('refused_'):
        p.update(state='refused', reason=stage.removeprefix('refused_'))
    elif stage == 'kernel_suspend':
        # Durable intent immediately before the blocking kernel write. Actual
        # SPM entry is proven only by the kernel journal after wake/reset.
        p.update(state='sleeping', reason=None)
    elif stage == 'restoring_radios':
        kernel = dict(re.findall(r'(\w+)=(\S+)', value.get('persistent_kernel') or ''))
        p['kernel_completed'] = (kernel.get('valid') == '1' and kernel.get('stage') == 'EXIT'
                                 and kernel.get('error') == '0' and p['same_boot'] is True)
        before, after = value.get('wake_irqs_before', {}), value.get('wake_irqs_after', {})
        if p['kernel_completed']:
            sources = [name for name in ('rtc', 'power') if name in before and name in after
                       and after[name] > before[name]]
            p['wake_reason'] = sources[0] if len(sources) == 1 else 'multiple' if sources else 'unknown'
        if p['state'] != 'refused':
            p.update(state='restoring', reason=reason)
        if kernel.get('failed_stage') == 'DPM_PREPARED' and kernel.get('error') == '-16':
            p.update(state='refused', reason='device_prepare_refused')
    elif stage.startswith('complete_result_'):
        if reason or result or value.get('error'):
            if p['state'] != 'refused':
                p.update(state='restore_failed', reason=reason or value.get('error') or 'platform_restoration_failed')
        elif p['same_boot'] is not True:
            p.update(state='restore_failed', reason='same_boot_not_proven')
        elif not p.get('kernel_completed'):
            p.update(state='restore_failed', reason='kernel_exit_not_proven')
        elif not value.get('reborn_restore'):
            p.update(state='restore_failed', reason='reborn_resume_probe_failed')
        else:
            p.update(state='restored', reason=None)
    return p


def request(ctx):
    """A successful API request may be refused by platform policy.

    There is deliberately no editable boolean that turns unqualified full
    suspend into ordinary product behavior. The owner helper retains the
    existing explicit qualification argument until same-boot wake is proven.
    """
    from .suspend_record import record
    root = private_directory(ctx.path('/run/y2'))
    fd = os.open(root / 'suspend.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            current = status(ctx)
            return dict(current, state='refused', reason='sleep_operation_in_progress')
        value = record(ctx, 'refused_physical_qualification_required', 2)
        return value['product']
    finally:
        os.close(fd)
