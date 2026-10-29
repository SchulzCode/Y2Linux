"""Golden cases of port group `service` (see tests/platform_cases/__init__.py): `update`, `firmware`,
`service-daemon`.

The update cases give the staging token through `Y2_FIXTURE_TOKEN` (honoured below a fixture root), so the
native core's `check-manifest` request has a fixed argv the fixture can answer and the staging directory has a
fixed name. The local package (`service_package/`) stands in for a signed one: the signature is the core's to
check, and the core is a fixture command here.
"""
import gzip
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
FIXED = {'Y2_FIXTURE_TOKEN': TOKEN}


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
    ('update-status', ['update', 'status'], {}),
    ('update-status-journal', ['update', 'status'], {'files': {
        **QUEUED, **CHANNEL, '/data/updates/channel.json': '{"name": "owner", "manifest_url": "https://x/manifest.json", "other": 1}',
        '/data/updates/download.json': '{"schema": 1, "state": "Staging", "id": "x"}',
        '/data/updates/last-check.json': '{"schema": 1, "attempt": 5, "result": "failed"}'}}),
    ('update-status-idle', ['update', 'status'], {'files': {'/data/updates/.keep': ''}}),
    ('update-status-bad-journal', ['update', 'status'], {'files': {
        '/data/updates/state.json': '{"schema": 3, "state": "Queued"}'}}),
    ('update-status-unreadable', ['update', 'status'], {'files': {'/data/updates/state.json': 'x'}}),
    ('update-status-url-option', ['update', 'status', '--url', 'https://x/manifest.json'], {}),
    ('update-status-bare-option', ['update', 'status', '--url'], {}),
    ('update-option-first', ['update', '--package', '/x', 'status'], {}),
    ('update-unknown-action', ['update', 'install'], {}),
    # update check / stage
    ('update-check-no-source', ['update', 'check'], {}),
    ('update-check-no-data', ['update', 'check'], {'files': CHANNEL}),
    # A channel whose manifest URL is no text: refused before anything is written.
    ('update-check-odd-url', ['update', 'check'], {'files': {
        '/etc/y2linux/update-channel.json': '{"manifest_url": 5}'}}),
    ('update-check-both', ['update', 'check', '--url', 'https://x/manifest.json', '--package', str(PACKAGE)],
     {}),
    ('update-check-maintenance', ['update', 'check'], {'commands': [BLKID], 'files': {
        **CHANNEL, **VOLUME, '/data/system/platform/maintenance-pending': ''}}),
    ('update-check-clock', ['update', 'check'], {'env': FIXED, 'commands': [BLKID], 'files': {
        **CHANNEL, **VOLUME}}),
    ('update-check-clock-keeps-offer', ['update', 'check'], {'env': FIXED, 'commands': [BLKID],
                                                             'files': {**CHANNEL, **VOLUME, '/data/updates/download.json': CHECKED}}),
    ('update-check-wrong-blkid', ['update', 'check'], {'files': {**CHANNEL, **VOLUME},
                                                       'commands': [answer(BLKID['argv'], 'other')]}),
    ('update-check-package', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'commands': [BLKID, CHECK], 'files': VOLUME}),
    ('update-check-up-to-date', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core(
            'check-manifest', ok=False, directory=INCOMING,
            output=json.dumps({'failure': 'manifest sequence downgrade/replay refused'}))]}),
    ('update-check-core-refuses', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'files': {**VOLUME, '/data/updates/download.json': CHECKED},
        'commands': [BLKID, core('check-manifest', ok=False, directory=INCOMING,
                                 output=json.dumps({'failure': 'manifest signature invalid'}))]}),
    ('update-check-core-unanswered', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-core-empty', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core('check-manifest', directory=INCOMING,
                                                                               output='')]}),
    ('update-check-core-list', ['update', 'check', '--package', str(PACKAGE)], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID, core('check-manifest', directory=INCOMING,
                                                                               output='[1]')]}),
    ('update-check-not-a-directory', ['update', 'check', '--package', str(PACKAGE / 'manifest.json')], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-missing-package', ['update', 'check', '--package', str(PACKAGE / 'missing')], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-check-no-signature', ['update', 'check', '--package', str(PACKAGE.parent)], {
        'env': FIXED, 'files': VOLUME, 'commands': [BLKID]}),
    ('update-stage', STAGE, {'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, CHECK, core('queue', {'state': 'Queued', 'release_version': '1.0.1-fixture'})]}),
    ('update-stage-replaces-pending', STAGE, {'env': FIXED, 'commands': [BLKID, CHECK], 'files': {
        **VOLUME, '/data/updates/pending/manifest.json': '{}', '/data/updates/.incoming-old/manifest.sig': 'x',
        '/run/y2/update-check-progress': '50 check\n'}}),
    ('update-stage-hash-mismatch', STAGE, {'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {**payload('rootfs.ext4.gz'), 'raw_bytes': 1},
                                                          'bootimg': {**payload('bootimg.img.gz'), 'sha256': '0' * 64},
                                                          'recovery': payload('recovery.img.gz')}),
                    directory=INCOMING)]}),
    ('update-stage-incomplete', STAGE, {'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {'bytes': 40, 'sha256': '', 'raw_bytes': 0}}),
                    directory=INCOMING)]}),
    ('update-stage-too-large', STAGE, {'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, core('check-manifest', manifest(payloads={'rootfs': {'bytes': 5, 'sha256': '', 'raw_bytes': 0}}),
                    directory=INCOMING)]}),
    ('update-stage-queue-refused', STAGE, {'env': FIXED, 'files': VOLUME, 'commands': [
        BLKID, CHECK, core('queue', ok=False, output='{"failure": "payload changed after queueing"}')]}),
    ('update-stage-pending', STAGE, {'env': FIXED, 'commands': [BLKID], 'files': {**VOLUME, **QUEUED}}),
    ('update-stage-excess', STAGE, {'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, **{'/data/updates/.incoming-%d/manifest.json' % i: '{}' for i in range(9)}}}),
    ('update-stage-unknown-leftover', STAGE, {'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, '/data/updates/.incoming-old/notes.txt': 'x'}}),
    ('update-stage-unsafe-pending', STAGE, {'env': FIXED, 'commands': [BLKID], 'files': {
        **VOLUME, '/data/updates/pending': 'a file'}}),
    # update apply / rollback / cancel / probe / recovery status
    ('update-apply-nothing-queued', ['update', 'apply'], {}),
    ('update-apply-battery', ['update', 'apply'], {'files': QUEUED}),
    ('update-apply-other-boot', ['update', 'apply'], {'files': {**QUEUED, '/run/y2/power.json': json.dumps(
        {'boot_id': 'boot-z', 'battery': {'charging': True}})}}),
    ('update-apply-half-battery', ['update', 'apply'], {'commands': [core('prepare', {'state': 'Prepared'})],
                                                         'files': {**QUEUED, '/run/y2/power.json': json.dumps(
                                                             {'battery': {'percent': 50, 'status': 'Discharging'}})}}),
    ('update-apply-prepare-refused', ['update', 'apply'], {'files': {**QUEUED, **CHARGING}}),
    ('update-apply-poweroff', ['update', 'apply'], {'commands': [core('prepare', {'state': 'Prepared'})],
                                                    'files': {**QUEUED, **CHARGING,
                                                              '/data/updates/restart.json': '{"mode": "poweroff"}'}}),
    # On a charger (the battery gate comes first since 2026-10-29): the core's own refusal.
    ('update-rollback-refused', ['update', 'rollback'], {'files': {
        '/run/y2/power.json': json.dumps({'boot_id': 'boot-a', 'battery': {'charging': True}})}}),
    ('update-rollback-battery', ['update', 'rollback'], {'commands': [core('rollback', {'state': 'RollbackPending'})]}),
    ('update-cancel', ['update', 'cancel'], {'commands': [core('cancel', {'state': 'Idle'})], 'files': {
        '/data/updates/pending/manifest.json': '{}', '/data/updates/pending/rootfs.ext4.gz': 'x'}}),
    ('update-cancel-refused', ['update', 'cancel'], {'commands': [
        core('cancel', ok=False, output='{"failure": 7}')]}),
    ('update-cancel-unsafe', ['update', 'cancel'], {'commands': [core('cancel', {'state': 'Idle'})],
                                                    'files': {'/data/updates/pending/other': 'x'}}),
    ('update-probe-recovery', ['update', 'probe-recovery'], {'commands': [core('probe', {'ok': True})]}),
    ('update-probe-refused', ['update', 'probe-recovery'], {}),
    ('update-recovery-status', ['update', 'recovery-status'], {'commands': [
        core('misc', {'command': 'boot-recovery', 'recovery': '--update'})]}),
    ('update-recovery-status-failed', ['update', 'recovery-status'], {'commands': [
        core('misc', ok=False, output='not json')]}),
    # update health-ack
    ('update-health-ack-idle', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s']}),
    ('update-health-ack-first-frame', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': PENDING}),
    ('update-health-ack-pid', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true, "pid": "1"}'}}),
    ('update-health-ack-bad-pid', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true, "pid": 5000000}'}}),
    ('update-health-ack-generation', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, **READY, '/proc/1234/stat': stat_line(778)}}),
    ('update-health-ack-expired', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, '/run/y2/update-health.json': '{"boot_id": "boot-a", "deadline_monotonic_s": 0}'},
        'commands': [core('rollback', {'state': 'RollbackPending'}), core('prepare', {'state': 'Prepared'})]}),
    ('update-health-ack-expired-refused', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, '/run/y2/update-health.json': '{"boot_id": "boot-a", "deadline_monotonic_s": 1.5}'}}),
    # A health record of this boot whose deadline is no number: refused, nothing changed.
    ('update-health-ack-odd-deadline', ['update', 'health-ack'], {'files': {
        **PENDING, '/run/y2/update-health.json': '{"boot_id": "boot-a", "deadline_monotonic_s": "soon"}'}}),
    ('update-health-ack-checks', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'files': {
        **PENDING, **READY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-app-not-ready', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'commands': HEALTHY_BLKID, 'files': {
            **PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-app-not-json', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'], 'commands': [*HEALTHY_BLKID, answer(REBORNCTL, 'x')],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    ('update-health-ack-acknowledged', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'],
        'commands': [*HEALTHY_BLKID, answer(REBORNCTL, '{"healthy": true}'), core('ack', {'state': 'Acknowledged'})],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777),
                  '/data/system/platform/boot.json': json.dumps({'schema': 1, 'boot_id': 'boot-a',
                                                                 'last_stage': 'services_started'}),
                  '/run/y2/health-debug.json': '{"printk": "4\\t4 1 7", "panic": "0", "panic_on_rcu_stall": "1"}',
                  '/proc/sys/kernel/printk': '7 4 1 7\n', '/proc/sys/kernel/panic': '0\n',
                  '/proc/sys/kernel/panic_on_rcu_stall': '1\n',
                  '/data/updates/pending/manifest.json': '{}'}}),
    ('update-health-ack-graphics', ['update', 'health-ack'], {
        'volatile': ['deadline_monotonic_s'],
        'commands': [*HEALTHY_BLKID, answer(REBORNCTL, '{"graphics": {"available": true}, "uptime_seconds": 9}')],
        'files': {**PENDING, **READY, **HEALTHY, '/proc/1234/stat': stat_line(777)}}),
    # update auto-check (the service's daily check)
    ('update-auto-check-not-due', ['update', 'auto-check'], {'volatile': ['attempt'], 'files': {
        '/data/updates/last-check.json': last_check()}}),
    ('update-auto-check-no-channel', ['update', 'auto-check'], {'volatile': ['attempt'], 'files': {
        '/data/updates/last-check.json': last_check('failed')}}),
    ('update-auto-check-offline', ['update', 'auto-check'], {'volatile': ['attempt'], 'files': {
        **CHANNEL, '/data/updates/last-check.json': last_check(age=90000)}}),
    ('update-auto-check-clock', ['update', 'auto-check'], {'volatile': ['attempt'], 'files': {**CHANNEL, **ONLINE}}),
    ('update-auto-check-fails', ['update', 'auto-check'], {'volatile': ['attempt'], 
        'env': FIXED, 'commands': [BLKID], 'files': {
            **CHANNEL, **ONLINE, **VOLUME, '/run/y2/time.json': anchor(),
            '/data/updates/download.json': '{"schema": 1, "state": "Staging", "id": "y"}',
            '/data/updates/last-check.json': last_check('failed', age=4000)}}),
    ('update-auto-check-fails-keeps-offer', ['update', 'auto-check'], {'volatile': ['attempt'], 
        'env': FIXED, 'files': {**CHANNEL, **ONLINE, '/run/y2/time.json': anchor(),
                                                '/data/updates/download.json': CHECKED}}),
    ('update-auto-check-odd-url', ['update', 'auto-check'], {'volatile': ['attempt'], 'files': {
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


IMAGE_FIXTURES = Path(__file__).resolve().parent / 'firmware_images'
# The copies' time: long settled (a copy changed within 5 s is still arriving), and fixed, as `seen` records it.
SETTLED = 1788000000


def make_firmware_images():
    """Rewrite firmware_images/<name>.img.gz (IMAGES, with the host's mke2fs): the goldens of the firmware
    cases read these files, so a rewrite re-records them."""
    with tempfile.TemporaryDirectory() as scratch:
        for name, (tree_options, options, size, block) in IMAGES.items():
            tree = Path(scratch) / name
            firmware_tree(tree, **tree_options)
            raw = Path(scratch) / (name + '.img')
            if not mke2fs(tree, raw, options, size):
                raise SystemExit('mke2fs failed for ' + name)
            data = raw.read_bytes() if block is None else sparse(raw.read_bytes(), block)
            (IMAGE_FIXTURES / (name + '.img.gz')).write_bytes(gzip.compress(data, compresslevel=9, mtime=0))


def firmware_images(root):
    """The owner copies below `root`: the kept images (make_firmware_images), a text file, a bad sparse header."""
    for name in IMAGES:
        target = root / 'srv/radiofw' / name / 'Firmware/system.img'
        target.parent.mkdir(parents=True)
        target.write_bytes(gzip.decompress((IMAGE_FIXTURES / (name + '.img.gz')).read_bytes()))
    for name, data in (('text', b'not an image\n'),
                       ('badsparse', struct.pack('<IHHHHIIII', 0xED26FF3A, 2, 0, 28, 12, 4096, 0, 0, 0))):
        target = root / 'srv/radiofw' / name / 'Firmware/system.img'
        target.parent.mkdir(parents=True)
        target.write_bytes(data)
    for image in (root / 'srv/radiofw').glob('*/Firmware/system.img'):
        os.utime(image, (SETTLED, SETTLED))
        info = image.lstat()
        SEEN[image.parent.parent.name] = [info.st_size, info.st_mtime_ns]


INTERNAL = {'/run/y2/internal-media.json': json.dumps({'state': 'Ready', 'boot_id': 'boot-a', 'mount_id': 40,
                                                       'device_id': '179:8'}),
            '/proc/self/mountinfo': '22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
                                    '30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n'
                                    '40 22 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw\n'}

CASES += [
    ('firmware-status', ['firmware', 'status'], {}),
    ('firmware-status-path', ['firmware', 'status', 'ignored'], {}),
    ('firmware-status-installing', ['firmware', 'status'], {'files': {
        '/run/y2/radio-firmware.json': '{"schema": 1, "state": "Installing", "seen": [1, 2]}'}}),
    ('firmware-status-no-space', ['firmware', 'status'], {'files': {
        '/run/y2/mtp-events.json': '{"error": 28.0}'}}),
    ('firmware-status-internal', ['firmware', 'status'], {'files': INTERNAL}),
    ('firmware-status-partly-there', ['firmware', 'status'], {'files': {
        '/data/firmware/mt6582/WMT_SOC.cfg': 'x' * 80}}),
    ('firmware-scan', ['firmware', 'scan'], {}),
    ('firmware-scan-internal', ['firmware', 'scan'], {'files': {
        **INTERNAL, '/media/internal/music/.keep': ''}}),
    ('firmware-scan-internal-unmounted', ['firmware', 'scan'], {'files': INTERNAL}),
    ('firmware-scan-rejected-copy-gone', ['firmware', 'scan'], {'files': {
        '/run/y2/radio-firmware.json': '{"schema": 1, "state": "Failed", "reason": "x", "seen": [1, 2]}'}}),
    ('firmware-scan-new-copy', ['firmware', 'scan'], {'files': mtp_root('ext4')}),
    # The copy is written when the case runs: its time is a clock reading.
    ('firmware-scan-arriving', ['firmware', 'scan'], {'volatile': ['seen'], 'files': {
        '/data/firmware/inbox/system.img': 'still copying'}}),
    ('firmware-scan-rejected-before', ['firmware', 'scan'], {'files': mtp_root('ext4', 'Failed')}),
    ('firmware-install-dir', ['firmware', 'install', str(PACKAGE)], {}),
    ('firmware-install-image', ['firmware', 'install', str(PACKAGE / 'manifest.json')], {}),
    ('firmware-install-missing', ['firmware', 'install', str(PACKAGE / 'system.img')], {}),
    ('firmware-install-no-path', ['firmware', 'install'], {}),
    ('firmware-help', ['firmware', '--help'], {}),
    ('firmware-bad-action', ['firmware', 'flash'], {}),
] + [('firmware-scan-' + name, ['firmware', 'scan'], {'files': mtp_root(name, 'Missing')})
     for name in ('ext4', 'ext3', 'partial', 'large', 'text', 'badsparse')]


# ---- service-daemon (commands/service/daemon.rs, usb.rs) --------------------------------------------------
# duration_ns: how long the (fixture) supplicant query took.
DAEMON = {'volatile': ['attempt', 'deadline_monotonic_s', 'duration_ns']}
CASES += [
    ('service-daemon-one-tick', ['service-daemon'], {**DAEMON, 'env': {'Y2_DAEMON_TICKS': '1'}}),
    ('service-daemon-three-ticks', ['service-daemon'], {**DAEMON, 'env': {'Y2_DAEMON_TICKS': '3'}}),
    ('service-daemon-dark-settled', ['service-daemon'], {**DAEMON, 'env': {'Y2_DAEMON_TICKS': '2'}, 'files': {
        '/sys/class/backlight/panel/bl_power': '4\n', '/sys/class/backlight/panel/brightness': '200\n',
        '/run/y2/media-lifecycle.json': '{"inventory": []}',
        '/data/updates/last-check.json': last_check(),
        '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true}'},
        'commands': [answer(['/usr/sbin/y2-platform', 'update', 'health-ack'], '{"state": "Acknowledged"}')]}),
    ('service-daemon-online', ['service-daemon'], {**DAEMON, 'env': {'Y2_DAEMON_TICKS': '1'}, 'files': {
        **ONLINE, '/run/y2/media-mount.json': '{"read_only": "checking", "boot_id": "boot-a"}'}}),
]


def ticks(n):
    return {'Y2_DAEMON_TICKS': str(n)}


def wifi_online(status='wpa_state=COMPLETED\nbssid=00:11:22:33:44:55\nid=0\n', dns='2'):
    """The supplicant associated, an address and a default route on wlan0, the DNS probe answering."""
    return [answer(['wpa_cli', '-i', 'wlan0', 'status'], status),
            answer(['ip', '-j', '-4', 'addr', 'show', 'dev', 'wlan0'],
                   '[{"addr_info": [{"local": "192.168.1.40", "scope": "global"}]}]'),
            answer(['ip', '-j', '-4', 'route', 'show', 'default', 'dev', 'wlan0'],
                   '[{"dst": "default", "gateway": "192.168.1.1"}]'),
            answer(['wpa_cli', '-i', 'wlan0', 'signal_poll'], 'RSSI=-52\nLINKSPEED=72\n'),
            answer(['getaddrinfo', 'pool.ntp.org'], dns),
            answer(['/usr/sbin/iw', 'dev', 'wlan0', 'get', 'power_save'], 'Power save: off')]


WLAN = {'/sys/class/net/wlan0/ifindex': '3\n', '/sys/class/net/wlan0/statistics/rx_bytes': '1000\n',
        '/sys/class/net/wlan0/statistics/tx_bytes': '500\n', '/data/network/enabled': '1\n',
        '/run/y2/bt-reconnect.json': '{"state": "Connected"}', '/proc/pressure/cpu': 'some avg10=0.00\n'}
# One USB device controller with the gadget's configfs mounted (the gadget directory itself is the
# service's to make).
UDC = {'/sys/class/udc/musb-hdrc.0/state': 'not attached\n', '/sys/kernel/config/usb_gadget/.keep': ''}
DEVELOPER = {'/data/system/platform/usb-mode.json': '{"mode": "developer"}'}
FUNCTIONFS = answer(['mount', '-t', 'functionfs', 'mtp', '/dev/ffs-mtp'])
# Internal Storage mounted and recorded Ready: the MTP view of its music folder, the Firmware folder there.
INTERNAL_DEVICE = '../../devices/platform/11230000.mmc/mmcblk0p8'
INTERNAL_MEDIA = {
    **VOLUME, '/proc/self/mountinfo': MOUNTINFO + '40 30 %s / /media/internal rw - ext4 /dev/mmcblk0p8 rw\n'
                                                  % INTERNAL_DEVICE,
    '/sys/devices/platform/11230000.mmc/mmcblk0p8/dev': '179:8\n',
    '/sys/devices/platform/11230000.mmc/mmcblk0p8/start': '3742720\n',
    '/sys/devices/platform/11230000.mmc/mmcblk0p8/size': '11460608\n', '/media/internal/music/.keep': '',
    '/run/y2/internal-media.json': json.dumps({'state': 'Ready', 'boot_id': 'boot-a', 'mount_id': 40,
                                               'device_id': INTERNAL_DEVICE})}
INTERNAL_BLKID = answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p8'],
                        '79324c69-6e75-4801-8000-000000000103')


def firmware_present():
    """Every reviewed radio firmware file with its size (present() looks at sizes only)."""
    sizes = {'WIFI_RAM_CODE_MT6582': 160368, 'WMT_SOC.cfg': 80, 'mt6572_82_patch_e1_0_hdr.bin': 33120,
             'mt6572_82_patch_e1_1_hdr.bin': 17476, 'modem_1_2g_n.img': 2424376, 'WIFI.defaults': 512,
             'BT.defaults': 64}
    return {'/data/firmware/mt6582/' + name: 'x' * size for name, size in sizes.items()}


REPORTS_ON = {'/usr/share/y2linux/report-upload.json': json.dumps({'upload_key': 'K' * 40}),
              '/data/system/platform/reports.json': json.dumps({'schema': 1, 'enabled': True}),
              '/data/system/platform/reports/20261020T100000Z-x.tar.gz': 'queued'}

CASES += [
    # USB: the gadget for MTP, its responder refused (no umtprd below a fixture root) until the budget ends.
    ('service-daemon-usb-no-configfs', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/sys/class/udc/musb-hdrc.0/state': 'not attached\n'}}),
    ('service-daemon-usb-mtp-budget', ['service-daemon'], {**DAEMON, 'env': ticks(6), 'files': UDC,
                                                           'commands': [FUNCTIONFS]}),
    ('service-daemon-usb-functionfs-refused', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': UDC}),
    ('service-daemon-usb-internal-storage', ['service-daemon'], {
        **DAEMON, 'env': ticks(2), 'commands': [FUNCTIONFS, BLKID, INTERNAL_BLKID],
        'files': {**UDC, **INTERNAL_MEDIA, '/sys/class/udc/musb-hdrc.0/state': 'configured\n'}}),
    ('service-daemon-usb-old-gadget', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'commands': [FUNCTIONFS],
                                                          'files': {
        **UDC, '/sys/kernel/config/usb_gadget/y2/UDC': 'musb-hdrc.0\n',
        '/sys/kernel/config/usb_gadget/y2/configs/c.1/old.usb0/.keep': ''}}),
    ('service-daemon-usb-developer', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        **UDC, **DEVELOPER, '/sys/class/udc/musb-hdrc.0/state': 'configured\n'}}),
    ('service-daemon-usb-developer-unplugged', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        **UDC, **DEVELOPER}}),
    ('service-daemon-usb-developer-suspended', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **UDC, **DEVELOPER, '/sys/class/udc/musb-hdrc.0/state': 'suspended\n'}}),
    ('service-daemon-usb-unsupported-mode', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        **UDC, '/data/system/platform/usb-mode.json': '{"mode": "mass-storage"}'}}),
    ('service-daemon-usb-null-mode', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **UDC, '/data/system/platform/usb-mode.json': '{"mode": null}'}}),
    ('service-daemon-usb-two-controllers', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **UDC, '/sys/class/udc/other.1/state': 'not attached\n'}}),
    # Wi-Fi: associating (the 1 s cadence), a failed status query counted, online with the DNS probe, the
    # power policy, the time service, the daily update check and the report run.
    ('service-daemon-wifi-associating', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': WLAN, 'commands': [
        answer(['wpa_cli', '-i', 'wlan0', 'status'], 'wpa_state=ASSOCIATING\n')]}),
    ('service-daemon-wifi-query-failures', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        **WLAN, '/run/y2/network.json': '{"boot_id": "boot-a", "status_query_failures": 4}'}}),
    ('service-daemon-wifi-query-failures-other-boot', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, '/run/y2/network.json': '{"boot_id": "boot-z", "status_query_failures": 4}'}}),
    ('service-daemon-wifi-query-failures-odd', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, '/run/y2/network.json': '{"boot_id": "boot-a", "status_query_failures": -3}'}}),
    ('service-daemon-wifi-online', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {**WLAN, **REPORTS_ON},
                                                        'commands': wifi_online()}),
    ('service-daemon-wifi-dns-fails', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': WLAN,
                                                           'commands': wifi_online(dns='0')}),
    ('service-daemon-wifi-bad-probe-host', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, '/etc/y2linux/network-policy.json': '{"dns_probe_host": "not a host"}'}, 'commands': wifi_online()}),
    ('service-daemon-wifi-online-checked', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, **REPORTS_ON, '/data/updates/last-check.json': last_check(),
        '/data/system/platform/reports.json': json.dumps({'schema': 1, 'enabled': True, 'uploads': {
            '20261020T100000Z-x.tar.gz': {'next': 4102444800}}})}, 'commands': wifi_online()}),
    ('service-daemon-wifi-online-failed-check', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, '/data/updates/last-check.json': last_check('failed', age=7200)}, 'commands': wifi_online()}),
    ('service-daemon-wifi-reports-off', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        **WLAN, **REPORTS_ON, '/data/system/platform/reports.json': '{"enabled": false}'},
        'commands': wifi_online()}),
    # Maintenance: the time floor, application_ready, firmware already there, a lit display.
    ('service-daemon-time-floor', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/run/y2/time.json': anchor(), '/data/system/platform/time-last.json': None}}),
    ('service-daemon-time-floor-recent', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/run/y2/time.json': anchor(), '/data/system/platform/time-last.json': lambda: json.dumps(
            {'epoch': time.time() - 60, 'source': 'ntp'})}}),
    ('service-daemon-application-ready', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true}',
        '/data/system/platform/boot.json': json.dumps({'schema': 1, 'boot_id': 'boot-a',
                                                       'last_stage': 'services_started'})}}),
    ('service-daemon-firmware-present', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        **firmware_present(), '/sys/class/backlight/panel/bl_power': '0\n',
        '/sys/class/backlight/panel/brightness': '120\n'}}),
    ('service-daemon-firmware-copy-waiting', ['service-daemon'], {**DAEMON, 'env': ticks(2), 'files': {
        '/data/firmware/inbox/system.img': 'copying'}}),
    ('service-daemon-reconcile-retry', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/run/y2/media-lifecycle.json': '{"inventory": [], "retry": true}'}}),
    ('service-daemon-verify-other-boot', ['service-daemon'], {**DAEMON, 'env': ticks(1), 'files': {
        '/run/y2/media-mount.json': '{"read_only": "checking", "boot_id": "boot-z"}'}}),
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path)."""
    firmware_images(root)
