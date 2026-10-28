"""One bounded durable boot journal; an unclean boot is not a reset diagnosis."""
# SPDX-License-Identifier: GPL-2.0-only
import os
from pathlib import Path
import re
import stat
import time
from .common import atomic_json


def private_directory(path):
    path = Path(path)
    # No symlink traversal in mutable platform-owned state.
    for parent in list(reversed(path.parents)) + [path]:
        if parent.exists() or parent.is_symlink():
            mode = parent.lstat().st_mode
            if not stat.S_ISDIR(mode):
                raise ValueError('state_directory_not_directory')
        else:
            parent.mkdir(mode=0o700)
    os.chmod(path, 0o700)
    return path


RESET_STATUS = '/sys/firmware/y2_pm/reset_status'


def reset_cause(text, missing_reason='not_observed'):
    """Decoded MT6582 RGU WDT_STATUS that the watchdog driver read at probe,
    before its first write: the cause of the reset that started this boot.
    Zero, unknown bits or an absent file never prove a power-on reset."""
    fields = dict(re.findall(r'(\w+)=(\S+)', text or ''))
    if fields.get('valid') != '1':
        return {'reset_cause': None, 'reset_cause_reason': missing_reason, 'reset_status_raw': None}
    cause = fields.get('cause')
    if cause in ('watchdog_timeout', 'software_reset', 'watchdog_irq', 'debug_reset', 'spm_watchdog'):
        return {'reset_cause': cause, 'reset_cause_reason': 'rgu_wdt_status_at_probe',
                'reset_status_raw': fields.get('raw')}
    return {'reset_cause': None, 'reset_cause_reason': 'rgu_status_' + str(cause or 'unreadable'),
            'reset_status_raw': fields.get('raw')}


def loader_boot(text):
    """F56: how LK says this boot started. The RGU status is cleared before
    Linux reads it (0x0 in every report), but a SoC reset (the watchdog, a
    restart) keeps the PMIC on: boot_reason 3-6 is warm, anything else cold.
    After a previous boot without shutdown_complete, warm means the watchdog
    or a kernel restart ended it, cold that power was cut or Power was held."""
    from .battery import WARM_BOOT_REASONS
    fields = dict(f.split('=', 1) for f in (text or '').split() if '=' in f)
    if fields.get('valid') != '1' or not fields.get('boot_reason', '').isdecimal():
        return {'boot_reason': None, 'reset': None}
    reason = int(fields['boot_reason'])
    return {'boot_reason': reason, 'reset': 'warm' if reason in WARM_BOOT_REASONS else 'cold'}


PMIC_POWER_STATUS = '/sys/firmware/y2_pmic/power_status'
# TOP_RST_MISC PWRKEY_RST_TD, as the vendor keypad header (KPD_PMIC_LPRST_TD)
# names its selectors; not timed on a Y2.
LONG_PRESS_SECONDS = (8, 11, 14, 5)


def pmic_power_status(text, earlier=None):
    """F56/F57: the MT6323's record of the last power-down (kernel
    pmic-lowpower-policy.h), as the loaders left it. LK clears a latched
    Power long-press reset by setting STRUP_CON8 bit 4 and leaves it set,
    so a set bit may stay from an earlier boot: only a change from the
    previous boot's record (`earlier`) proves a long-press reset now.
    A clear bit rules one out; brownout and a dead battery stay unrecorded."""
    fields = dict(re.findall(r'(\w+)=(\S+)', text or ''))
    regs = {}
    for name, value in fields.items():
        if name != 'valid' and re.fullmatch(r'0x[0-9a-fA-F]{1,4}', value):
            regs[name] = int(value, 16)
    if fields.get('valid') != '1' or 'strup_con8' not in regs:
        return {'state': 'Unavailable', 'long_press_reset': None,
                'long_press_reset_reason': 'not_observed'}
    flag = bool(regs['strup_con8'] & 0x0010)
    if not flag:
        verdict, reason = False, 'no_long_press_record'
    elif earlier is False:
        verdict, reason = True, 'lk_cleared_long_press_reset'
    else:
        verdict, reason = None, ('record_set_before' if earlier else 'first_record')
    result = {'state': 'Ready', 'raw': {name: '0x%04x' % value for name, value in regs.items()},
              'lk_long_press_record': flag, 'long_press_reset': verdict,
              'long_press_reset_reason': reason,
              'just_pwrkey_rst': bool(regs['strup_con8'] & 0x4000)}
    misc = regs.get('top_rst_misc')
    if misc is not None:
        result['long_press_reset_enabled'] = bool(misc & 0x0040)
        result['long_press_reset_seconds'] = LONG_PRESS_SECONDS[(misc >> 8) & 3]
    if 'chr_con16' in regs:
        result['uvlo_selector'] = regs['chr_con16'] & 3
    if 'strup_con5' in regs:
        result['thermal_status'] = (regs['strup_con5'] >> 8) & 7
    if 'chrstatus' in regs:
        result['power_key_down'] = not regs['chrstatus'] & 0x0002
    return result


def transition(previous, boot_id, stage, identity, now_ns, reset=None):
    if not boot_id:
        raise ValueError('boot_id_unavailable')
    if not isinstance(previous, dict) or previous.get('schema') != 1:
        previous = {}
    same = previous.get('boot_id') == boot_id
    count = previous.get('boot_count', 0)
    if not isinstance(count, int) or count < 0:
        count = 0
    unclean = previous.get('consecutive_unclean', 0)
    if not isinstance(unclean, int) or unclean < 0:
        unclean = 0
    if not same:
        unclean = min(1000, unclean + 1) if previous and not previous.get('orderly_shutdown') else 0
    history = previous.get('history', []) if isinstance(previous.get('history'), list) else []
    if previous and not same:
        history = (history + [{k: previous.get(k) for k in
                              ('boot_id', 'last_stage', 'orderly_shutdown', 'identity')}])[-8:]
    return {'schema': 1, 'boot_id': boot_id, 'boot_count': count + (not same),
            'previous_boot_id': previous.get('previous_boot_id') if same else previous.get('boot_id'),
            'last_stage': stage, 'stage_monotonic_ns': now_ns,
            'orderly_shutdown': stage == 'shutdown_complete',
            'consecutive_unclean': unclean, 'recovery_recommended': unclean >= 3,
            **(reset or reset_cause(None)),
            'history': history, 'identity': identity}


def mark(ctx, stage):
    if stage not in ('platform_start', 'services_started', 'application_ready', 'shutdown_pending', 'shutdown_complete'):
        raise ValueError('unknown_boot_stage')
    if ctx.read('/data/.y2data-schema') != '1':
        raise ValueError('data_schema_unavailable')
    from .observe import mountinfo
    mounted = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/data']
    if len(mounted) != 1 or mounted[0]['source'] != ctx.read('/run/y2-data-device'):
        raise ValueError('verified_data_mount_unavailable')
    path = private_directory(ctx.path('/data/system/platform')) / 'boot.json'
    import fcntl
    fd = os.open(path.parent / '.boot.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        value = transition(ctx.json('/data/system/platform/boot.json'),
                           ctx.read('/proc/sys/kernel/random/boot_id'), stage,
                           ctx.json('/etc/y2linux/versions.json'), time.monotonic_ns(),
                           reset_cause(ctx.read(RESET_STATUS)))
        atomic_json(path, value, durable=True)
        runtime = ctx.path('/run/y2/boot-stages.jsonl')
        runtime.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        import json
        fd_stage = os.open(runtime, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            if os.fstat(fd_stage).st_size < 8192:
                os.write(fd_stage, (json.dumps({'stage': stage, 'monotonic_s': time.monotonic()}) + '\n').encode())
        finally:
            os.close(fd_stage)
        # The reset-surviving stage trail of init and rcS (F23); best effort.
        try:
            fd_pmsg = os.open(ctx.path('/dev/pmsg0'), os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        except OSError:
            pass
        else:
            try:
                os.write(fd_pmsg, ('%.2f platform %s\n' % (time.monotonic(), stage)).encode())
            except OSError:
                pass
            finally:
                os.close(fd_pmsg)
        if stage == 'application_ready':
            from .power_features import apply
            apply(ctx)
        return value
    finally:
        os.close(fd)


def capture_previous(ctx):
    """Snapshot existing bounded syslog before this boot's logging starts.

    No new logger/poll loop, retained-RAM allocation or guessed reset register.
    A panic may stop userspace before logging; absence of a marker proves nothing.
    """
    import hashlib
    import json
    if ctx.read('/data/.y2data-schema') != '1':
        raise ValueError('data_schema_unavailable')
    from .observe import mountinfo
    mounts = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/data']
    if len(mounts) != 1 or mounts[0]['source'] != ctx.read('/run/y2-data-device'):
        raise ValueError('verified_data_mount_unavailable')
    root = private_directory(ctx.path('/data/system/platform'))
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if not boot:
        raise ValueError('boot_id_unavailable')
    previous = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if previous.get('captured_on_boot_id') == boot:
        return previous
    journal = ctx.json('/data/system/platform/boot.json', {})
    if journal.get('boot_id') == boot:
        raise ValueError('previous_boot_capture_missed_early_boundary')
    result = {'schema': 1, 'captured_on_boot_id': boot, 'previous_boot_id': journal.get('boot_id'),
              'previous_last_stage': journal.get('last_stage'), 'previous_orderly_shutdown': journal.get('orderly_shutdown'),
              'reset_cause': None, 'reset_cause_reason': 'no_qualified_retained_register_or_pstore_backend',
              'kernel_panic_retention_guaranteed': False, 'logs_are_private': True,
              'reset_status_raw': None,
              'record': ctx.record('previous-boot-evidence'), 'logs': []}
    if ctx.read(RESET_STATUS) is not None:
        result.update(reset_cause(ctx.read(RESET_STATUS)))
    from .battery import BOOT_METADATA
    result['loader'] = loader_boot(ctx.read(BOOT_METADATA))
    earlier = previous.get('pmic') if isinstance(previous.get('pmic'), dict) else {}
    result['pmic'] = pmic_power_status(ctx.read(PMIC_POWER_STATUS), earlier.get('lk_long_press_record'))
    from .boot_retained import capture
    try:
        result['retained'] = capture(ctx, boot, journal.get('boot_id'))
    except (OSError, ValueError) as error:
        result['retained'] = {'error': str(error), 'source_entries_deleted': False}
    data = os.open(ctx.path('/data'), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    logs = None
    try:
        try:
            logs = os.open('logs', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=data)
        except FileNotFoundError:
            logs = None
        if logs is not None:
            if os.fstat(logs).st_dev != os.fstat(data).st_dev:
                raise ValueError('log_submount_refused')
            for name in ('system.log.0', 'system.log'):
                try:
                    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=logs)
                except FileNotFoundError:
                    continue
                with os.fdopen(fd, 'rb') as stream:
                    info = os.fstat(stream.fileno())
                    if not stat.S_ISREG(info.st_mode):
                        raise ValueError('invalid_log_file')
                    stream.seek(max(0, info.st_size - 65536))
                    raw = stream.read(65536)
                lines = [line[:500] for line in raw.decode(errors='replace').splitlines()[-32:]]
                result['logs'].append({'name': name, 'tail_sha256': hashlib.sha256(raw).hexdigest(),
                                       'source_bytes_at_capture': info.st_size, 'tail_lines': lines,
                                       'bounded_tail_not_complete_log': True})
        while len(json.dumps(result).encode()) > 96000:
            largest = max(result['logs'], key=lambda item: len(item['tail_lines']))
            largest['tail_lines'].pop(0)
        atomic_json(root/'previous-boot-evidence.json', result, durable=True)
        return result
    finally:
        if logs is not None: os.close(logs)
        os.close(data)


def evidence_status(ctx):
    value = ctx.json('/data/system/platform/previous-boot-evidence.json', {})
    if not value:
        return {'state': 'Unavailable', 'reason': 'no_previous_boot_record'}
    return {k: value.get(k) for k in ('captured_on_boot_id', 'previous_boot_id',
            'previous_last_stage', 'previous_orderly_shutdown', 'reset_cause',
            'reset_cause_reason', 'reset_status_raw', 'kernel_panic_retention_guaranteed')} | {
            'private_record': '/data/system/platform/previous-boot-evidence.json',
            'retained': value.get('retained'), 'pmic': value.get('pmic'), 'loader': value.get('loader'),
            'log_tails': [{k: log.get(k) for k in ('name','tail_sha256','source_bytes_at_capture')}
                          for log in value.get('logs',[])]}
