"""Internal user media: the eMMC partition that the stock firmware used as its storage (p8).

Y2Media is ext4, label Y2MEDIA, fixed UUID, at stock logical sector 3742720 for 11460608
sectors, on the internal controller. It is mounted at /media/internal and carries only the music
library (Reborn) and what the owner copies over MTP/SFTP. System state stays on Y2DATA (/data).

Mounting only ever accepts that exact identity. Creating the filesystem replaces only the stock
FAT storage: either the explicit `provision` (names its serial) or `setup`, which every player runs
at start (STORAGE-SETUP, docs/planning/INTERNAL-STORAGE-SETUP-PLAN.md). `setup` decides from a
read-only look at the old storage: nothing worth keeping, or a few files (kept on Y2DATA meanwhile
and moved to "Old Player Files"), set up at once; a real library waits for the listener. Every step
is recorded on Y2DATA and resumes after a power loss.
"""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import hashlib
import os
import stat
from .common import atomic_json, read
from .observe import device_instance, mountinfo

PATH = '/media/internal'
MUSIC = PATH + '/music'
LEGACY_MUSIC = '/data/music'
LABEL = 'Y2MEDIA'
UUID = '79324c69-6e75-4801-8000-000000000103'
START = '3742720'
SIZE = '11460608'
CONTROLLER = '/11230000.mmc/'
MARKER = '/run/y2/internal-media.json'
# 30: noatime, no access-time writes during playback. noinit_itable: the eMMC write guard
# refuses WRITE_ZEROES/erase (storage-policy.h), so ext4's background inode-table zeroing
# only failed at every start (`mmc_erase: group start error -30`, V1 code review R6).
OPTIONS = 'rw,nosuid,nodev,noexec,noatime,noinit_itable'
CHUNK = 1 << 20
RESERVE = 64 << 20         # keep this much free after a migration
# Every ext4 setting named, so a later e2fsprogs cannot change the layout: the set the first
# Y2Media (owner's player, 2026-10-10) got from e2fsprogs 1.47 defaults.
FEATURES = ('none,has_journal,ext_attr,resize_inode,dir_index,orphan_file,filetype,extent,64bit,'
            'flex_bg,metadata_csum_seed,sparse_super,large_file,huge_file,dir_nlink,extra_isize,'
            'metadata_csum')
MKFS = ['mkfs.ext4', '-F', '-q', '-L', LABEL, '-U', UUID, '-m', '0', '-J', 'size=64', '-b', '4096',
        '-I', '256', '-i', '16384', '-O', FEATURES, '-E', 'lazy_itable_init=1,lazy_journal_init=1,nodiscard']

# --- automatic setup (STORAGE-SETUP) ---
SETUP = '/data/system/internal-media-setup.json'      # survives formatting p8
KEPT_STAGING = '/data/system/internal-media-kept'      # kept files while p8 is formatted
OLD = '/run/y2/old-storage'                            # the stock FAT, read-only
STATUS = '/run/y2/internal-media-setup.json'
KEPT = 'Old Player Files'
KEEP_LIMIT = 400 * 1000 ** 2
DATA_RESERVE = 150 * 1000 ** 2
# On battery: at least the percentage the plan named, read like the shown one (stock
# oam_init: the PMIC's power-on OCV, latched before the boot load pulls the voltage down).
MIN_PERCENT = 25
SPLASH = '/run/reborn-splash'
# Start-screen milestones (Reborn's boot table names them; copy, format, move).
SPLASH_PHASES = {0: 'media_setup', 10: 'media_setup_10', 20: 'media_setup_20', 30: 'media_setup_30',
                 40: 'media_setup_40', 50: 'media_setup_50', 60: 'media_setup_60', 70: 'media_setup_70',
                 80: 'media_setup_80', 90: 'media_setup_90', 100: 'media_setup_100'}
# Stock leftovers are never kept; everything else on the old storage is the listener's.
LEFTOVER_DIRS = {'.android_secure', '.thumbnails', 'system volume information', '.spotlight-v100',
                 '.trashes', '.fseventsd', '.temporaryitems'}
LEFTOVER_FILES = {'.nomedia', '.ds_store', 'thumbs.db', 'desktop.ini'}
LEFTOVER_TREES = ('android/data/com.android.providers.media/albumthumbs',)


def partition(ctx):
    """The one internal partition with the Y2Media geometry, else None."""
    found = []
    for entry in ctx.glob('/sys/class/block/mmcblk*p*'):
        try:
            real = str(entry.resolve())
        except OSError:
            continue
        if (CONTROLLER in real and read(entry / 'partition') is not None and
                read(entry / 'start') == START and read(entry / 'size') == SIZE):
            found.append(entry)
    return found[0] if len(found) == 1 else None


def probe(ctx, entry):
    answer = ctx.command(['blkid', '-p', '-o', 'export', '/dev/' + entry.name])
    if not answer['ok']:
        return {}
    return dict(line.split('=', 1) for line in (answer['output'] or '').splitlines() if '=' in line)


def valid(props):
    return props.get('TYPE') == 'ext4' and props.get('LABEL') == LABEL and props.get('UUID') == UUID


def mounts(ctx):
    return [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == PATH]


def record(ctx, state, reason=None, **fields):
    value = {'schema': 1, 'state': state, 'reason': reason, 'path': PATH,
             'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'), **fields}
    ctx.path('/run/y2').mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_json(ctx.path(MARKER), value)
    return value


def lock_runtime(ctx):
    runtime = ctx.path('/run/y2')
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = os.open(runtime / 'internal-media.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(lock)
        raise
    return lock


def ready(ctx):
    """True only while the recorded mount is still the live, exact Y2Media mount."""
    claim = ctx.json(MARKER, {})
    current = mounts(ctx)
    return (claim.get('state') == 'Ready' and len(current) == 1 and
            claim.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
            claim.get('mount_id') == current[0]['mount_id'] and
            claim.get('device_id') == current[0]['device_id'])


def music_root(ctx):
    """Where internal music lives now: Y2Media when mounted, the legacy folder otherwise."""
    return MUSIC if ready(ctx) else LEGACY_MUSIC


def mount(ctx):
    lock = lock_runtime(ctx)
    try:
        return _mount(ctx)
    finally:
        os.close(lock)


def _mount(ctx):
    entry = partition(ctx)
    if entry is None:
        return record(ctx, 'Unavailable', 'partition_geometry_not_found')
    props = probe(ctx, entry)
    if not valid(props):
        return record(ctx, 'Unavailable', 'filesystem_identity_not_y2media',
                      filesystem=props.get('TYPE'), label=props.get('LABEL'))
    current = mounts(ctx)
    if current:
        same = len(current) == 1 and current[0]['source'] == '/dev/' + entry.name
        return record(ctx, 'Ready' if same else 'Failed', None if same else 'foreign_mount_at_path',
                      mount_id=current[0]['mount_id'], device_id=current[0]['device_id'], uuid=UUID)
    device = '/dev/' + entry.name
    # Same policy as Y2DATA in the initramfs: only safe automatic repairs (journal replay etc).
    check = ctx.command(['e2fsck', '-p', device], timeout=180)
    if not check['ok']:
        return record(ctx, 'Degraded', 'filesystem_check_failed')
    ctx.path(PATH).mkdir(parents=True, exist_ok=True)
    result = ctx.command(['mount', '-t', 'ext4', '-o', OPTIONS, device, PATH], timeout=20)
    current = mounts(ctx)
    if not result['ok'] or len(current) != 1 or current[0]['source'] != device:
        return record(ctx, 'Failed', result['reason'] or 'mount_identity_failed')
    return record(ctx, 'Ready', mount_id=current[0]['mount_id'], device_id=current[0]['device_id'],
                  uuid=UUID, source_instance=device_instance(ctx, current[0]['device_id']))


def unmount(ctx):
    lock = lock_runtime(ctx)
    try:
        current = mounts(ctx)
        if not current:
            return record(ctx, 'Unavailable', 'not_mounted')
        result = ctx.command(['umount', PATH], timeout=10)
        if not result['ok'] or mounts(ctx):
            return record(ctx, 'Failed', result['reason'] or 'unmount_busy')
        return record(ctx, 'Unavailable', 'unmounted')
    finally:
        os.close(lock)


def status(ctx):
    entry = partition(ctx)
    props = probe(ctx, entry) if entry else {}
    return {'schema': 1, 'path': PATH, 'partition': entry.name if entry else None,
            'filesystem': props.get('TYPE'), 'label': props.get('LABEL'), 'uuid': props.get('UUID'),
            'identity_valid': valid(props), 'mounted': bool(mounts(ctx)), 'ready': ready(ctx),
            'marker': ctx.json(MARKER, {})}


# --- one-time provisioning ------------------------------------------------------------------

def tree(root):
    """(relative path, size) of every regular file; refuses symlinks and special files."""
    files, directories = [], []
    for current, names, entries in os.walk(root, followlinks=False):
        relative = os.path.relpath(current, root)
        for name in list(names):
            if os.path.islink(os.path.join(current, name)):
                raise ValueError('migration_refuses_symlink:' + os.path.join(relative, name))
        directories.append(relative)
        for name in entries:
            path = os.path.join(current, name)
            info = os.lstat(path)
            if not stat.S_ISREG(info.st_mode):
                raise ValueError('migration_refuses_special_file:' + os.path.join(relative, name))
            files.append((os.path.normpath(os.path.join(relative, name)), info.st_size))
    return files, directories


def digest(path):
    h = hashlib.sha256()
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        try:
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)     # read the media, not the page cache
        except OSError:
            pass
        while True:
            chunk = os.read(fd, CHUNK)
            if not chunk:
                break
            h.update(chunk)
    finally:
        os.close(fd)
    return h.hexdigest()


def copy_verified(source, target, size):
    """Copy to a temporary name, fsync, verify by hash, then publish atomically."""
    if os.path.exists(target):
        if os.path.getsize(target) == size and digest(source) == digest(target):
            return 'present'
        raise ValueError('migration_target_differs:' + target)
    temporary = target + '.part'
    src = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    dst = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o644)
    try:
        while True:
            chunk = os.read(src, CHUNK)
            if not chunk:
                break
            view = memoryview(chunk)
            while view:
                view = view[os.write(dst, view):]
        os.fsync(dst)
    finally:
        os.close(src)
        os.close(dst)
    if digest(source) != digest(temporary):
        os.unlink(temporary)
        raise ValueError('migration_hash_mismatch:' + source)
    os.replace(temporary, target)
    info = os.stat(source)
    os.utime(target, ns=(info.st_atime_ns, info.st_mtime_ns))
    return 'copied'


def migrate(ctx, source=LEGACY_MUSIC, target=MUSIC, progress=None):
    """Move the old music library onto Y2Media. Sources are removed only after everything verified."""
    src, dst = str(ctx.path(source)), str(ctx.path(target))
    if not os.path.isdir(src):
        return {'files': 0, 'bytes': 0, 'copied': 0, 'removed': 0}
    files, directories = tree(src)
    total = sum(size for _, size in files)
    free = os.statvfs(ctx.path(PATH))
    if total + RESERVE > free.f_bavail * free.f_frsize:
        raise ValueError('migration_does_not_fit')
    os.makedirs(dst, mode=0o755, exist_ok=True)
    for relative in directories:
        os.makedirs(os.path.join(dst, relative), mode=0o755, exist_ok=True)
    copied = done = 0
    for relative, size in files:
        if copy_verified(os.path.join(src, relative), os.path.join(dst, relative), size) == 'copied':
            copied += 1
        done += size
        if progress:
            progress(done, total)
    os.sync()
    # Everything on the target is verified: only now remove the originals.
    removed = 0
    for relative, _ in files:
        os.unlink(os.path.join(src, relative))
        removed += 1
    for relative in sorted(directories, key=len, reverse=True):
        if relative != '.':
            try:
                os.rmdir(os.path.join(src, relative))
            except OSError:
                pass
    return {'files': len(files), 'bytes': total, 'copied': copied, 'removed': removed}


def provision(ctx, confirm_uuid, migrate_music=True):
    """Create Y2Media over the stock FAT partition. Names the filesystem it replaces."""
    lock = lock_runtime(ctx)
    try:
        entry = partition(ctx)
        if entry is None:
            return record(ctx, 'Unavailable', 'partition_geometry_not_found')
        props = probe(ctx, entry)
        in_use = mounts(ctx) or any(m['source'] == '/dev/' + entry.name
                                    for m in mountinfo(ctx.read('/proc/self/mountinfo')))
        if in_use and not valid(props):
            # Never replace a mounted filesystem. A finished Y2Media may be re-run to resume
            # an interrupted migration.
            return record(ctx, 'Failed', 'partition_is_mounted')
        if not valid(props):
            if props.get('TYPE') != 'vfat' or not confirm_uuid or props.get('UUID') != confirm_uuid:
                # Only the stock FAT filesystem, named by its serial, may be replaced.
                return record(ctx, 'Failed', 'confirmation_does_not_match_stock_filesystem',
                              filesystem=props.get('TYPE'), serial=props.get('UUID'))
            device = '/dev/' + entry.name
            made = ctx.command(MKFS + [device], timeout=300)
            if not made['ok']:
                return record(ctx, 'Failed', 'mkfs_failed:' + str(made['reason']))
    finally:
        os.close(lock)
    state = mount(ctx)
    if state['state'] != 'Ready':
        return state
    ctx.path(MUSIC).mkdir(mode=0o755, parents=True, exist_ok=True)
    result = dict(state)
    if migrate_music:
        try:
            result['migration'] = migrate(ctx)
        except (OSError, ValueError) as error:
            return record(ctx, 'Degraded', 'migration_failed:' + str(error),
                          mount_id=state.get('mount_id'), device_id=state.get('device_id'), uuid=UUID)
    return result


# --- automatic setup (STORAGE-SETUP) ----------------------------------------------------------

def leftover(relative):
    """A stock leftover (cache, empty marker, computer clutter), never kept."""
    path = relative.replace(os.sep, '/').lower()
    parts = path.split('/')
    return (any(p in LEFTOVER_DIRS for p in parts[:-1]) or parts[-1] in LEFTOVER_FILES or
            parts[-1].startswith('._') or any(path.startswith(t + '/') for t in LEFTOVER_TREES))


def keepable(root):
    """(relative path, size) of the listener's files on the old storage."""
    files, _ = tree(root)
    return [(relative, size) for relative, size in files if not leftover(relative)]


def decide(kept, data_free):
    """1: nothing to keep; 2: few enough to keep on Y2DATA meanwhile; 3: ask the listener."""
    total = sum(size for _, size in kept)
    if not kept:
        return 1
    if total <= KEEP_LIMIT and data_free - total >= DATA_RESERVE:
        return 2
    return 3


def note(ctx, state, reason=None, **fields):
    """This boot's setup state, for Reborn (counts only, never file names)."""
    value = {'schema': 1, 'state': state, 'reason': reason,
             'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'), **fields}
    ctx.path('/run/y2').mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_json(ctx.path(STATUS), value, mode=0o644)
    return value


def setup_status(ctx):
    value = ctx.json(STATUS, None)
    if not isinstance(value, dict) or value.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id'):
        return {'state': 'Unknown'}
    return {k: value[k] for k in ('state', 'reason', 'files', 'bytes', 'serial', 'kept') if k in value}


def tens(low, high, done, total):
    """A progress step between `low` and `high`, rounded down to tens."""
    return low + (high - low) * min(done, total) // max(total, 1) // 10 * 10


def splash(ctx, step):
    """Progress on the start screen while the setup runs at boot (0-100 in tens)."""
    folder = ctx.path(SPLASH)
    if folder.is_dir() and step in SPLASH_PHASES:
        try:
            (folder / 'phase').write_text(SPLASH_PHASES[step] + '\n')
        except OSError:
            pass


def automatic_blocked(ctx):
    """Why the setup must not start on its own now (it runs at a later start)."""
    from .update import status as update_status
    if update_status(ctx).get('state') in ('Queued', 'PendingHealth', 'RollbackPending'):
        return 'update_in_progress'           # 180 s health window: never a long step here
    battery = ctx.path('/sys/class/power_supply/BAT0')
    if read(battery / 'status') in ('Charging', 'Full') or read(
            ctx.path('/sys/class/power_supply/y2-usb-input/online')) == '1':
        return None
    from .battery import interpolate, load_profile
    profile = load_profile(ctx)
    voltage = read(battery / 'voltage_ocv')
    if not voltage or not voltage.isdigit():
        voltage = read(battery / 'voltage_now')          # loaded: lower, so never too early
    if profile is None or not voltage or not voltage.isdigit():
        return 'battery_unknown'
    return None if interpolate(profile['voltage_soc'], int(voltage)) >= MIN_PERCENT else 'battery_low'


def mount_old(ctx, device):
    """The stock FAT read-only at OLD after the read-only check (#44/F12 rules); None or a reason."""
    from .media import fat_benign_only
    if any(m['path'] == OLD and m['source'] == device for m in mountinfo(ctx.read('/proc/self/mountinfo'))):
        return None
    check = ctx.command(['fsck.fat', '-n', device], timeout=180, failure_output=True)
    if not check['ok'] and not (check['reason'] == 'command_failed' and fat_benign_only(check['output'])):
        return 'old_storage_check_failed'
    target = ctx.path(OLD)
    if target.is_symlink():
        return 'old_storage_path_unsafe'
    target.mkdir(mode=0o700, parents=True, exist_ok=True)
    result = ctx.command(['mount', '-t', 'vfat', '-o', 'ro,nosuid,nodev,noexec,utf8=1,fmask=0177,dmask=0077',
                          device, OLD], timeout=20)
    mounted = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == OLD]
    if not result['ok'] or len(mounted) != 1 or mounted[0]['source'] != device:
        return 'old_storage_mount_failed'
    return None


def unmount_old(ctx):
    if any(m['path'] == OLD for m in mountinfo(ctx.read('/proc/self/mountinfo'))):
        result = ctx.command(['umount', OLD], timeout=10)
        if not result['ok'] or any(m['path'] == OLD for m in mountinfo(ctx.read('/proc/self/mountinfo'))):
            return False
    return True


def save(ctx, value):
    folder = ctx.path(SETUP).parent
    folder.mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_json(ctx.path(SETUP), value, durable=True)
    dfd = os.open(folder, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(dfd)
    finally:
        os.close(dfd)


def setup(ctx, automatic=False, confirm_serial=None):
    """Turn the stock FAT into Y2Media (see the module note). `automatic` at start decides by
    itself; otherwise the listener confirmed with the old storage's serial (erases what is
    too much to keep). Resumes an interrupted run from the Y2DATA record."""
    lock = lock_runtime(ctx)
    try:
        return _setup(ctx, automatic, confirm_serial)
    except (OSError, ValueError) as error:
        return note(ctx, 'Failed', 'setup_error:' + str(error)[:100])
    finally:
        os.close(lock)


def _setup(ctx, automatic, confirm_serial):
    entry = partition(ctx)
    if entry is None:
        return note(ctx, 'Unavailable', 'partition_geometry_not_found')
    device = '/dev/' + entry.name
    props = probe(ctx, entry)
    saved = ctx.json(SETUP, None)
    if ctx.path(SETUP).exists() and not isinstance(saved, dict):
        return note(ctx, 'Failed', 'setup_record_unreadable')          # never guess: leave p8 alone
    if saved:
        if (saved.get('device'), saved.get('start'), saved.get('size')) != (device, START, SIZE):
            return note(ctx, 'Failed', 'setup_record_mismatch')
        if automatic and saved.get('phase') == 'staging' and automatic_blocked(ctx):
            return note(ctx, 'Pending', automatic_blocked(ctx))
    else:
        if valid(props):
            return note(ctx, 'NotNeeded')
        if props.get('TYPE') != 'vfat':
            return note(ctx, 'Unavailable', 'not_stock_storage', filesystem=props.get('TYPE'))
        serial = props.get('UUID')
        if automatic:
            blocked = automatic_blocked(ctx)
            if blocked:
                return note(ctx, 'Pending', blocked)
        elif not confirm_serial or confirm_serial != serial:
            return note(ctx, 'Failed', 'confirmation_does_not_match_stock_filesystem')
        if any(m['source'] == device and m['path'] != OLD for m in mountinfo(ctx.read('/proc/self/mountinfo'))):
            return note(ctx, 'Failed', 'partition_is_mounted')
        splash(ctx, 0)
        problem = mount_old(ctx, device)
        kept = [] if problem else keepable(ctx.path(OLD))
        total = sum(size for _, size in kept)
        if problem:
            case = 3
        else:
            free = os.statvfs(ctx.path('/data'))
            case = decide(kept, free.f_bavail * free.f_frsize)
        if case == 3:
            if automatic:
                # Nothing changes; the old files stay readable (Old Storage) until the listener decides.
                return note(ctx, 'Waiting', problem or 'old_storage_has_files', files=len(kept),
                            bytes=total, serial=serial)
            kept, total = [], 0                    # confirmed: what was too much to keep is erased
        saved = {'schema': 1, 'device': device, 'start': START, 'size': SIZE, 'serial': serial,
                 'phase': 'staging' if kept else 'formatting', 'files': len(kept), 'bytes': total}
        save(ctx, saved)
    note(ctx, 'Running', None, files=saved.get('files', 0), bytes=saved.get('bytes', 0))

    if saved['phase'] == 'staging':
        if props.get('TYPE') != 'vfat' or props.get('UUID') != saved['serial']:
            return note(ctx, 'Failed', 'old_storage_changed')
        problem = mount_old(ctx, device)
        if problem:
            return note(ctx, 'Failed', problem)
        source, staging = str(ctx.path(OLD)), str(ctx.path(KEPT_STAGING))
        kept = keepable(source)
        total, done = sum(size for _, size in kept), 0
        for relative, size in kept:
            target = os.path.join(staging, relative)
            os.makedirs(os.path.dirname(target), mode=0o700, exist_ok=True)
            copy_verified(os.path.join(source, relative), target, size)
            done += size
            splash(ctx, tens(10, 50, done, total))
        os.sync()
        saved['phase'] = 'formatting'
        save(ctx, saved)

    if saved['phase'] == 'formatting':
        # Old Storage leaves the computer first (its view ended with 'Running' above).
        from .usb import release_view
        if not release_view(ctx, OLD) or not unmount_old(ctx):
            return note(ctx, 'Failed', 'old_storage_busy')
        props = probe(ctx, entry)
        if not valid(props):
            if props.get('TYPE') == 'vfat' and props.get('UUID') != saved['serial']:
                return note(ctx, 'Failed', 'old_storage_changed')
            # The recorded stock FAT, or what an interrupted format left: only this record
            # (same partition, serial, every kept file verified on Y2DATA) allows formatting.
            if any(m['source'] == device for m in mountinfo(ctx.read('/proc/self/mountinfo'))):
                return note(ctx, 'Failed', 'partition_is_mounted')
            splash(ctx, 50)
            made = ctx.command(MKFS + [device], timeout=300)
            if not made['ok']:
                return note(ctx, 'Failed', 'mkfs_failed:' + str(made['reason']))
        saved['phase'] = 'migrating'
        save(ctx, saved)

    if saved['phase'] == 'migrating':
        splash(ctx, 60)
        state = _mount(ctx)
        if state['state'] != 'Ready':
            return note(ctx, 'Failed', 'mount_after_setup:' + str(state['reason']))
        ctx.path(MUSIC).mkdir(mode=0o755, parents=True, exist_ok=True)
        if ctx.path(KEPT_STAGING).is_dir():
            migrate(ctx, KEPT_STAGING, MUSIC + '/' + KEPT,
                    progress=lambda done, total: splash(ctx, tens(60, 90, done, total)))
            for current, names, files in os.walk(ctx.path(KEPT_STAGING), topdown=False):
                for name in names:
                    try:
                        os.rmdir(os.path.join(current, name))
                    except OSError:
                        pass
            try:
                os.rmdir(ctx.path(KEPT_STAGING))
            except OSError:
                pass
        migrate(ctx)                                      # the old /data/music library, as provision does
        os.sync()
        ctx.path(SETUP).unlink()
        splash(ctx, 100)
        return note(ctx, 'Done', None, kept=saved.get('files', 0))
    return note(ctx, 'Failed', 'setup_record_phase')
