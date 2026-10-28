"""Differential cases of port group `service` (see tests/platform_cases/__init__.py): `update`, `firmware`,
`service-daemon`.

The update cases give the staging token through `Y2_FIXTURE_TOKEN` (the Rust side honours it below a fixture
root; `fixed_token` patches Python's uuid4 the same way), so the native core's `check-manifest` request has a
fixed argv the fixture can answer and the staging directory has a fixed name. The local package
(`service_package/`) stands in for a signed one: the signature is the core's to check, and the core is a
fixture command here.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time

PACKAGE = Path(__file__).resolve().parent / 'service_package'
TOKEN = '0123456789abcdef0123456789abcdef'
INCOMING = '/data/updates/.incoming-' + TOKEN
CORE = '/usr/sbin/y2-update-core'
FIXED = {'Y2_FIXTURE_TOKEN': TOKEN, 'Y2_LAUNCHER_PATCH': 'platform_cases.service:fixed_token'}


def fixed_token():
    """Y2_LAUNCHER_PATCH: Python's uuid4 gives the token the Rust side takes from Y2_FIXTURE_TOKEN."""
    import uuid
    fixed = uuid.UUID(hex=os.environ['Y2_FIXTURE_TOKEN'])
    uuid.uuid4 = lambda: fixed


def daemon_ticks():
    """Y2_LAUNCHER_PATCH for `service-daemon`: the loop ends after Y2_DAEMON_TICKS iterations (exit 0), its
    waits return at once, and the children it starts with subprocess.Popen itself (service.py and usb.py do
    so even below a fixture root) are recorded as fixture calls (timeout 0) and refused like a missing
    binary (OSError), never started on the host."""
    import errno
    import subprocess
    from y2_platform import network, service
    ticks = int(os.environ['Y2_DAEMON_TICKS'])
    loops = []
    wake = service.SignalWake.__init__

    def init(self, running):
        loops.append(running)
        wake(self, running)

    def wait(self, timeout, *, wake_fd=None):
        loops.append(None)
        if len(loops) - 1 >= ticks:
            loops[0][0] = False

    real = subprocess.Popen
    # The launcher's own --calls (Python side), or the fixture environment (the Rust fallback).
    import sys
    record = (sys.argv[sys.argv.index('--calls') + 1] if '--calls' in sys.argv
              else os.environ['Y2_FIXTURE_CALLS'])

    class Popen(real):
        def __init__(self, argv, *args, **kwargs):
            if isinstance(argv, (list, tuple)) and argv and str(argv[0]).startswith(('/usr/sbin/y2-platform',
                                                                                     '/usr/sbin/umtprd')):
                with open(record, 'a') as calls:
                    calls.write(json.dumps({'argv': [str(a) for a in argv], 'timeout': 0}) + '\n')
                raise OSError(errno.ENOENT, 'No such file or directory', str(argv[0]))
            super().__init__(argv, *args, **kwargs)

    service.SignalWake.__init__ = init
    network.EventMonitor.wait = wait
    subprocess.Popen = Popen


def answer(argv, output='', ok=True, reason=None):
    return {'argv': argv, 'ok': ok, 'reason': reason if reason or ok else 'command_failed', 'output': output}


def core(action, value=None, ok=True, directory=None, output=None):
    argv = [CORE, action] + ([directory] if directory else [])
    return answer(argv, output if output is not None else json.dumps(value if value is not None else {}), ok)


def payload(name):
    data = (PACKAGE / name).read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def manifest(**changes):
    value = {'release_version': '1.0.1-fixture', 'sequence': 11200, 'notes': 'Fixture notes',
             'reborn_version': 'reborn-fixture',
             # The core's order, not sorted: the payloads are fetched in it.
             'payloads': {'rootfs': {**payload('rootfs.ext4.gz'), 'raw_bytes': 4096},
                          'bootimg': payload('bootimg.img.gz'), 'recovery': payload('recovery.img.gz')}}
    value.update(changes)
    return value


# A verified Y2DATA mount: the device link resolves below the internal controller (a `..` path stands in
# for the sysfs symlink), and blkid names the Y2DATA filesystem.
MOUNTINFO = ('22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
             '30 22 ../../devices/platform/11230000.mmc/mmcblk0p9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n')
VOLUME = {'/proc/self/mountinfo': MOUNTINFO, '/sys/dev/block/.present': '',
          '/sys/devices/platform/11230000.mmc/mmcblk0p9/dev': '179:9\n'}
BLKID = answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], '79324c69-6e75-4801-8000-000000000102')
CHANNEL = {'/etc/y2linux/update-channel.json': json.dumps({
    'name': 'beta', 'manifest_url': 'https://releases.example/y2/manifest.json',
    'allowed_redirect_hosts': ['objects.example']})}
CHECK = core('check-manifest', manifest(), directory=INCOMING)
CHECKED = json.dumps({'schema': 1, 'state': 'Checked', 'release_version': '1.0.0-offered', 'sequence': 11100,
                      'download_bytes': 70, 'notes': '', 'reborn_version': ''})
STAGE = ['update', 'stage', '--package', str(PACKAGE)]
QUEUED = {'/data/updates/state.json': json.dumps({'schema': 2, 'state': 'Queued', 'release_version': '1.0.1'})}
PENDING = {'/data/updates/state.json': json.dumps({'schema': 2, 'state': 'PendingHealth'})}
CHARGING = {'/run/y2/power.json': json.dumps({'boot_id': 'boot-a', 'battery': {'charging': True}})}
READY = {'/run/y2/application-ready.json': json.dumps({'boot_id': 'boot-a', 'first_frame': True, 'pid': 1234,
                                                       'start_ticks': 777})}


# Every core health check health-ack needs OK: a writable root and Y2DATA on the internal controller with
# their filesystem identities, no ext4 errors, and the display, render, audio and input nodes.
HEALTHY = {'/proc/self/mountinfo': '22 1 ../../devices/platform/11230000.mmc/mmcblk0p2 / / rw - ext4 /dev/mmcblk0p2 rw\n'
                                   '30 22 ../../devices/platform/11230000.mmc/mmcblk0p9 / /data rw,noatime - ext4 '
                                   '/dev/mmcblk0p9 rw\n',
           '/sys/dev/block/.present': '', '/sys/devices/platform/11230000.mmc/mmcblk0p2/dev': '179:2\n',
           '/sys/devices/platform/11230000.mmc/mmcblk0p9/dev': '179:9\n',
           '/sys/fs/ext4/mmcblk0p9/errors_count': '0\n', '/dev/dri/card0': '', '/dev/dri/renderD128': '',
           '/dev/snd/pcmC0D0p': '', '/dev/input/event0': ''}
HEALTHY_BLKID = [answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p2'],
                        '79324c69-6e75-4801-8000-000000000101'), BLKID]
REBORNCTL = ['/usr/bin/rebornctl', 'status', '--json']


def stat_line(ticks):
    return '1234 (reborn) S ' + ' '.join(['1'] * 18) + ' %d 0\n' % ticks


def anchor(source='ntp', boot='boot-a'):
    """/run/y2/time.json as ntp-event writes it, made when the case runs."""
    return lambda: json.dumps({'schema': 1, 'boot_id': boot, 'epoch': time.time(), 'monotonic_s': time.monotonic(),
                               'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME), 'source': source},
                              sort_keys=True, separators=(',', ':'))


def last_check(result='checked', age=0.0):
    return lambda: json.dumps({'schema': 1, 'time': time.time() - age, 'attempt': time.time() - age, 'result': result})


ONLINE = {'/run/y2/network.json': '{"state":"Online"}'}

CASES = [
    # update status
    ('update-status', ['update', 'status'], {'ported': True}),
    ('update-status-journal', ['update', 'status'], {'ported': True, 'files': {
        **QUEUED, **CHANNEL, '/data/updates/channel.json': '{"name": "owner", "manifest_url": "https://x/manifest.json", "other": 1}',
        '/data/updates/download.json': '{"schema": 1, "state": "Staging", "id": "x"}',
        '/data/updates/last-check.json': '{"schema": 1, "attempt": 5, "result": "failed"}'}}),
    ('update-status-idle', ['update', 'status'], {'ported': True, 'files': {'/data/updates/.keep': ''}}),
    ('update-status-bad-journal', ['update', 'status'], {'ported': True, 'files': {
        '/data/updates/state.json': '{"schema": 3, "state": "Queued"}'}}),
    ('update-status-unreadable', ['update', 'status'], {'ported': True, 'files': {'/data/updates/state.json': 'x'}}),
    ('update-status-url-option', ['update', 'status', '--url', 'https://x/manifest.json'], {'ported': True}),
    ('update-status-bare-option', ['update', 'status', '--url'], {'ported': True}),
    ('update-option-first', ['update', '--package', '/x', 'status'], {'ported': True}),
    ('update-unknown-action', ['update', 'install'], {'ported': True}),
    # update check / stage
    ('update-check-no-source', ['update', 'check'], {'ported': True}),
    ('update-check-no-data', ['update', 'check'], {'ported': True, 'files': CHANNEL}),
    ('update-check-both', ['update', 'check', '--url', 'https://x/manifest.json', '--package', str(PACKAGE)],
     {'ported': True}),
    ('update-check-maintenance', ['update', 'check'], {'ported': True, 'commands': [BLKID], 'files': {
        **CHANNEL, **VOLUME, '/data/system/platform/maintenance-pending': ''}}),
    ('update-check-clock', ['update', 'check'], {'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {
        **CHANNEL, **VOLUME}}),
    ('update-check-clock-keeps-offer', ['update', 'check'], {'ported': True, 'env': FIXED, 'commands': [BLKID],
                                                             'files': {**CHANNEL, **VOLUME, '/data/updates/download.json': CHECKED}}),
    ('update-check-wrong-blkid', ['update', 'check'], {'ported': True, 'files': {**CHANNEL, **VOLUME},
                                                       'commands': [answer(BLKID['argv'], 'other')]}),
    ('update-check-package', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'commands': [BLKID, CHECK], 'files': VOLUME}),
    ('update-check-up-to-date', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core(
            'check-manifest', ok=False, directory=INCOMING,
            output=json.dumps({'failure': 'manifest sequence downgrade/replay refused'}))]}),
    ('update-check-core-refuses', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'files': {**VOLUME, '/data/updates/download.json': CHECKED},
        'commands': [BLKID, core('check-manifest', ok=False, directory=INCOMING,
                                 output=json.dumps({'failure': 'manifest signature invalid'}))]}),
    ('update-check-core-unanswered', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-core-empty', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core('check-manifest', directory=INCOMING,
                                                                               output='')]}),
    ('update-check-core-list', ['update', 'check', '--package', str(PACKAGE)], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core('check-manifest', directory=INCOMING,
                                                                               output='[1]')]}),
    ('update-check-not-a-directory', ['update', 'check', '--package', str(PACKAGE / 'manifest.json')], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-missing-package', ['update', 'check', '--package', str(PACKAGE / 'missing')], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-no-signature', ['update', 'check', '--package', str(PACKAGE.parent)], {
        'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-stage', STAGE, {'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, CHECK, core('queue', {'state': 'Queued', 'release_version': '1.0.1-fixture'})]}),
    ('update-stage-replaces-pending', STAGE, {'ported': True, 'env': FIXED, 'commands': [BLKID, CHECK], 'files': {
        **VOLUME, '/data/updates/pending/manifest.json': '{}', '/data/updates/.incoming-old/manifest.sig': 'x',
        '/run/y2/update-check-progress': '50 check\n'}}),
    ('update-stage-hash-mismatch', STAGE, {'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {**payload('rootfs.ext4.gz'), 'raw_bytes': 1},
                                                          'bootimg': {**payload('bootimg.img.gz'), 'sha256': '0' * 64},
                                                          'recovery': payload('recovery.img.gz')}),
                    directory=INCOMING)]}),
    ('update-stage-incomplete', STAGE, {'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {'bytes': 40, 'sha256': '', 'raw_bytes': 0}}),
                    directory=INCOMING)]}),
    ('update-stage-too-large', STAGE, {'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {'bytes': 5, 'sha256': '', 'raw_bytes': 0}}),
                    directory=INCOMING)]}),
    ('update-stage-queue-refused', STAGE, {'ported': True, 'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, CHECK, core('queue', ok=False, output='{"failure": "payload changed after queueing"}')]}),
    ('update-stage-pending', STAGE, {'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {**VOLUME, **QUEUED}}),
    ('update-stage-excess', STAGE, {'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, **{'/data/updates/.incoming-%d/manifest.json' % i: '{}' for i in range(9)}}}),
    ('update-stage-unknown-leftover', STAGE, {'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, '/data/updates/.incoming-old/notes.txt': 'x'}}),
    ('update-stage-unsafe-pending', STAGE, {'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, '/data/updates/pending': 'a file'}}),
    # update apply / rollback / cancel / probe / recovery status
    ('update-apply-nothing-queued', ['update', 'apply'], {'ported': True}),
    ('update-apply-battery', ['update', 'apply'], {'ported': True, 'files': QUEUED}),
    ('update-apply-other-boot', ['update', 'apply'], {'ported': True, 'files': {**QUEUED, '/run/y2/power.json': json.dumps(
        {'boot_id': 'boot-z', 'battery': {'charging': True}})}}),
    ('update-apply-half-battery', ['update', 'apply'], {'ported': True, 'commands': [core('prepare', {'state': 'Prepared'})],
                                                         'files': {**QUEUED, '/run/y2/power.json': json.dumps(
                                                             {'battery': {'percent': 50, 'status': 'Discharging'}})}}),
    ('update-apply-prepare-refused', ['update', 'apply'], {'ported': True, 'files': {**QUEUED, **CHARGING}}),
    ('update-apply-poweroff', ['update', 'apply'], {'ported': True, 'commands': [core('prepare', {'state': 'Prepared'})],
                                                    'files': {**QUEUED, **CHARGING,
                                                              '/data/updates/restart.json': '{"mode": "poweroff"}'}}),
    ('update-rollback-refused', ['update', 'rollback'], {'ported': True}),
    ('update-rollback-battery', ['update', 'rollback'], {'ported': True, 'commands': [core('rollback', {'state': 'RollbackPending'})]}),
    ('update-cancel', ['update', 'cancel'], {'ported': True, 'commands': [core('cancel', {'state': 'Idle'})], 'files': {
        '/data/updates/pending/manifest.json': '{}', '/data/updates/pending/rootfs.ext4.gz': 'x'}}),
    ('update-cancel-refused', ['update', 'cancel'], {'ported': True, 'commands': [
        core('cancel', ok=False, output='{"failure": 7}')]}),
    ('update-cancel-unsafe', ['update', 'cancel'], {'ported': True, 'commands': [core('cancel', {'state': 'Idle'})],
                                                    'files': {'/data/updates/pending/other': 'x'}}),
    ('update-probe-recovery', ['update', 'probe-recovery'], {'ported': True, 'commands': [core('probe', {'ok': True})]}),
    ('update-probe-refused', ['update', 'probe-recovery'], {'ported': True}),
    ('update-recovery-status', ['update', 'recovery-status'], {'ported': True, 'commands': [
        core('misc', {'command': 'boot-recovery', 'recovery': '--update'})]}),
    ('update-recovery-status-failed', ['update', 'recovery-status'], {'ported': True, 'commands': [
        core('misc', ok=False, output='not json')]}),
    # update health-ack
    ('update-health-ack-idle', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True}),
    ('update-health-ack-first-frame', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': PENDING}),
    ('update-health-ack-pid', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true, "pid": "1"}'}}),
    ('update-health-ack-bad-pid', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true, "pid": 5000000}'}}),
    ('update-health-ack-generation', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, **READY, '/proc/1234/stat': stat_line(778)}}),
    ('update-health-ack-expired', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, '/run/y2/update-health.json': '{"boot_id": "boot-a", "deadline_monotonic_s": 0}'},
        'commands': [core('rollback', {'state': 'RollbackPending'}), core('prepare', {'state': 'Prepared'})]}),
    ('update-health-ack-expired-refused', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, '/run/y2/update-health.json': '{"boot_id": "boot-a", "deadline_monotonic_s": 1.5}'}}),
    ('update-health-ack-checks', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': True, 'files': {
        **PENDING, **READY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-app-not-ready', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'ported': True, 'commands': HEALTHY_BLKID, 'files': {
            **PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-app-not-json', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'ported': True, 'commands': [*HEALTHY_BLKID, answer(REBORNCTL, 'x')],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-acknowledged', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'ported': True,
        'commands': [*HEALTHY_BLKID, answer(REBORNCTL, '{"healthy": true}'), core('ack', {'state': 'Acknowledged'})],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777),
                  '/data/system/platform/boot.json': json.dumps({'schema': 1, 'boot_id': 'boot-a',
                                                                 'last_stage': 'services_started'}),
                  '/run/y2/health-debug.json': '{"printk": "4\\t4 1 7", "panic": "0", "panic_on_rcu_stall": "1"}',
                  '/proc/sys/kernel/printk': '7 4 1 7\n', '/proc/sys/kernel/panic': '0\n',
                  '/proc/sys/kernel/panic_on_rcu_stall': '1\n',
                  '/data/updates/pending/manifest.json': '{}'}}),
    ('update-health-ack-graphics', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'ported': True,
        'commands': [*HEALTHY_BLKID, answer(REBORNCTL, '{"graphics": {"available": true}, "uptime_seconds": 9}')],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    # update auto-check (the service's daily check)
    ('update-auto-check-not-due', ['update', 'auto-check'], {'volatile': ['attempt'], 'ported': True, 'files': {
        '/data/updates/last-check.json': last_check()}}),
    ('update-auto-check-no-channel', ['update', 'auto-check'], {'volatile': ['attempt'], 'ported': True, 'files': {
        '/data/updates/last-check.json': last_check('failed')}}),
    ('update-auto-check-offline', ['update', 'auto-check'], {'volatile': ['attempt'], 'ported': True, 'files': {
        **CHANNEL, '/data/updates/last-check.json': last_check(age=90000)}}),
    ('update-auto-check-clock', ['update', 'auto-check'], {'volatile': ['attempt'], 'ported': True, 'files': {**CHANNEL, **ONLINE}}),
    ('update-auto-check-fails', ['update', 'auto-check'], {'volatile': ['attempt'], 
        'ported': True, 'env': FIXED, 'commands': [BLKID], 'files': {
            **CHANNEL, **ONLINE, **VOLUME, '/run/y2/time.json': anchor(),
            '/data/updates/download.json': '{"schema": 1, "state": "Staging", "id": "y"}',
            '/data/updates/last-check.json': last_check('failed', age=4000)}}),
    ('update-auto-check-fails-keeps-offer', ['update', 'auto-check'], {'volatile': ['attempt'], 
        'ported': True, 'env': FIXED, 'files': {**CHANNEL, **ONLINE, '/run/y2/time.json': anchor(),
                                                '/data/updates/download.json': CHECKED}}),
    ('update-auto-check-odd-url', ['update', 'auto-check'], {'volatile': ['attempt'], 'ported': False, 'files': {
        '/etc/y2linux/update-channel.json': '{"manifest_url": 5}'}}),
]


# ---- firmware -------------------------------------------------------------------------------------------
# Owner copies of system.img below /srv/radiofw/<name>/Firmware (an MTP root the cases name): ext4 and ext3
# images with stand-in firmware (the reviewed hashes are the real files'), so a scan reads the image, the
# files and the reader library and refuses at the library's hash, or earlier.
SEEN = {}


def mtp_root(name, state=None):
    """The case files: /srv/radiofw/<name> exported, and the previous pass having seen this copy."""
    files = {'/run/y2/mtp-roots.json': json.dumps({'roots': ['/srv/radiofw/' + name, '/data/firmware/inbox']})}
    if state:
        files['/run/y2/radio-firmware.json'] = lambda: json.dumps(
            {'schema': 1, 'state': state, 'seen': SEEN.get(name), 'reason': 'firmware_mismatch:old'})
    return files


def sparse(raw, block):
    """Android sparse encoding: raw chunks, zero runs as don't-care, one fill (tests/test_radio_firmware.py)."""
    blocks = [raw[i:i + block].ljust(block, b'\0') for i in range(0, len(raw), block)]
    chunks, i = [], 0
    while i < len(blocks):
        j = i
        if blocks[i] == bytes(block):
            while j < len(blocks) and blocks[j] == bytes(block):
                j += 1
            chunks.append(struct.pack('<HHII', 0xCAC3, 0, j - i, 12))
        else:
            while j < len(blocks) and blocks[j] != bytes(block):
                j += 1
            chunks.append(struct.pack('<HHII', 0xCAC1, 0, j - i, 12 + (j - i) * block) + b''.join(blocks[i:j]))
        i = j
    chunks.append(struct.pack('<HHII', 0xCAC2, 0, 2, 16) + b'\xaa\xbb\xcc\xdd')
    header = struct.pack('<IHHHHIIII', 0xED26FF3A, 1, 0, 28, 12, block, len(blocks) + 2, len(chunks), 0)
    return header + b''.join(chunks)


def firmware_tree(directory, omit=(), large=()):
    sizes = {'WIFI_RAM_CODE_MT6582': 3000, 'WMT_SOC.cfg': 80, 'mt6572_82_patch_e1_0_hdr.bin': 64,
             'mt6572_82_patch_e1_1_hdr.bin': 64, 'modem_1_2g_n.img': 9000}
    (directory / 'etc/firmware').mkdir(parents=True)
    (directory / 'lib').mkdir()
    for name, size in sizes.items():
        if name not in omit:
            size += 1 if name in large else 0
            (directory / 'etc/firmware' / name).write_bytes(bytes((i * 7 + len(name)) % 256 for i in range(size)))
    library = bytearray(i * 13 % 256 for i in range(51224))
    library[:7] = b'\x7fELF\x01\x01\x01'
    (directory / 'lib/libcustom_nvram.so').write_bytes(bytes(library))


def mke2fs(tree, image, options, size):
    command = shutil.which('mke2fs')
    if not command:
        return False
    try:
        return subprocess.run([command, '-q', '-F', *options, '-d', str(tree), str(image), size],
                              capture_output=True).returncode == 0
    except OSError:
        return False


IMAGES = {
    # name: (tree options, mke2fs options, size, sparse block or None for a raw image)
    'ext4': ({}, ['-b', '1024', '-t', 'ext4'], '1M', None),
    'ext3': ({}, ['-b', '1024', '-O', '^extent,^64bit', '-t', 'ext3'], '1M', 1024),
    'partial': ({'omit': ('modem_1_2g_n.img',)}, ['-b', '4096', '-t', 'ext4'], '2M', 4096),
    'large': ({'large': ('WMT_SOC.cfg',)}, ['-b', '1024', '-t', 'ext4'], '1M', 1024),
}


def firmware_images(root):
    """Without a host mke2fs only the images that need none are made; their cases then see no copy."""
    old = time.time() - 3600
    with tempfile.TemporaryDirectory() as scratch:
        for name, (tree_options, options, size, block) in IMAGES.items():
            tree = Path(scratch) / name
            firmware_tree(tree, **tree_options)
            raw = Path(scratch) / (name + '.img')
            if not mke2fs(tree, raw, options, size):
                continue
            target = root / 'srv/radiofw' / name / 'Firmware/system.img'
            target.parent.mkdir(parents=True)
            target.write_bytes(raw.read_bytes() if block is None else sparse(raw.read_bytes(), block))
    for name, data in (('text', b'not an image\n'),
                       ('badsparse', struct.pack('<IHHHHIIII', 0xED26FF3A, 2, 0, 28, 12, 4096, 0, 0, 0))):
        target = root / 'srv/radiofw' / name / 'Firmware/system.img'
        target.parent.mkdir(parents=True)
        target.write_bytes(data)
    for image in (root / 'srv/radiofw').glob('*/Firmware/system.img'):
        os.utime(image, (old, old))
        info = image.lstat()
        SEEN[image.parent.parent.name] = [info.st_size, info.st_mtime_ns]


INTERNAL = {'/run/y2/internal-media.json': json.dumps({'state': 'Ready', 'boot_id': 'boot-a', 'mount_id': 40,
                                                       'device_id': '179:8'}),
            '/proc/self/mountinfo': '22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
                                    '30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n'
                                    '40 22 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw\n'}

CASES += [
    ('firmware-status', ['firmware', 'status'], {'ported': True}),
    ('firmware-status-path', ['firmware', 'status', 'ignored'], {'ported': True}),
    ('firmware-status-installing', ['firmware', 'status'], {'ported': True, 'files': {
        '/run/y2/radio-firmware.json': '{"schema": 1, "state": "Installing", "seen": [1, 2]}'}}),
    ('firmware-status-no-space', ['firmware', 'status'], {'ported': True, 'files': {
        '/run/y2/mtp-events.json': '{"error": 28.0}'}}),
    ('firmware-status-internal', ['firmware', 'status'], {'ported': True, 'files': INTERNAL}),
    ('firmware-status-partly-there', ['firmware', 'status'], {'ported': True, 'files': {
        '/data/firmware/mt6582/WMT_SOC.cfg': 'x' * 80}}),
    ('firmware-scan', ['firmware', 'scan'], {'ported': True}),
    ('firmware-scan-internal', ['firmware', 'scan'], {'ported': True, 'files': {
        **INTERNAL, '/media/internal/music/.keep': ''}}),
    ('firmware-scan-internal-unmounted', ['firmware', 'scan'], {'ported': True, 'files': INTERNAL}),
    ('firmware-scan-rejected-copy-gone', ['firmware', 'scan'], {'ported': True, 'files': {
        '/run/y2/radio-firmware.json': '{"schema": 1, "state": "Failed", "reason": "x", "seen": [1, 2]}'}}),
    ('firmware-scan-new-copy', ['firmware', 'scan'], {'ported': True, 'files': mtp_root('ext4')}),
    # The copy is written when the case runs: its time is a clock reading.
    ('firmware-scan-arriving', ['firmware', 'scan'], {'ported': True, 'volatile': ['seen'], 'files': {
        '/data/firmware/inbox/system.img': 'still copying'}}),
    ('firmware-scan-rejected-before', ['firmware', 'scan'], {'ported': True, 'files': mtp_root('ext4', 'Failed')}),
    ('firmware-install-dir', ['firmware', 'install', str(PACKAGE)], {'ported': True}),
    ('firmware-install-image', ['firmware', 'install', str(PACKAGE / 'manifest.json')], {'ported': True}),
    ('firmware-install-missing', ['firmware', 'install', str(PACKAGE / 'system.img')], {'ported': True}),
    ('firmware-install-no-path', ['firmware', 'install'], {'ported': True}),
    ('firmware-help', ['firmware', '--help'], {'ported': True}),
    ('firmware-bad-action', ['firmware', 'flash'], {'ported': True}),
] + [('firmware-scan-' + name, ['firmware', 'scan'], {'ported': True, 'files': mtp_root(name, 'Missing')})
     for name in ('ext4', 'ext3', 'partial', 'large', 'text', 'badsparse')]


# ---- service-daemon (still Python: its tick is mostly other groups' code, see service/mod.rs) -------------
TICKS = {'Y2_LAUNCHER_PATCH': 'platform_cases.service:daemon_ticks'}
DAEMON = {'ported': False, 'volatile': ['attempt', 'deadline_monotonic_s']}
CASES += [
    ('service-daemon-one-tick', ['service-daemon'], {**DAEMON, 'env': {**TICKS, 'Y2_DAEMON_TICKS': '1'}}),
    ('service-daemon-three-ticks', ['service-daemon'], {**DAEMON, 'env': {**TICKS, 'Y2_DAEMON_TICKS': '3'}}),
    ('service-daemon-dark-settled', ['service-daemon'], {**DAEMON, 'env': {**TICKS, 'Y2_DAEMON_TICKS': '2'}, 'files': {
        '/sys/class/backlight/panel/bl_power': '4\n', '/sys/class/backlight/panel/brightness': '200\n',
        '/run/y2/media-lifecycle.json': '{"inventory": []}',
        '/data/updates/last-check.json': last_check(),
        '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true}'},
        'commands': [answer(['/usr/sbin/y2-platform', 'update', 'health-ack'], '{"state": "Acknowledged"}')]}),
    ('service-daemon-online', ['service-daemon'], {**DAEMON, 'env': {**TICKS, 'Y2_DAEMON_TICKS': '1'}, 'files': {
        **ONLINE, '/run/y2/media-mount.json': '{"read_only": "checking", "boot_id": "boot-a"}'}}),
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path)."""
    firmware_images(root)
