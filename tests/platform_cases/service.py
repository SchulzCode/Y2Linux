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
    ('update-status-bare-option', ['update', 'status', '--url'], {'ported': False}),
    ('update-option-first', ['update', '--package', '/x', 'status'], {'ported': False}),
    ('update-unknown-action', ['update', 'install'], {'ported': False}),
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
    ('update-health-ack-checks', ['update', 'health-ack'], {'volatile': ['deadline_monotonic_s'], 'ported': False, 'files': {
        **PENDING, **READY, '/proc/1234/stat': stat_line(777)}}),
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


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path)."""
