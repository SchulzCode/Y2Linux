"""Bounded observational health; full scratch checks are explicit."""
# SPDX-License-Identifier: GPL-2.0-only
from .observe import snapshot


def check(ctx, full=False):
    status = snapshot(ctx)
    checks = []

    def add(name, state, reason, **details):
        checks.append(dict(name=name, state=state, reason=reason, **details))

    for volume in status['storage']['volumes']:
        name = {'/': 'root', '/data': 'data', '/media/sd': 'sd'}[volume['path']]
        state = {'Ready': 'OK', 'Degraded': 'DEGRADED', 'Failed': 'FAILED'}.get(volume['state'], 'UNAVAILABLE')
        if volume['space_state'] in ('LowSpace', 'CriticalSpace'):
            state = 'DEGRADED'
        add(name, state, volume.get('reason') or volume['space_state'])
    fs_errors = []
    unreadable = []
    error_sources = ctx.glob('/sys/fs/ext4/*/errors_count')
    for path in error_sources:
        from .common import number, read
        count = number(read(path))
        if count is None or count < 0:
            unreadable.append(path.parent.name)
        elif count:
            fs_errors.append({'filesystem': path.parent.name, 'errors': count})
    available = bool(error_sources) and not unreadable
    add('filesystem_errors', 'FAILED' if fs_errors else ('OK' if available else 'UNAVAILABLE'),
        'kernel_reported_errors' if fs_errors else ('no_reported_ext4_errors' if available else 'counter_unavailable'),
        errors=fs_errors, unreadable=unreadable)
    for name, present in (
        ('render_node', bool(ctx.glob('/dev/dri/renderD*'))),
        ('display', bool(ctx.glob('/dev/dri/card*'))),
        ('audio', bool(ctx.glob('/dev/snd/pcm*p'))),
        ('input', bool(ctx.glob('/dev/input/event*'))),
        ('rtc', bool(status['system']['rtc'])),
        ('thermal', any(z['temperature_millicelsius'] is not None for z in status['thermal']['zones'])),
        ('power_supply', bool(status['power']['supplies'])),
        ('usb', bool(status['system']['usb']['udcs'])),
    ):
        add(name, 'OK' if present else 'UNAVAILABLE', 'observed' if present else 'interface_missing')
    for name in ('wifi', 'bluetooth', 'update'):
        item = status['readiness'][name]
        add(name, {'Ready': 'OK', 'Starting': 'DEGRADED', 'Degraded': 'DEGRADED',
                   'Failed': 'FAILED', 'Unavailable': 'UNAVAILABLE'}[item['state']], item['reason'])
    if full:
        # Queries only: no render master takeover, ALSA playback, radio action or reboot.
        for name, argv in [('alsa_query', ['y2-audio-contract']), ('render_query', ['modetest', '-M', 'mediatek', '-c'])]:
            answer = ctx.command(argv, timeout=2)
            add(name, 'OK' if answer['ok'] else 'UNAVAILABLE', answer['reason'] or 'query_completed')
    priority = ('FAILED', 'DEGRADED', 'UNAVAILABLE', 'OK')
    state = next(s for s in priority if any(c['state'] == s for c in checks))
    record = ctx.record('health', {'mode': 'full' if full else 'quick'})
    record.update(result=state, failure=[c['name'] for c in checks if c['state'] == 'FAILED'])
    return {'schema': 'org.y2linux.health/v1', 'record': record, 'state': state, 'checks': checks}
