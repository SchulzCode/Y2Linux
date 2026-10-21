"""Signed full-system OTA (OTA-35): check, download, install request, boot health.

The small native core is the shared authentication/policy authority. It writes
the RECOVERY image and the MISC request; the Y2 recovery image performs the
offline backup/write/readback of BOOTIMG and Y2ROOT. This module never opens a
block device itself.
"""
# SPDX-License-Identifier: GPL-2.0-only
from .workload import workload
import contextlib
import fcntl
import hashlib
import json
import collections
import os
from pathlib import Path
import shutil
import signal
import stat
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from .boot import private_directory, mark
from .common import atomic_json
from .transfer import data_volume

FILES = ('manifest.json', 'manifest.sig', 'rootfs.ext4.gz', 'bootimg.img.gz', 'recovery.img.gz')
PAYLOADS = {'rootfs': 'rootfs.ext4.gz', 'bootimg': 'bootimg.img.gz', 'recovery': 'recovery.img.gz'}
TERMINAL = ('Idle', 'Acknowledged', 'RolledBack', 'Failed')
RESERVE = 96 * 1024**2
CHANNEL = '/etc/y2linux/update-channel.json'
# Owner gate before the device turns off to install (decision 2026-10-15).
MINIMUM_PERCENT = 50
CHECK_INTERVAL = 24 * 3600
# Wi-Fi needs 15-20 s to reconnect after the player wakes; a Check for Updates
# pressed meanwhile failed at once ("name resolution") and hid Update Now.
NETWORK_WAIT_S = 30
MANIFEST_ATTEMPTS = 3


def channel(ctx):
    """The shipped release channel, or the owner's override on /data."""
    shipped = ctx.json(CHANNEL, {}) or {}
    override = ctx.json('/data/updates/channel.json', {}) or {}
    value = {**shipped, **{k: v for k, v in override.items() if k in ('manifest_url', 'name')}}
    hosts = shipped.get('allowed_redirect_hosts', [])
    if not isinstance(hosts, list) or not all(isinstance(h, str) and h for h in hosts):
        hosts = []
    value['allowed_redirect_hosts'] = hosts
    return value


def status(ctx):
    value = ctx.json('/data/updates/state.json')
    if value is None and not ctx.path('/data/updates/state.json').exists() and not ctx.path('/data/updates/state.json').is_symlink():
        ready = ctx.read('/data/.y2data-schema') == '1' and ctx.path('/data/updates').is_dir()
        value = {'schema': 1, 'state': 'Idle' if ready else 'Unavailable',
                 'failure': None if ready else 'data_state_unavailable'}
    # The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    if not isinstance(value, dict) or value.get('schema') not in (1, 2):
        return {'schema': 1, 'state': 'Failed', 'failure': 'journal_unreadable'}
    return {**value, 'download': ctx.json('/data/updates/download.json'),
            'last_check': ctx.json('/data/updates/last-check.json'),
            'channel': channel(ctx).get('name'), 'full_system': True,
            'physical_qualification': False}


def core(ctx, action, directory=None, timeout=15):
    argv = ['/usr/sbin/y2-update-core', action]
    if directory:
        argv.append(directory)
    result = ctx.command(argv, timeout=timeout, limit=20000, failure_output=True)
    if not result['ok']:
        try:
            refusal = json.loads(result.get('output') or '{}').get('failure')
        except (ValueError, TypeError, AttributeError):
            refusal = None
        raise ValueError(refusal or ('update_core_' + (result['reason'] or 'refused')))
    value = json.loads(result['output'])
    if not isinstance(value, dict):
        raise ValueError('invalid_update_core_response')
    return value


def free(fd, size):
    info = os.fstatvfs(fd)
    if (info.f_flag & os.ST_RDONLY or info.f_bavail * info.f_frsize < size + RESERVE or
            info.f_files and info.f_favail < 128):
        raise ValueError('update_data_space_reserve')


class NoSpace(ValueError):
    """Y2DATA cannot hold the download even after the cleanup."""
    def __init__(self, needed):
        super().__init__('update_data_space_reserve')
        self.needed = needed


# Rollback copies of the running system. The recovery deletes them right
# before it writes the new ones, so they count as space the update can use.
BACKUPS = ('previous.ext4.gz', 'previous-bootimg.gz')
# Kept in /data/logs: the live system log and the last installer run.
KEPT_LOGS = ('system.log', 'recovery-last.log', 'recovery-display.txt')


def disposable(ctx):
    """Data the player can lose without losing anything a user sees: rotated
    and diagnostic logs, rotated platform journals and finished qualification
    runs. Never music, settings, the library or the rollback copies."""
    logs = ctx.path('/data/logs')
    if logs.is_dir():
        yield from sorted(p for p in logs.iterdir() if p.name not in KEPT_LOGS)
    platform = ctx.path('/data/system/platform')
    if platform.is_dir():
        yield from sorted(p for p in platform.iterdir()
                          if p.name.endswith('.previous.jsonl') or p.name == 'cpu-idle-qualification' or
                          p.name.startswith(('fix01-', 'fix02-')))
    system = ctx.path('/data/system')
    if system.is_dir():
        yield from sorted(system.glob('cpu-final-qualification-*'))


def remove_disposable(path):
    """Remove one entry without following links; returns the bytes freed."""
    info = path.lstat()
    if stat.S_ISDIR(info.st_mode):
        size = sum(f.lstat().st_size for f in path.rglob('*') if not f.is_symlink() and f.is_file())
        shutil.rmtree(path)
        return size
    path.unlink()
    return info.st_size if stat.S_ISREG(info.st_mode) else 0


def make_room(ctx, fd, size):
    """Make sure Y2DATA holds `size` bytes plus the reserve (OTA-46): count the
    rollback copies the recovery replaces, then remove disposable data until the
    download fits. Returns the bytes removed; raises NoSpace with the total the
    update needs when even that is not enough."""
    root = ctx.path('/data/updates')
    reclaimable = 0
    for name in BACKUPS:
        with contextlib.suppress(OSError):
            info = (root / name).lstat()
            if stat.S_ISREG(info.st_mode):
                reclaimable += info.st_size
    need = size + RESERVE - reclaimable

    def fits():
        info = os.fstatvfs(fd)
        return (not info.f_flag & os.ST_RDONLY and info.f_bavail * info.f_frsize >= need and
                not (info.f_files and info.f_favail < 128))
    removed = 0
    if fits():
        return removed
    for path in list(disposable(ctx)):
        with contextlib.suppress(OSError):
            removed += remove_disposable(path)
        if fits():
            return removed
    # Last: the kept package of the running release (OTA-50). Without it the
    # recovery backs up the live root instead, which is only slower.
    kept = root / 'current'
    with contextlib.suppress(OSError, ValueError):
        size = sum(f.lstat().st_size for f in kept.iterdir()) if kept.is_dir() else 0
        remove_package(kept)
        removed += size
        if fits():
            return removed
    raise NoSpace(need)


class AllowlistedRedirect(urllib.request.HTTPRedirectHandler):
    """Release hosting redirects to a CDN. Follow HTTPS only, to listed hosts;
    authenticity comes from the signed manifest, never from the transport."""
    max_redirections = 5

    def __init__(self, hosts):
        super().__init__()
        self.hosts = frozenset(h.lower() for h in hosts)

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        parts = urllib.parse.urlsplit(newurl)
        if (parts.scheme != 'https' or not parts.hostname or parts.username or parts.password
                or parts.hostname.lower() not in self.hosts or any(ord(c) < 33 for c in newurl)):
            raise ValueError('update_redirect_refused')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def base_url(url):
    parts = urllib.parse.urlsplit(url)
    if (parts.scheme != 'https' or not parts.hostname or parts.username or parts.password or
            parts.fragment or parts.query or not parts.path.endswith('/manifest.json') or
            any(ord(c) < 33 for c in url)):
        raise ValueError('HTTPS_manifest_URL_required_without_credentials_query_or_redirect')
    return url[:-len('manifest.json')]


def copy_stream(source, destination, maximum, exact=None, seconds=900, report=None):
    digest = hashlib.sha256()
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    count = 0
    deadline = time.monotonic() + seconds
    try:
        with os.fdopen(fd, 'wb') as stream:
            while True:
                if time.monotonic() >= deadline:
                    raise ValueError('download_deadline')
                chunk = source.read(min(1024**2, maximum - count + 1))
                if not chunk:
                    break
                count += len(chunk)
                if count > maximum:
                    raise ValueError('download_size_limit')
                free(stream.fileno(), len(chunk))
                stream.write(chunk)
                digest.update(chunk)
                if report is not None:
                    report(count)
            if exact is not None and count != exact:
                raise ValueError('incomplete_download')
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        # Keep interrupted bytes for explicit diagnostics; never queue them.
        raise
    return count, digest.hexdigest()


def remove_package(directory):
    # Only these known disposable files; unexpected contents stop cleanup.
    if not directory.exists():
        return
    if not stat.S_ISDIR(directory.lstat().st_mode):
        raise ValueError('unsafe_staging_directory')
    names = set(os.listdir(directory))
    if names - set(FILES):
        raise ValueError('unknown_staging_contents')
    for name in names:
        file = directory / name
        if not stat.S_ISREG(file.lstat().st_mode) or file.stat().st_nlink != 1:
            raise ValueError('unsafe_staging_file')
    for name in names:
        (directory / name).unlink()
    directory.rmdir()


PROGRESS = '/run/y2/update-progress.json'
CHECK_PROGRESS = '/run/y2/update-check-progress'


class Progress:
    """Live download progress for Reborn in RAM (OTA-48): bytes, speed over
    the last few seconds and time left, at most four times a second. The
    durable journal on Y2DATA only records the start and the result."""
    WINDOW = 4.0

    def __init__(self, ctx, release, total):
        self.path = ctx.path(PROGRESS)
        self.release, self.total = release, total
        self.samples = collections.deque()
        self.written = 0.0
        self.done = 0

    def publish(self, phase, force=False):
        now = time.monotonic()
        if not force and now - self.written < 0.25:
            return
        self.written = now
        self.samples.append((now, self.done))
        while len(self.samples) > 2 and now - self.samples[0][0] > self.WINDOW:
            self.samples.popleft()
        (t0, b0), (t1, b1) = self.samples[0], self.samples[-1]
        speed = int((b1 - b0) / (t1 - t0)) if t1 - t0 >= 0.5 else None
        left = int((self.total - self.done) / speed) if speed and phase == 'download' else None
        value = {'schema': 1, 'phase': phase, 'release_version': self.release, 'bytes_done': self.done,
                 'bytes_total': self.total, 'speed_bytes_per_s': speed, 'seconds_left': left}
        with contextlib.suppress(OSError, ValueError):
            atomic_json(self.path, value, mode=0o644)

    def finish(self, phase):
        self.publish(phase, force=True)


class Cancelled(ValueError):
    """The owner stopped a running download (Reborn sends SIGTERM)."""


def wait_online(ctx, seconds=None, sleep=time.sleep):
    """True once Wi-Fi is Online (DNS answered), waiting at most `seconds`."""
    deadline = time.monotonic() + (NETWORK_WAIT_S if seconds is None else seconds)
    while ctx.json('/run/y2/network.json', {}).get('state') != 'Online':
        if time.monotonic() >= deadline:
            return False
        sleep(1)
    return True


def _cancel(signum, frame):
    raise Cancelled('download_cancelled')


def stage(ctx, source=None, url=None, check_only=False):
    if bool(source) == bool(url):
        raise ValueError('exactly_one_local_package_or_HTTPS_manifest_required')
    volume = data_volume(ctx)
    if ctx.path('/data/system/platform/maintenance-pending').exists():
        raise ValueError('maintenance_in_progress')
    root = private_directory(ctx.path('/data/updates'))
    fd = os.open(root / '.stage.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    token = '.incoming-' + uuid.uuid4().hex
    incoming = root / token
    live = None
    summary = None
    previous = None
    terminate = signal.getsignal(signal.SIGTERM)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if status(ctx)['state'] not in TERMINAL:
            raise ValueError('update_already_pending')
        # An earlier interrupted download is disposable; bounded, named cleanup.
        old = list(root.glob('.incoming-*'))
        if len(old) > 8:
            raise ValueError('excess_staging_owner_inspection_required')
        for item in old:
            remove_package(item)
        # A cancelled or failed package is never installed; free its space
        # before this download is measured (OTA-46).
        if not check_only:
            remove_package(root/'pending')
        # Even the manifest needs the reserve: a nearly full volume is cleaned
        # first, so a check can still find the release (OTA-46).
        cleaned = make_room(ctx, fd, 64 * 1024)
        # What the Software Update page showed: a failed check keeps it.
        previous = ctx.json('/data/updates/download.json')
        incoming.mkdir(mode=0o700)
        # A check only reads the signed manifest; it never shows as a download.
        atomic_json(root/'download.json', {'schema': 1, 'state': 'Checking' if check_only else 'Staging',
                    'id': token}, durable=True)
        if url:
            from .timekeeping import status as clock
            if not clock(ctx)['tls_ready'] or not clock(ctx)['entropy_ready']:
                raise ValueError('clock_or_entropy_not_ready_for_TLS')
            if not wait_online(ctx):
                raise ValueError('network_not_online')
            base = base_url(url)
            opener = urllib.request.build_opener(AllowlistedRedirect(channel(ctx)['allowed_redirect_hosts']),
                                                 urllib.request.ProxyHandler({}))

            @contextlib.contextmanager
            def get(name):
                with opener.open(base + name, timeout=10) as response:
                    if response.status != 200:
                        raise ValueError('HTTP_status')
                    yield response
        else:
            directory = Path(source)
            if not stat.S_ISDIR(directory.lstat().st_mode):
                raise ValueError('local_package_directory_required')

            @contextlib.contextmanager
            def get(name):
                source_fd = os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(source_fd, 'rb') as stream:
                    if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                        raise ValueError('local_package_regular_files_required')
                    yield stream
        for name, maximum in [('manifest.json', 16384), ('manifest.sig', 64)]:
            for attempt in range(1, MANIFEST_ATTEMPTS + 1):
                try:
                    with get(name) as stream:
                        copy_stream(stream, incoming/name, maximum, exact=64 if name.endswith('.sig') else None,
                                    seconds=30)
                    break
                except urllib.error.HTTPError as error:
                    if not (check_only and error.code == 404 and name == 'manifest.json'):
                        raise
                    # Nothing published on the channel yet: nothing newer to install.
                    remove_package(incoming)
                    installed = ctx.json('/etc/y2linux/versions.json', {}).get('release_version')
                    atomic_json(root/'download.json', {'schema': 1, 'state': 'Checked', 'up_to_date': True,
                                                       'release_version': installed, 'published': False})
                    return {'state': 'UpToDate', 'release_version': installed}
                except (urllib.error.URLError, TimeoutError):
                    # A lookup or handshake that failed while Wi-Fi was still
                    # settling: try again once it is online.
                    if not url or attempt == MANIFEST_ATTEMPTS:
                        raise
                    (incoming/name).unlink(missing_ok=True)
                    time.sleep(3)
                    wait_online(ctx, 10)
        try:
            manifest = core(ctx, 'check-manifest', '/data/updates/' + token)
        except ValueError as error:
            if not check_only or 'downgrade/replay' not in str(error):
                raise
            # The channel offers this release (or an older one): up to date.
            remove_package(incoming)
            installed = ctx.json('/etc/y2linux/versions.json', {}).get('release_version')
            atomic_json(root/'download.json', {'schema': 1, 'state': 'Checked', 'up_to_date': True,
                                               'release_version': installed})
            return {'state': 'UpToDate', 'release_version': installed}
        payloads = manifest['payloads']
        total = sum(item['bytes'] for item in payloads.values())
        summary = {'schema': 1, 'state': 'Checked', 'release_version': manifest['release_version'],
                   'sequence': manifest['sequence'], 'download_bytes': total,
                   'notes': manifest.get('notes', ''), 'reborn_version': manifest.get('reborn_version', '')}
        if check_only:
            remove_package(incoming)
            atomic_json(root/'download.json', summary)
            return manifest
        # Until the package is committed below, SIGTERM (Reborn's Cancel)
        # stops the download and leaves the release offered as before.
        signal.signal(signal.SIGTERM, _cancel)
        # Root payload plus its backup during installation; refuse early, not in recovery.
        cleaned += make_room(ctx, fd, total + payloads['rootfs']['raw_bytes'] // 4)
        # Music and the screen come first: download and check yield the CPU.
        with contextlib.suppress(OSError):
            os.nice(10)
        live = Progress(ctx, manifest['release_version'], total)
        live.finish('download')
        count = 0

        def report(done):
            live.done = count + done
            live.publish('download')
        for key, item in payloads.items():
            name = PAYLOADS[key]
            with get(name) as stream:
                got, digest = copy_stream(stream, incoming/name, item['bytes'], exact=item['bytes'],
                                          report=report)
            if digest != item['sha256']:
                raise ValueError('download_hash_mismatch')
            count += got
        live.done = count
        # The queue below decodes and checks every payload once (OTA-48: the
        # separate check before it decoded the same files a second time).
        with contextlib.suppress(OSError):
            ctx.path(CHECK_PROGRESS).unlink()
        live.finish('verify')
        if data_volume(ctx)['generation'] != volume['generation']:
            raise ValueError('update_data_generation_changed')
        # The commit point: from here the package is queued, never half-cancelled.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        remove_package(root/'pending')
        incoming.rename(root/'pending')
        parent = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
        result = core(ctx, 'queue', timeout=300)
        live.finish('done')
        atomic_json(root/'download.json', {'schema': 1, 'state': 'Verified', 'bytes': count,
                    'release_version': manifest['release_version'], 'notes': manifest.get('notes', ''),
                    'backup_budget': 'recovery captures the compressed previous root and BOOTIMG before any write',
                    'reserve_bytes': RESERVE, 'cleaned_bytes': cleaned}, durable=True)
        return result
    except Cancelled:
        with contextlib.suppress(OSError, ValueError):
            remove_package(incoming)
        atomic_json(root/'download.json', summary, durable=True)
        if live is not None:
            live.finish('failed')
        raise
    except (OSError, ValueError, TimeoutError) as error:
        record = {'schema': 1, 'state': 'Failed', 'failure': str(error)}
        if check_only and isinstance(previous, dict) and previous.get('state') == 'Checked':
            # A check that couldn't reach the server keeps the release the page
            # already offered (Update Now stays); the error goes to the caller.
            record = {**previous, 'last_check_failure': str(error)[:120]}
        if isinstance(error, NoSpace):
            record['needed_bytes'] = error.needed
            with contextlib.suppress(OSError, ValueError):
                remove_package(incoming)
        atomic_json(root/'download.json', record, durable=True)
        if live is not None:
            live.finish('failed')
        raise
    finally:
        with contextlib.suppress(TypeError, ValueError):
            signal.signal(signal.SIGTERM, terminate if terminate is not None else signal.SIG_DFL)
        os.close(fd)


def restart_mode(ctx):
    """Restart into the installer and back (proven on 43 and 45). Owner
    override: /data/updates/restart.json {"mode": "poweroff"} turns off instead,
    and Power starts the installer and the new system."""
    value = ctx.json('/data/updates/restart.json', {}) or {}
    return 'poweroff' if isinstance(value, dict) and value.get('mode') == 'poweroff' else 'reboot'


def battery_gate(ctx):
    """Charger connected, or at least MINIMUM_PERCENT from this boot's estimate."""
    power = ctx.json('/run/y2/power.json', {}) or {}
    battery = power.get('battery') or {}
    if power.get('boot_id') not in (None, ctx.read('/proc/sys/kernel/random/boot_id')):
        battery = {}
    if battery.get('charging') is True or battery.get('status') in ('Charging', 'Full'):
        return
    percent = battery.get('percent')
    if type(percent) is not int or percent < MINIMUM_PERCENT:
        raise ValueError('update_needs_charger_or_half_battery')


@workload("Maintenance")
def apply(ctx, rollback=False):
    """Install (or restore) in the Y2 recovery image: LK starts RECOVERY from
    the MISC request on the restart (or, with the owner's override, on the
    next Power press)."""
    if rollback:
        core(ctx, 'rollback')
    elif status(ctx)['state'] != 'Queued':
        raise ValueError('no_verified_queued_update')
    battery_gate(ctx)
    core(ctx, 'prepare', timeout=120)
    from .power import rpc
    return rpc(ctx, {'action': restart_mode(ctx), 'reason': 'update'})


@workload("Maintenance")
def probe(ctx):
    """Owner test: enter the recovery image, which records that it ran and
    shows its screen, then returns like an install (restart by default)."""
    core(ctx, 'probe', timeout=60)
    from .power import rpc
    return rpc(ctx, {'action': restart_mode(ctx), 'reason': 'update'})


def cancel(ctx):
    """Cancel a queued update and free its package right away (OTA-46)."""
    result = core(ctx, 'cancel')
    with contextlib.suppress(OSError, ValueError):
        remove_package(ctx.path('/data/updates/pending'))
    return result


def automatic_check(ctx, now=None):
    """Daily check on Wi-Fi: only reports that an update exists, never downloads
    a payload or installs. Called by the maintenance loop; cheap when not due."""
    now = time.time() if now is None else now
    directory = private_directory(ctx.path('/data/updates'))
    last = ctx.json('/data/updates/last-check.json', {}) or {}
    # `time` counts only from a check that reached the server: ≤ 8.14 kept it on
    # a failed one too, which held off every automatic check for a day (ef22a23d).
    if last.get('result') != 'checked':
        last = {k: v for k, v in last.items() if k != 'time'}
    if type(last.get('time')) in (int, float) and 0 <= now - last['time'] < CHECK_INTERVAL:
        return None
    # Any attempt is recorded, so an unready clock retries hourly, not every tick.
    atomic_json(directory/'last-check.json', {**last, 'schema': 1, 'attempt': now})
    if status(ctx)['state'] not in TERMINAL or not channel(ctx).get('manifest_url'):
        return None
    if ctx.json('/run/y2/network.json', {}).get('state') != 'Online':
        return None
    from .timekeeping import status as clock
    if not clock(ctx)['tls_ready']:
        return None
    # Only a check that reached the server starts the next day; a failed one
    # is tried again with the hourly attempt (it waited a whole day).
    record = {'schema': 1, 'attempt': now}
    if type(last.get('time')) in (int, float):
        record['time'] = last['time']
    previous = ctx.json('/data/updates/download.json')
    try:
        manifest = stage(ctx, url=channel(ctx)['manifest_url'], check_only=True)
        record.update(time=now, result='checked', release_version=manifest['release_version'])
    except (OSError, ValueError, TimeoutError) as error:
        record.update(result='failed', failure=str(error)[:120])
        # A failed background check is not the owner's failed update: keep
        # what the Software Update page showed before.
        with contextlib.suppress(OSError, ValueError):
            if previous is None:
                ctx.path('/data/updates/download.json').unlink(missing_ok=True)
            else:
                atomic_json(ctx.path('/data/updates/download.json'), previous, durable=True)
    atomic_json(directory/'last-check.json', record, durable=True)
    return record


def application_healthy(app):
    """Reborn's own verdict (`healthy`, Beta 8): drawing, database working, no
    stuck worker. A library scan that could not read some songs is not a
    reason to roll an update back (V1 code review B3). Without the field (a
    Reborn before Beta 8), the drawing and uptime checks alone."""
    if not isinstance(app, dict):
        return False
    if 'healthy' in app:
        return app['healthy'] is True
    graphics = app.get('graphics')
    return (isinstance(graphics, dict) and graphics.get('available') is True and
            type(app.get('uptime_seconds')) is int and app['uptime_seconds'] >= 10)


def health_ack(ctx):
    pending = status(ctx)
    if pending['state'] != 'PendingHealth':
        return pending
    runtime = private_directory(ctx.path('/run/y2'))
    progress = ctx.json('/run/y2/update-health.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if progress.get('boot_id') != boot:
        progress = {'boot_id': boot, 'deadline_monotonic_s': time.monotonic() + 180}
        atomic_json(runtime/'update-health.json', progress)
    if time.monotonic() >= progress['deadline_monotonic_s']:
        # Ask LK for the recovery image now, so even a later hang before the
        # rescue restores on the next power-on. Never an automatic power-off.
        core(ctx, 'rollback')
        return core(ctx, 'prepare', timeout=120)
    # A successful frame alone is insufficient: app must still answer, core
    # mounts/devices must be usable, and the same boot/process must own readiness.
    ready = ctx.json('/run/y2/application-ready.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    if ready.get('boot_id') != boot or ready.get('first_frame') is not True:
        return {'state': 'PendingHealth', 'reason': 'application_first_frame_missing'}
    pid = ready.get('pid')
    if type(pid) is not int or pid <= 0:
        return {'state': 'PendingHealth', 'reason': 'application_pid_missing'}
    from .observe import memory
    current = memory(ctx, [pid], False)['processes'][0]
    if current.get('start_ticks') != ready.get('start_ticks'):
        return {'state': 'PendingHealth', 'reason': 'application_generation_changed'}
    from .health import check
    checks = {item['name']: item for item in check(ctx)['checks']}
    required = ('root', 'data', 'filesystem_errors', 'display', 'render_node', 'audio', 'input')
    if any(checks.get(name, {}).get('state') != 'OK' for name in required):
        return {'state': 'PendingHealth', 'reason': 'core_health_not_OK'}
    answer = ctx.command(['/usr/bin/rebornctl', 'status', '--json'], timeout=2)
    try:
        app = json.loads(answer['output']) if answer['ok'] else {}
    except (ValueError, TypeError):
        app = {}
    if not application_healthy(app):
        return {'state': 'PendingHealth', 'reason': 'application_not_ready'}
    mark(ctx, 'application_ready')
    result = core(ctx, 'ack')
    # The core keeps the installed package as current/ (the next install's
    # rollback copy); anything left in pending/ is no longer needed.
    with contextlib.suppress(OSError, ValueError):
        remove_package(ctx.path('/data/updates/pending'))
    return result
