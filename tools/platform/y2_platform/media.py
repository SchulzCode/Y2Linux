"""Single explicit SD lifecycle, guarded by controller and filesystem identity."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import json
import os
import hashlib
import re
import struct
import time
from .common import atomic_json, read
from .observe import mountinfo, device_instance


def media_identity(entry, filesystem, uuid):
    """CID/partition geometry supplies identity when a legal FAT UUID is absent."""
    device = entry.resolve()
    partition = read(entry / 'partition')
    if partition is not None:
        device = device.parent
    cid = read(device / 'device/cid')
    geometry = {'partition': partition, 'start': read(entry / 'start') if partition is not None else '0',
                'size': read(entry / 'size')}
    if not uuid and (not cid or any(geometry[k] is None or not geometry[k].isdigit()
                                  for k in ('start', 'size'))):
        return None
    identity = dict(cid=cid, geometry=geometry, filesystem=filesystem, uuid=uuid)
    identity['id'] = hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
    return identity


# fsck.fat -n exits 1 whenever it would change anything, including only the
# dirty bit a FAT card keeps after it was pulled out while mounted (Android,
# Windows and macOS mount such a card; issue #44). Windows also mounts a card
# whose free-cluster count (FSInfo) is stale, whose volume label differs
# between the root directory and the boot sector (Windows renames only the
# former), or with unused clusters still marked used; the Linux FAT driver
# recounts free space and none of them touches a file (Reddit: "needs repair"
# on a card the PC read; F12). Each such line has a class; any other line
# (file chains, directory entries, sizes, names) is a real finding and keeps
# the card out.
FAT_LINES = [(cls, re.compile(p)) for cls, p in (
    (None, r'fsck\.fat \S+ \(.*\)'),
    (None, r'Leaving filesystem unchanged\.'),
    (None, r'\S+: \d+ files, \d+/\d+ clusters'),
    ('backup', r'There are differences between boot sector and its backup\.'),
    ('backup', r'This is mostly harmless\. Differences: \(offset:original/backup\)'),
    ('backup', r'(?:, )?\d+:[0-9a-f]{2}/[0-9a-f]{2}(?:, \d+:[0-9a-f]{2}/[0-9a-f]{2})*'),
    ('backup', r'Not automatically fixing this\.'),
    ('dirty', r'Dirty bit is set\. Fs was not properly unmounted and some data may be corrupt\.'),
    ('dirty', r'Automatically removing dirty bit\.'),
    ('fsinfo_free', r'Free cluster summary wrong \(\d+ vs\. really \d+\)'),
    ('fsinfo_free', r'Free cluster summary uninitialized \(should be \d+\)'),
    ('label_mismatch', r"Volume label '.*' stored in root directory and label '.*' stored in boot sector "
                       r"and different\."),
    ('label_mismatch', r'Auto-copying volume label from root directory to boot sector\.'),
    # A label written by another system's tools: invalid characters, lowercase.
    ('label_mismatch', r"Volume label '.*' stored in root directory is not valid\."),
    ('label_mismatch', r"Volume label '.*' stored in root directory contains lowercase characters\."),
    ('lost_clusters', r'Reclaimed \d+ unused clusters? \(\d+ bytes\)\.'),
)]
# The answer line a finding prints next; accepted only right after its finding.
FAT_FOLLOW = {'Auto-correcting.': 'fsinfo_free', 'Auto-removing label.': 'label_mismatch',
              'Auto-changing lowercase characters to uppercase': 'label_mismatch'}
# The boot sector's own dirty flag (offset 37 on FAT12/16, 65 on FAT32) also
# differs from its backup on a card pulled while mounted.
FAT_DIRTY_OFFSETS = {'37', '65'}


def fat_findings(output):
    """The classes a `fsck.fat -n` run reported, sorted; 'other' for any line
    that is not a benign class. Never any file or label text."""
    found, offsets, previous = set(), set(), None
    for line in (output or '').splitlines():
        line = line.strip()
        if not line:
            continue
        if FAT_FOLLOW.get(line) is not None and FAT_FOLLOW[line] == previous:
            continue
        cls = next((c for c, p in FAT_LINES if p.fullmatch(line)), 'other')
        if cls == 'backup' and ':' in line and '/' in line:
            offsets.update(item.split(':')[0] for item in re.findall(r'\d+:[0-9a-f]{2}/[0-9a-f]{2}', line))
        if cls not in (None, 'backup'):
            found.add(cls)
        previous = cls
    if offsets - FAT_DIRTY_OFFSETS:
        found.add('backup_differs')
    return sorted(found)


def fat_benign_only(output):
    """True when a failed `fsck.fat -n` reported only findings Windows mounts
    over: an unclean unmount, a stale free count, a label mismatch, unused
    clusters, a differing backup boot sector."""
    found = fat_findings(output)
    return bool(found) and 'other' not in found


def read_only_check(ctx, card, force=False):
    """The admission check, never a repair. The record keeps no checker
    output from a failed run (it can name files); benign findings pass with
    `finding`, and every run names its classes in `findings`."""
    argv = {'ext4': ['e2fsck', '-f', '-n'] if force else ['e2fsck', '-n'],
            'vfat': ['fsck.fat', '-n'], 'exfat': ['fsck.exfat', '-n']}[card['filesystem']]
    started = time.monotonic()
    result = ctx.command(argv + [card['device']], timeout=60,
                         failure_output=card['filesystem'] == 'vfat')
    check = {'ok': result['ok'], 'reason': result['reason'],
             'output': result['output'] if result['ok'] else None,
             'elapsed_ms': int((time.monotonic() - started) * 1000)}
    if result['reason'] == 'command_timeout':
        check['findings'] = ['timeout']
    elif not result['ok'] and result['reason'] == 'command_failed' and card['filesystem'] == 'vfat':
        check['findings'] = fat_findings(result['output'])
        if fat_benign_only(result['output']):
            # 'unclean_unmount' is the #44 name for the dirty bit alone.
            check.update(ok=True, finding='unclean_unmount' if check['findings'] == ['dirty'] else 'benign')
    elif not result['ok']:
        check['findings'] = ['other']
    return check


# Why the last cards were refused or let in, across boots (F12: a card refused
# at every insertion left no reason in a report). No checker output, no names.
ADMISSIONS = '/data/system/platform/sd-admissions.json'
ADMISSIONS_KEPT = 10


def remember_admission(ctx, card, check):
    try:
        old = ctx.json(ADMISSIONS, {}) or {}
        entries = [e for e in old.get('admissions', []) if isinstance(e, dict)] if isinstance(old, dict) else []
        entries.append({'time': int(time.time()), 'card': card['identity']['id'][:12],
                        'filesystem': card['filesystem'], 'ok': bool(check.get('ok')),
                        'reason': check.get('reason'), 'finding': check.get('finding'),
                        'findings': check.get('findings', []), 'elapsed_ms': check.get('elapsed_ms')})
        directory = ctx.path(ADMISSIONS).parent
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        atomic_json(ctx.path(ADMISSIONS), {'schema': 1, 'admissions': entries[-ADMISSIONS_KEPT:]})
    except (OSError, ValueError, KeyError, TypeError):
        pass


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
        if props.get('TYPE') not in ('ext4', 'vfat', 'exfat'):
            continue
        identity = media_identity(entry, props['TYPE'], props.get('UUID'))
        instance = device_instance(ctx, read(entry / 'dev') or '')
        if identity is None or instance is None:
            continue
        result.append({'device': '/dev/' + entry.name, 'filesystem': props['TYPE'],
                       'uuid': props.get('UUID'), 'cid': identity['cid'], 'identity': identity,
                       'source_instance': instance})
    return result


def mounts(ctx):
    return [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/media/sd']


def owned_mount(ctx, mounted):
    device = ctx.path('/sys/dev/block/' + mounted['device_id'])
    if device.exists():
        return '/11240000.mmc/' in str(device.resolve())
    claim = ctx.json('/run/y2/media-mount.json', {})
    # Removed cards no longer have a sysfs controller path. The retained claim
    # permits only the exact mount we created, never an arbitrary mountpoint.
    return (claim.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
            claim.get('mount_id') == mounted['mount_id'] and
            claim.get('device_id') == mounted['device_id'] and
            claim.get('device') == mounted['source'])


def inventory(ctx):
    """SD presence by stable kernel identity, never by kernfs inode/ctime.

    Page-cache drops and ordinary memory pressure reclaim kernfs inodes, so a
    recreated inode is not a new card. dev_t and the disk sequence number stay
    fixed for one gendisk; every re-enumeration allocates a new diskseq, which
    also distinguishes a reinserted identical card (same CID)."""
    result = []
    for entry in ctx.glob('/sys/class/block/mmcblk*'):
        try:
            device = entry.resolve()
            if '/11240000.mmc/' in str(device):
                disk = device.parent if read(entry / 'partition') is not None else device
                result.append((entry.name, read(entry / 'dev'), read(disk / 'diskseq'),
                               read(disk / 'device/cid')))
        except OSError:
            pass
    return result


def reconcile(ctx):
    """Insertion/removal work; no automatic retry storm or remount after eject."""
    runtime = ctx.path('/run/y2')
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    current = [list(v) for v in inventory(ctx)]
    previous = ctx.json('/run/y2/media-lifecycle.json', {})
    if previous.get('inventory') == current and not previous.get('retry'):
        return previous
    attempts = previous.get('attempts', 0) if previous.get('inventory') == current else 0
    attempts = attempts if isinstance(attempts, int) else 0
    result = {'schema': 1, 'inventory': current, 'attempts': attempts + 1, 'retry': False}
    if mounts(ctx):
        result.update(operation(ctx, 'unmount'))
        if mounts(ctx):
            result.update(retry=attempts < 4, reason='source_change_unmount_failed')
            atomic_json(runtime / 'media-lifecycle.json', result)
            return result
    if current:
        result.update(operation(ctx, 'mount'))
    else:
        result.update(state='Unavailable', reason='no_card')
    atomic_json(runtime / 'media-lifecycle.json', result)
    return result


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
        if old and any(not owned_mount(ctx, m) for m in old):
            state.update(state='Failed', reason='mounted_source_not_sd')
            atomic_json(runtime / 'media.json', state)
            return state
        if action == 'unmount':
            if len(old) > 1:
                state.update(state='Failed', reason='ambiguous_mount_stack')
            elif old:
                from .usb import release_sd
                if not release_sd(ctx, old[0]['mount_id']):
                    state.update(state='Failed', reason='usb_transfer_connected_or_busy')
                    atomic_json(runtime / 'media.json', state)
                    return state
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
                    result = read_only_check(ctx, card, force=True)
                    state['filesystem_check'] = result
                    state.update(state='Ready' if result['ok'] else 'Degraded',
                                 reason='read_only_check_passed' if result['ok'] else result['reason'])
                else:
                    # Read-only admission never authorizes checker repairs.
                    # Large/unhealthy media may time out;
                    # the owner must see that failure rather than a RW mount.
                    result = read_only_check(ctx, card)
                    state['filesystem_check'] = result
                    remember_admission(ctx, card, result)
                    if not result['ok']:
                        state.update(state='Degraded', reason='filesystem_check_failed')
                        atomic_json(runtime / 'media.json', state)
                        return state
                    if candidates(ctx) != available:
                        state.update(state='Failed', reason='source_changed_during_check')
                        atomic_json(runtime / 'media.json', state)
                        return state
                    target = ctx.path('/media/sd')
                    if target.is_symlink():
                        raise ValueError('mountpoint_symlink')
                    target.mkdir(parents=True, exist_ok=True)
                    # POWER-FINAL-30: noatime on every card format. relatime still
                    # wrote an access time the first time each song was read every
                    # day: a metadata write to the card during playback.
                    options = 'rw,nosuid,nodev,noexec,noatime'
                    if card['filesystem'] in ('vfat', 'exfat'):
                        options += ',fmask=0177,dmask=0077'
                    if card['filesystem'] == 'vfat':
                        options += ',utf8=1'
                    result = ctx.command(['mount', '-t', card['filesystem'], '-o', options,
                                          card['device'], '/media/sd'], timeout=3)
                    current = mounts(ctx)
                    if result['ok'] and len(current) == 1 and current[0]['source'] == card['device']:
                        state.update(state='Ready', mount_id=current[0]['mount_id'],
                                     device_id=current[0]['device_id'],
                                     source_instance=device_instance(ctx, current[0]['device_id']))
                        if candidates(ctx) != available:
                            state.update(state='Failed', reason='source_changed_during_mount')
                        else:
                            atomic_json(runtime / 'media-mount.json', state)
                            # The kernel recycles mount ids, so a release marker for a card that was
                            # ejected or reformatted must not hide the next mount that reuses its id.
                            try:
                                ctx.path('/run/y2/usb-release-sd.json').unlink()
                            except FileNotFoundError:
                                pass
                    else:
                        state.update(state='Failed', reason=result['reason'] or 'mount_identity_failed')
        atomic_json(runtime / 'media.json', state)
        return state
    finally:
        os.close(lock)


# ---------------------------------------------------------------------------
# Explicit owner operations on the removable card: format and grow.
# Never automatic: a card with a foreign or undersized filesystem is reported,
# and only a command that names the card by CID may change it.

MIN_CARD_SECTORS = 16384          # 8 MiB; anything smaller is not a storage card
PARTITION_START = 2048            # 1 MiB alignment
WIPE_TAIL_SECTORS = 64            # a stale GPT backup header lives in the last sectors
BLKRRPART = 0x125F


def sd_disk(ctx):
    """The unique whole-disk entry on the removable-card controller, else None."""
    found = []
    for entry in ctx.glob('/sys/class/block/mmcblk*'):
        try:
            real = str(entry.resolve())
        except OSError:
            continue
        if '/11240000.mmc/' in real and read(entry / 'partition') is None:
            found.append(entry)
    return found[0] if len(found) == 1 else None


def mbr_for(sectors):
    """One Linux partition from sector 2048 to the end of the card."""
    size = min(sectors - PARTITION_START, 0xFFFFFFFF)
    entry = struct.pack('<B3sB3sII', 0, b'\xfe\xff\xff', 0x83, b'\xfe\xff\xff', PARTITION_START, size)
    return bytes(446) + entry + bytes(48) + b'\x55\xaa'


def reread_partitions(ctx, fd):
    return getattr(ctx, 'ioctl', fcntl.ioctl)(fd, BLKRRPART)


def detach_for_change(ctx, state):
    """Release the card from MTP and unmount it, for an explicit owner command."""
    old = mounts(ctx)
    if not old:
        return True
    if len(old) > 1 or any(not owned_mount(ctx, m) for m in old):
        state.update(state='Failed', reason='ambiguous_or_foreign_mount')
        return False
    from .usb import release_sd
    if not release_sd(ctx, old[0]['mount_id'], explicit=True):
        state.update(state='Failed', reason='usb_release_failed')
        return False
    result = ctx.command(['umount', '/media/sd'], timeout=10)
    if not result['ok'] or mounts(ctx):
        state.update(state='Failed', reason=result['reason'] or 'unmount_busy')
        return False
    return True


def change_card(ctx, action, confirm_cid=None):
    """action: 'format' (new table + ext4 over the whole card) or 'grow' (ext4 to the partition)."""
    if action not in ('format', 'grow'):
        raise ValueError('unknown_media_change')
    state = apply_change(ctx, action, confirm_cid)
    if state['state'] == 'Starting':
        # Mount through the normal admission path, after the change released the media lock.
        state.update(operation(ctx, 'mount'))
        state['action'] = action
    return state


def apply_change(ctx, action, confirm_cid):
    runtime = ctx.path('/run/y2')
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = os.open(runtime / 'media.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = {'schema': 1, 'action': action, 'state': 'Starting', 'reason': None,
                 'boot_id': ctx.read('/proc/sys/kernel/random/boot_id')}
        disk = sd_disk(ctx)
        cid = read(disk / 'device/cid') if disk else None
        sectors = read(disk / 'size') if disk else None
        if disk is None or not cid or not sectors or not sectors.isdigit():
            state.update(state='Unavailable', reason='no_unique_card')
        elif action == 'format' and confirm_cid != cid:
            # The owner must name the card: a wrong or swapped card is never formatted.
            state.update(state='Failed', reason='cid_confirmation_mismatch', cid=cid)
        elif int(sectors) < MIN_CARD_SECTORS:
            state.update(state='Failed', reason='card_too_small')
        else:
            device = '/dev/' + disk.name
            state.update(cid=cid, sectors=int(sectors), device=device)
            if detach_for_change(ctx, state):
                if action == 'format':
                    format_steps(ctx, state, device, int(sectors))
                else:
                    grow_steps(ctx, state)
        if state['state'] != 'Starting':
            atomic_json(runtime / 'media.json', state)
        return state
    finally:
        os.close(lock)


def format_steps(ctx, state, device, sectors):
    try:
        fd = os.open(ctx.path(device), os.O_RDWR | os.O_SYNC)
    except OSError:
        state.update(state='Failed', reason='card_not_writable')
        return
    try:
        # The old table, boot sector and a stale GPT backup must not survive.
        zero = bytes(512)
        for sector in list(range(1, PARTITION_START)) + list(range(sectors - WIPE_TAIL_SECTORS, sectors)):
            os.pwrite(fd, zero, sector * 512)
        os.pwrite(fd, mbr_for(sectors), 0)
        os.fsync(fd)
        try:
            reread_partitions(ctx, fd)
        except OSError as error:
            state.update(state='Failed', reason='partition_reread_failed', errno=error.errno)
            return
    except OSError as error:
        state.update(state='Failed', reason='card_write_failed', errno=error.errno)
        return
    finally:
        os.close(fd)
    argv = ['mkfs.ext4', '-F', '-q', '-L', 'Y2SD', '-m', '0', '-E', 'lazy_itable_init=1,lazy_journal_init=1']
    if sectors >= 32 * 1024 * 1024:           # 16 GiB and up: a 128 MiB journal is plenty
        argv += ['-J', 'size=128']
    partition = device + 'p1'
    deadline = time.monotonic() + 5
    while not ctx.path(partition).exists() and time.monotonic() < deadline:
        time.sleep(0.05)          # devtmpfs creates the node as the kernel re-reads the table
    result = ctx.command(argv + [partition], timeout=180)
    if not result['ok']:
        state.update(state='Failed', reason='mkfs_failed:' + str(result['reason']))


def grow_steps(ctx, state):
    available = candidates(ctx)
    if len(available) != 1 or available[0]['filesystem'] != 'ext4':
        state.update(state='Failed', reason='grow_requires_one_ext4_card')
        return
    device = available[0]['device']
    # Only a clean (or safely auto-corrected) filesystem is resized.
    if not ctx.command(['e2fsck', '-f', '-p', device], timeout=300)['ok']:
        state.update(state='Failed', reason='filesystem_check_failed')
        return
    result = ctx.command(['resize2fs', device], timeout=300)
    if not result['ok']:
        state.update(state='Failed', reason='resize_failed:' + str(result['reason']))
