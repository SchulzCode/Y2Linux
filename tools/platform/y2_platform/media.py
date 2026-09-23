"""Single explicit SD lifecycle, guarded by controller and filesystem identity."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import json
import os
from .common import atomic_json, read
from .observe import mountinfo


def candidates(ctx):
    result = []
    for entry in ctx.glob('/sys/class/block/*'):
        if '/11240000.mmc/' not in str(entry.resolve()):
            continue
        if not (entry / 'partition').is_file() and list(entry.glob('*/partition')):
            continue
        answer = ctx.command(['blkid', '-p', '-o', 'export', '/dev/' + entry.name])
        if not answer['ok']:
            continue
        props = dict(line.split('=', 1) for line in answer['output'].splitlines() if '=' in line)
        if props.get('TYPE') not in ('ext4', 'vfat', 'exfat') or not props.get('UUID'):
            continue
        device = entry.resolve()
        if (entry / 'partition').is_file():
            device = device.parent
        result.append({'device': '/dev/' + entry.name, 'filesystem': props['TYPE'],
                       'uuid': props['UUID'], 'cid': read(device / 'device/cid')})
    return result


def mounts(ctx):
    return [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/media/sd']


def operation(ctx, action):
    if action not in ('mount', 'unmount', 'check', 'status'):
        raise ValueError('unknown_media_action')
    if action == 'status':
        from .observe import storage
        return storage(ctx)['volumes'][2]
    runtime = ctx.path('/run/y2')
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = os.open(runtime / 'media.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = {'schema': 1, 'action': action, 'state': 'Starting', 'reason': None,
                 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id')}
        old = mounts(ctx)
        if old and any('/11240000.mmc/' not in str(ctx.path('/sys/dev/block/' + m['device_id']).resolve()) for m in old):
            state.update(state='Failed', reason='mounted_source_not_sd')
            atomic_json(runtime / 'media.json', state)
            return state
        if action == 'unmount':
            if len(old) > 1:
                state.update(state='Failed', reason='ambiguous_mount_stack')
            elif old:
                result = ctx.command(['umount', '/media/sd'], timeout=3)
                if not result['ok'] or mounts(ctx):
                    state.update(state='Failed', reason=result['reason'] or 'unmount_busy')
                else:
                    state.update(state='Unavailable', reason='ejected')
            else:
                state.update(state='Unavailable', reason='not_mounted')
        elif old:
            # Never check/repair an active filesystem or stack a new card on it.
            state.update(state='Failed', reason='already_mounted' if action == 'check' else 'mount_requires_unmounted_path')
        else:
            available = candidates(ctx)
            if len(available) != 1:
                state.update(state='Unavailable', reason='no_unique_supported_filesystem')
            else:
                card = available[0]
                state.update(card)
                if action == 'check':
                    argv = {'ext4': ['e2fsck', '-f', '-n'], 'vfat': ['fsck.fat', '-n'],
                            'exfat': ['fsck.exfat', '-n']}[card['filesystem']]
                    result = ctx.command(argv + [card['device']], timeout=60)
                    state.update(state='Ready' if result['ok'] else 'Degraded',
                                 reason='read_only_check_passed' if result['ok'] else result['reason'])
                else:
                    target = ctx.path('/media/sd')
                    if target.is_symlink():
                        raise ValueError('mountpoint_symlink')
                    target.mkdir(parents=True, exist_ok=True)
                    options = 'rw,nosuid,nodev,noexec'
                    if card['filesystem'] in ('vfat', 'exfat'):
                        options += ',fmask=0177,dmask=0077'
                    result = ctx.command(['mount', '-t', card['filesystem'], '-o', options,
                                          card['device'], '/media/sd'], timeout=3)
                    current = mounts(ctx)
                    if result['ok'] and len(current) == 1 and current[0]['source'] == card['device']:
                        state.update(state='Ready', mount_id=current[0]['mount_id'],
                                     device_id=current[0]['device_id'])
                        if candidates(ctx) != available:
                            state.update(state='Failed', reason='source_changed_during_mount')
                        else:
                            atomic_json(runtime / 'media-mount.json', state)
                    else:
                        state.update(state='Failed', reason=result['reason'] or 'mount_identity_failed')
        atomic_json(runtime / 'media.json', state)
        return state
    finally:
        os.close(lock)
