"""Consumer MTP + advanced ECM; Linux is the only block/filesystem owner.

No block-export function is selectable. The daemon pins media roots per lifetime;
mount identity changes require unbinding/stopping it before a replacement view.
"""
# SPDX-License-Identifier: GPL-2.0-only
import os
import signal
import subprocess
import time
from pathlib import Path
from .common import atomic_json, read
from .observe import mountinfo, storage

GADGET = '/sys/kernel/config/usb_gadget/y2'
FUNCTIONS = {'mtp': ('ffs.mtp', 'ecm.usb0'), 'developer': ('acm.usb0', 'ecm.usb0')}
SCHEMA = 'org.y2linux.usb-transfer/v1'
STABLE_SECONDS = 60


def selection(mode):
    if mode not in FUNCTIONS:
        raise ValueError('unsupported_usb_mode')
    return FUNCTIONS[mode]


def media_views(ctx):
    """Host receives user music, never Y2DATA/private/system contents or a LUN."""
    volumes = storage(ctx)['volumes']
    mounts = mountinfo(ctx.read('/proc/self/mountinfo'))
    result = []
    # Internal Storage is the Y2Media partition's music folder (volumes[3]); the old
    # /data/music folder on Y2DATA is no longer exported.
    for volume, path, label in ((volumes[3], '/media/internal/music', 'Internal Storage'),
                                (volumes[2], '/media/sd', 'SD Card')):
        mount = next((m for m in mounts if m['path'] == volume['path']), None)
        root = ctx.path(path)
        if path == '/media/internal/music':
            device = ctx.path('/sys/dev/block/' + mount['device_id']) if mount else None
            if (not device or read(device / 'start') != '3742720' or
                    read(device / 'size') != '11460608' or
                    volume.get('uuid') != '79324c69-6e75-4801-8000-000000000103' or
                    not mount or mount['filesystem'] != 'ext4'):
                continue
        # Existing validated internal partition and platform SD mount claim
        # supply the identity. Never create a view of an absent underlying mount.
        release = ctx.json('/run/y2/usb-release-sd.json', {})
        if path == '/media/sd' and mount and release.get('mount_id') == mount['mount_id']:
            continue
        if (volume['state'] in ('Ready', 'Degraded') and mount and root.is_dir() and not root.is_symlink()
                and not any(m['path'].startswith(path + '/') or
                            (path == '/media/internal/music' and m['path'] == path) for m in mounts)):
            result.append({'path': path, 'name': label, 'mount_id': mount['mount_id'],
                           'device_id': mount['device_id'], 'uuid': volume.get('uuid'),
                           'filesystem': mount['filesystem'], 'access': 'ro' if 'ro' in mount['options'] or 'ro' in mount['super_options'] else 'rw'})
    # STORAGE-SETUP case 3: the stock storage, read-only, until the listener sets up Internal
    # Storage, so the old files can be copied off first.
    from . import internal_media
    old = next((m for m in mounts if m['path'] == internal_media.OLD), None)
    entry = internal_media.partition(ctx)
    if (old and entry and old['source'] == '/dev/' + entry.name and old['filesystem'] == 'vfat'
            and internal_media.setup_status(ctx).get('state') == 'Waiting'):
        result.append({'path': internal_media.OLD, 'name': NAMES[internal_media.OLD],
                       'mount_id': old['mount_id'], 'device_id': old['device_id'], 'uuid': None,
                       'filesystem': 'vfat', 'access': 'ro'})
    # RELEASE-49: while the radio firmware is missing, one empty folder of
    # Y2DATA is shown as "Firmware" for the owner's system.img copy. With
    # Internal Storage the copy goes into its Firmware folder instead (#39:
    # Y2DATA is too small for an unpacked image).
    from .radiofw import INBOX, inbox, present
    data = next((m for m in mounts if m['path'] == '/data'), None)
    if not present(ctx) and internal_media.ready(ctx):
        try:
            inbox(ctx)              # Internal Storage → Firmware, before the computer lists it
        except OSError:
            pass
    elif data and 'ro' not in data['options'] and not present(ctx):
        try:
            inbox(ctx)
            result.append({'path': INBOX, 'name': NAMES[INBOX], 'mount_id': data['mount_id'],
                           'device_id': data['device_id'], 'uuid': None,
                           'filesystem': data['filesystem'], 'access': 'rw'})
        except OSError:
            pass
    return result


NAMES = {'/media/internal/music': 'Internal Storage', '/media/sd': 'SD Card',
         '/data/firmware/inbox': 'Firmware', '/run/y2/old-storage': 'Old Storage'}


def identity(view):
    return (view.get('mount_id'), view.get('device_id'), view.get('uuid'), view['access'])


def expectations(views):
    """Mount identity the responder must find when it pins a root."""
    return ''.join('%s %s %s\n' % (v['path'], v['device_id'], v['mount_id'])
                   for v in views if v.get('device_id') and v.get('mount_id') is not None)


def config(views):
    # Only fixed paths and labels reach the responder parser. Both views are
    # always declared, in fixed slots (stable MTP storage IDs); a view that is
    # not currently available is declared `notmounted` and joins or leaves the
    # live session later without a USB re-enumeration.
    allowed = NAMES
    lines = ['loop_on_disconnect 1', 'manufacturer "Y2Linux"', 'product "Y2"',
             'serial "Y2LINUX-DEV-01"', 'interface "MTP"', 'firmware_version "Platform 1.0"',
             'mtp_extensions ""', 'usb_class 0x6', 'usb_subclass 0x1', 'usb_protocol 0x1', 'perceived_device_type 0x2', 'usb_functionfs_mode 1',
             'usb_dev_path "/dev/ffs-mtp/ep0"', 'usb_epin_path "/dev/ffs-mtp/ep1"',
             'usb_epout_path "/dev/ffs-mtp/ep2"', 'usb_epint_path "/dev/ffs-mtp/ep3"',
             'usb_max_packet_size 0x200', 'usb_max_rd_buffer_size 0x10000',
             'usb_max_wr_buffer_size 0x10000', 'read_buffer_cache_size 0x100000',
             'sync_when_close 0', 'no_inotify 1', 'umask 077']
    present = {}
    for v in views:
        if allowed.get(v['path']) != v['name'] or v['access'] not in ('ro', 'rw'):
            raise ValueError('unsafe_media_view')
        present[v['path']] = v
    for path, name in allowed.items():
        v = present.get(path)
        options = ['ro' if v and v['access'] == 'ro' else 'rw']
        if path == '/media/sd':
            options.append('removable')
        if not v:
            options.append('notmounted')
        lines.append('storage "%s" "%s" "%s"' % (path, name, ','.join(options)))
    return '\n'.join(lines) + '\n'


def status(ctx):
    state = ctx.json('/run/y2/usb-transfer.json', {})
    if state.get('schema') != SCHEMA or state.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id'):
        return {'schema': SCHEMA, 'state': 'Unavailable', 'reason': 'service_not_observed',
                'filesystem_owner': 'linux', 'block_export': False}
    return state


def request_mode(ctx, mode):
    selection(mode)
    # Avoid disconnecting a host with an open session. Changes take effect after
    # unplug/replug; advanced users may choose before attaching the cable.
    atomic_json(ctx.path('/data/system/platform/usb-mode.json'), {'mode': mode}, durable=True)
    return {'state': 'Queued', 'mode': mode, 'apply': 'when_disconnected'}


def wake_service(ctx):
    """Ask the readiness service to re-read the media views now."""
    pid = ctx.integer('/run/y2/readiness.pid')
    if pid and pid > 1 and ctx.root == Path('/'):
        cmdline = ctx.read('/proc/%s/cmdline' % pid) or ''
        if 'service-daemon' in cmdline and 'y2-platform' in cmdline:
            try:
                os.kill(pid, signal.SIGUSR1)
            except ProcessLookupError:
                return False
    return True


def release_view(ctx, path, timeout=5.0):
    """Wait until the responder has dropped `path` (its view already left
    media_views); True when it is not pinned (also when no responder runs)."""
    if not wake_service(ctx):
        return False
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        roots = (ctx.json('/run/y2/mtp-roots.json', {}) or {}).get('roots')
        if not isinstance(roots, list) or path not in roots:
            return True
        time.sleep(.05)
    return False


def release_sd(ctx, mount_id, explicit=False):
    """Drop responder root pins before a device-side eject; never force umount.

    An automatic eject refuses while a host cable is attached. An explicit owner
    command (format/grow, which name the card by CID) may release the SD view
    from the responder anyway: the host only sees the storage leave, exactly as
    it would on removal."""
    current = status(ctx)
    if not any(v.get('path') == '/media/sd' for v in current.get('views', [])):
        return True
    if not explicit and current.get('cable') != 'not attached':
        return False
    atomic_json(ctx.path('/run/y2/usb-release-sd.json'), {'mount_id': mount_id})
    if not wake_service(ctx):
        return False
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        current = status(ctx)
        if (current.get('state') != 'Failed' and
                not any(v.get('path') == '/media/sd' for v in current.get('views', []))):
            return True
        time.sleep(.05)
    return False


class USBService:
    def __init__(self, ctx, launch=None):
        self.ctx = ctx
        self.launch = launch or self.spawn
        self.child = None
        self.mode = None
        self.views = None
        self.bound = False
        self.ready = False
        self.failures = 0
        self.started = 0
        self.previous_cable = None
        self.seen_generation = 0
        self.refresh_generation = 0
        self.last_record = None
        self.signature = None
        self.cached_views = []
        self.terminal = False
        self.mounted = {}
        self.sd_window = 0.0

    def write(self, relative, value):
        self.ctx.path(GADGET + '/' + relative).write_text(str(value) + '\n')

    def mkdir(self, relative):
        self.ctx.path(GADGET + '/' + relative).mkdir(parents=True, exist_ok=True)

    def spawn(self):
        # RLIMIT_AS bounds the lazy handle database as well as fixed USB buffers.
        import resource
        def limits():
            resource.setrlimit(resource.RLIMIT_AS, (64*1024**2, 64*1024**2))
            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        return subprocess.Popen(['/usr/sbin/umtprd', '-conf', '/run/y2/umtprd.conf'],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True, preexec_fn=limits,
            env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'})

    def stop(self):
        # Unbind first: endpoint reads return, then join/abort staged uploads.
        unbind_error = None
        if self.ctx.path(GADGET + '/UDC').exists() and (self.bound or self.ctx.read(GADGET + '/UDC')):
            try:
                self.write('UDC', '')
            except OSError as error:
                unbind_error = error
        self.bound = self.ready = False
        if self.child:
            if self.child.poll() is None:
                self.child.terminate()
                try:
                    self.child.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    self.child.kill()
                    self.child.wait(timeout=1)
            self.child = None
        if unbind_error:
            raise unbind_error

    def setup(self, mode, views):
        selection(mode)
        self.stop()
        root = self.ctx.path(GADGET)
        if not self.ctx.path('/sys/kernel/config/usb_gadget').exists():
            answer = self.ctx.command(['mount', '-t', 'configfs', 'none', '/sys/kernel/config'])
            if not answer['ok']:
                raise OSError('configfs_mount_failed')
        self.mkdir('')
        # Keep the existing development VID/PID identity (owner-local build),
        # power request and serial. No fabricated production USB allocation.
        for name, value in {'idVendor': '0x0525', 'idProduct': '0xa4aa', 'bcdUSB': '0x0200',
                            'bcdDevice': '0x0100', 'bDeviceClass': '0',
                            'bDeviceSubClass': '0', 'bDeviceProtocol': '0'}.items():
            self.write(name, value)
        self.mkdir('configs/c.1')
        self.mkdir('os_desc')
        self.write('os_desc/use', '1' if mode == 'mtp' else '0')
        self.write('os_desc/b_vendor_code', '0x20')
        self.write('os_desc/qw_sign', 'MSFT100')
        os_link = root / 'os_desc/c.1'
        if os_link.is_symlink():
            os_link.unlink()
        if mode == 'mtp':
            os_link.symlink_to(root / 'configs/c.1')
        self.mkdir('strings/0x409')
        for name, value in {'manufacturer': 'Y2Linux', 'product': 'Y2', 'serialnumber': 'Y2LINUX-DEV-01'}.items():
            self.write('strings/0x409/' + name, value)
        self.mkdir('configs/c.1/strings/0x409')
        self.write('configs/c.1/strings/0x409/configuration', 'Music Transfer' if mode == 'mtp' else 'Developer')
        # The charger grants the validated SDP ceiling (450 mA) only for a configured
        # 500 mA bus-powered request (kernel/platform/source-policy.h). A self-powered,
        # 0 mA configuration made the allocation 0 and left the PC port "Not charging".
        self.write('configs/c.1/MaxPower', '500')
        self.write('configs/c.1/bmAttributes', '0x80')
        for link in root.joinpath('configs/c.1').iterdir():
            if link.is_symlink():
                link.unlink()
        for function in selection(mode):
            self.mkdir('functions/' + function)
            if function == 'ecm.usb0':
                self.write('functions/ecm.usb0/host_addr', '02:42:00:00:00:02')
                self.write('functions/ecm.usb0/dev_addr', '02:42:00:00:00:01')
            root.joinpath('configs/c.1', function).symlink_to(root.joinpath('functions', function))
        # FFS must be mounted and have its descriptors written BEFORE UDC bind.
        if mode == 'mtp':
            ffs = self.ctx.path('/dev/ffs-mtp')
            ffs.mkdir(parents=True, exist_ok=True)
            mounts = mountinfo(self.ctx.read('/proc/self/mountinfo'))
            if not any(m['path'] == '/dev/ffs-mtp' and m['filesystem'] == 'functionfs' for m in mounts):
                result = self.ctx.command(['mount', '-t', 'functionfs', 'mtp', '/dev/ffs-mtp'])
                if not result['ok']:
                    raise OSError('functionfs_mount_failed')
            self.ctx.path('/run/y2/umtprd.conf').write_text(config(views))
            self.ctx.path('/run/y2/mtp-expect.txt').write_text(expectations(views))
            self.child = self.launch()
        self.mode, self.views = mode, views
        self.mounted = {v['path']: identity(v) for v in views} if mode == 'mtp' else {}
        self.started = time.monotonic()

    def command(self, message):
        """Send one IPC command to the running responder."""
        try:
            answer = subprocess.run(['/usr/sbin/umtprd', '-cmd:' + message], stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3,
                env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LC_ALL': 'C'})
        except (OSError, subprocess.TimeoutExpired):
            return False
        return answer.returncode == 0

    def pinned(self):
        state = self.ctx.json('/run/y2/mtp-roots.json', {})
        if not self.child or state.get('pid') != self.child.pid:
            return None
        roots = state.get('roots')
        return set(roots) if isinstance(roots, list) else None

    def await_root(self, path, present, timeout=3.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            roots = self.pinned()
            if roots is not None and (path in roots) == present:
                return True
            time.sleep(.05)
        return False

    def reconcile(self, views):
        """Change the live MTP view without touching the USB gadget."""
        desired = {v['path']: v for v in views}
        for path in list(self.mounted):
            view = desired.get(path)
            if view is not None and self.mounted[path] == identity(view):
                continue
            # Leaving view: the responder drops its pinned root and open files
            # before the platform may unmount or replace the filesystem.
            if not (self.command('unmount:' + NAMES[path]) and self.await_root(path, False)):
                raise OSError('responder_unmount_timeout')
            del self.mounted[path]
        self.ctx.path('/run/y2/mtp-expect.txt').write_text(expectations(views))
        for path, view in desired.items():
            if path in self.mounted:
                continue
            message = ('mountro:' if view['access'] == 'ro' else 'mount:') + NAMES[path]
            if not (self.command(message) and self.await_root(path, True)):
                raise OSError('responder_mount_timeout')
            self.mounted[path] = identity(view)
        self.views = views
        self.refresh_generation = time.monotonic_ns()

    def record(self, state, reason, cable, activity=None):
        value = {'schema': SCHEMA, 'boot_id': self.ctx.read('/proc/sys/kernel/random/boot_id'),
                 'state': state, 'reason': reason, 'mode': self.mode, 'cable': cable,
                 'functions': list(selection(self.mode)) if self.mode else [],
                 'views': self.views or [], 'filesystem_owner': 'linux', 'block_export': False,
                 'refresh_generation': self.refresh_generation,
                 'active': bool((activity or {}).get('active')), 'error': (activity or {}).get('error'),
                 'restarts': self.failures}
        if value != self.last_record:
            atomic_json(self.ctx.path('/run/y2/usb-transfer.json'), value)
            self.last_record = value
        return value

    # A connected computer may write to the SD card at any time: the protected
    # card (media.py) is writable while the host has the player enumerated, and
    # read-only again once the cable is gone. Renewed every 10 s for 30 s.
    SD_WINDOW_RENEW = 10
    SD_WINDOW_SECONDS = 30

    def sd_writable(self, cable):
        from .media import write_window
        host = cable in ('configured', 'suspended')
        now = time.monotonic()
        try:
            if host and now - self.sd_window >= self.SD_WINDOW_RENEW:
                if write_window(self.ctx, 'usb', self.SD_WINDOW_SECONDS).get('holders'):
                    self.sd_window = now
            elif not host and self.sd_window:
                write_window(self.ctx, 'usb', release=True)
                self.sd_window = 0.0
        except (OSError, ValueError):
            pass

    def tick(self):
        self.ctx.path('/run/y2').mkdir(parents=True, exist_ok=True)
        udcs = self.ctx.glob('/sys/class/udc/*')
        if len(udcs) != 1:
            return self.record('Unavailable', 'no_unique_udc', None)
        cable = read(udcs[0] / 'state')
        self.sd_writable(cable)
        if self.terminal:
            return self.record('Failed', 'responder_restart_budget', cable)
        requested = self.ctx.json('/data/system/platform/usb-mode.json', {}).get('mode', 'mtp')
        try:
            selection(requested)
            # Reuse validated views until identity/options change. This adds no
            # recurring blkid process, storage read, or polling wakeup.
            from .radiofw import present
            signature = (present(self.ctx), self.ctx.read('/proc/self/mountinfo'),
                         self.ctx.read('/run/y2/internal-media-setup.json'),
                         self.ctx.read('/run/y2/internal-media.json'),
                         self.ctx.read('/run/y2/media-mount.json'),
                         self.ctx.read('/run/y2/usb-release-sd.json'),
                         [(p.name, read(p / 'dev'), read(p / 'device/cid'))
                          for p in self.ctx.glob('/sys/class/block/mmcblk*')])
            if signature != self.signature:
                self.cached_views = media_views(self.ctx)
                self.signature = signature
            views = self.cached_views if requested == 'mtp' else []
            if self.mode is not None and requested != self.mode and cable != 'not attached':
                requested = self.mode
                views = self.cached_views if self.mode == 'mtp' else []
            if self.mode != requested:
                if self.mode is not None:
                    self.refresh_generation = time.monotonic_ns()
                self.setup(requested, views)
            elif self.mode == 'mtp' and self.views != views and self.child and self.child.poll() is None:
                self.reconcile(views)
            if self.child and self.child.poll() is not None:
                self.stop()
                # A responder that served for a minute before dying is a new
                # incident, not another step toward the permanent-failure budget.
                if time.monotonic() - self.started > STABLE_SECONDS: self.failures = 0
                self.failures += 1
                if self.failures >= 5:
                    self.terminal = True
                    return self.record('Failed', 'responder_restart_budget', cable)
                self.mode = None
                return self.record('Starting', 'responder_restarting', cable)
            if not self.bound:
                ready = self.mode == 'developer' or (
                    self.ctx.read(GADGET + '/functions/ffs.mtp/ready') == '1' and
                    all(self.ctx.path('/dev/ffs-mtp/ep' + str(i)).exists() for i in (1, 2, 3)))
                if not ready:
                    if time.monotonic() - self.started > 15:
                        raise OSError('responder_descriptor_timeout')
                    return self.record('Starting', 'waiting_for_descriptors', cable)
                self.write('UDC', udcs[0].name)
                self.bound = self.ready = True
            activity = self.ctx.json('/run/y2/mtp-events.json', {})
            if not self.child or activity.get('pid') != self.child.pid:
                activity = {}
            generation = activity.get('generation', 0)
            if isinstance(generation, int) and generation > self.seen_generation:
                self.seen_generation = generation
                self.refresh_generation = time.monotonic_ns()
            if self.previous_cable == 'configured' and cable not in ('configured', 'suspended'):
                self.refresh_generation = time.monotonic_ns()
            self.previous_cable = cable
            state = 'Disconnected' if cable == 'not attached' else 'Ready' if cable == 'configured' else 'Starting'
            if state == 'Ready' and activity.get('active'):
                state = 'Copying'
            if state == 'Ready' and self.mode == 'mtp' and not self.views:
                state = 'Unavailable'
            return self.record(state, None, cable, activity)
        except (OSError, ValueError) as error:
            try:
                self.stop()
            except OSError:
                self.terminal = True
            if self.started and time.monotonic() - self.started > STABLE_SECONDS: self.failures = 0
            self.failures += 1
            self.terminal = self.terminal or self.failures >= 5
            if self.failures < 5:
                self.mode = None
            return self.record('Failed', str(error), cable)
