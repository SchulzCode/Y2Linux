"""Golden cases of port group `reports` (see tests/platform_cases/__init__.py): report (every action),
reset (plan, execute, resume, purge), export-state, export-diagnostics.

The report upload goes through `curl` (a fixture command below a fixture root: `http` is its answer, as
upload.rs `answer` reads it).

EXTRA_CASES (tests/test_platform_reports.py runs them): a report or export that stays on disk (a random report
ID and wall-clock time in its name, a random scrubbing key in its tokens, deflate bytes), and a reset executed on
the plan made in the same root (a target's identity is its inode). The test normalises them: archives are
decompressed and recorded by member, tokens and IDs replaced.
"""
import gzip
import hashlib
import io
import json
import os
import tarfile
import time

from datetime import datetime, timezone

UUID = (r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', '<uuid>')
HEX32 = (r'\b[0-9a-f]{32}\b', '<hex32>')
HEX64 = (r'\b[0-9a-f]{64}\b', '<hex64>')
STAMP = (r'\d{8}T\d{6}Z', '<stamp>')
LOCAL = (r'y2linux-report-\d{8}-\d{6}', 'y2linux-report-<stamp>')
STATE = '/data/system/platform/reports.json'
QUEUE = '/data/system/platform/reports'
INSTALL = '6f1c3b2a-9d4e-4f5a-8b7c-0d1e2f3a4b5c'
KEY = {'/usr/share/y2linux/report-upload.json': json.dumps({'upload_key': 'K' * 40})}
# The three reports fixture() queues (names sort A, B; C was made four days ago).
A = '20261020T100000Z-0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a.tar.gz'
B = '20261021T100000Z-0b0b0b0b-0b0b-4b0b-8b0b-0b0b0b0b0b0b.tar.gz'
C = '20261019T100000Z-0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c.tar.gz'
BAD = QUEUE + '/20261018T100000Z-bad.tar.gz'
ENDPOINT = 'https://reports.schulzcode.dev/v1/report'
VOLATILE = ['created_utc', 'sent_utc', 'bytes', 'next', 'next_attempt']
DAY = datetime.now(timezone.utc).date().isoformat()


def curl(headers):
    """upload.rs `argv`: the request (headers as urllib sends them, an empty one as curl's `Name;`), the body on
    stdin."""
    argv = ['curl', '-q', '--silent', '--show-error', '--proto', '=https', '--noproxy', '*', '--connect-timeout',
            '30', '--speed-limit', '1', '--speed-time', '30', '--request', 'POST']
    for name, value in headers:
        argv += ['--header', '%s: %s' % (name, value) if value else name + ';']
    for header in ('Accept-Encoding: identity', 'Connection: close', 'Accept:', 'Expect:', 'Transfer-Encoding:'):
        argv += ['--header', header]
    return argv + ['--upload-file', '-', '--write-out', '\\n%{http_code} %{exitcode} %{errormsg}', ENDPOINT]


def answer(argv, output='', ok=True):
    return {'argv': argv, 'ok': ok, 'reason': None if ok else 'command_failed', 'output': output}


def archive(report_id, created, members=()):
    """A queued report as report._pack writes it (report.json first)."""
    index = {'schema': 1, 'report_id': report_id, 'install_id': INSTALL, 'trigger': 'manual', 'triggers': ['manual'],
             'created_utc': created, 'release': {'release_version': '1.0.0-fixture'}}
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w') as tar:
        for name, data in [('report.json', json.dumps(index, indent=1, sort_keys=True).encode()), *members]:
            info = tarfile.TarInfo(name)
            info.size, info.mtime, info.mode = len(data), 1761000000, 0o600
            tar.addfile(info, io.BytesIO(data))
    return gzip.compress(raw.getvalue(), compresslevel=6, mtime=0)


# The three reports fixture() queues: A and B made "in the future" (never too old to send), C long ago.
ARCHIVES = {name: archive(name[17:53], created, [('files/x.log', b'x\n')]) for name, created in (
    (A, '2099-01-01T00:00:00+00:00'), (B, '2099-01-02T00:00:00.500000+00:00'), (C, '2000-01-01T00:00:00+00:00'))}


def request(name, key='K' * 40):
    """The curl command upload.rs and `_post` make for one queued report of fixture()."""
    data = ARCHIVES[name]
    return curl([('Content-Type', 'application/gzip'), ('User-Agent', 'Y2Linux/1.0.0-fixture'),
                 ('X-Y2-Upload-Key', key), ('X-Y2-Schema', '1'), ('X-Y2-Install-Id', INSTALL),
                 ('X-Y2-Report-Id', name[17:53]), ('X-Y2-Sha256', hashlib.sha256(data).hexdigest()),
                 ('X-Y2-Trigger', 'manual'), ('X-Y2-Release', '1.0.0-fixture'), ('Content-Length', str(len(data)))])


def http(code, body='{}', exit_code=0, message='', name=A):
    """curl's output for one request: the receiver's answer, then the write-out line."""
    return answer(request(name), '%s\n%03d %d %s' % (body, code, exit_code, message), ok=exit_code == 0)


def anchor(source='ntp'):
    """/run/y2/time.json of this boot as ntp-event writes it, made when the case runs."""
    return lambda: json.dumps({'schema': 1, 'boot_id': 'boot-a', 'epoch': time.time(), 'monotonic_s': time.monotonic(),
                               'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME), 'source': source})


def state(**fields):
    value = {'schema': 1, 'enabled': True, 'install_id': INSTALL, 'daily': {}, 'boot_triggers': {}, 'seen': {}}
    value.update(fields)
    return json.dumps(value)


ENABLED = {STATE: state()}
# Everything an upload waits for: a key, opted in, Wi-Fi online this boot, a network clock, a charger.
READY = {**KEY, **ENABLED, '/run/y2/network.json': json.dumps({'state': 'Online', 'boot_id': 'boot-a'}),
         '/run/y2/time.json': anchor(), '/run/y2/power.json': json.dumps({'boot_id': 'boot-a',
                                                                          'battery': {'charging': True}})}
UPLOAD = {'volatile': VOLATILE, 'patterns': [UUID, STAMP]}
SAVED = {**UPLOAD, 'archives': True, 'patterns': [UUID, STAMP, LOCAL]}

# A verified /data and Y2Media (devices 259:5 and 259:6, whose sysfs links fixture() makes), an SD card (259:7).
MOUNTS = ('22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
          '30 22 259:5 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n')
INTERNAL = '31 22 259:6 / /media/internal rw,noatime - ext4 /dev/mmcblk0p8 rw\n'
SD = '32 22 259:7 / /media/sd ro,noatime - vfat /dev/mmcblk1p1 ro\n'
DATA = {'/proc/self/mountinfo': MOUNTS, '/data/updates/.keep': ''}
DATA_UUID = '79324c69-6e75-4801-8000-000000000102'
BLKID = [answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], DATA_UUID)]
GENERATION = 'boot-a:30:259:5'
SESSION = json.dumps({'schema': 3, 'model': {'settings': {'volume': 7, 'time_zone': 'Europe/Berlin'},
                                             'queue': [{'title': 'Ready', 'artist': 'Some Artist'}],
                                             'queue_position': 0, 'playback': 'Paused'}})
SETTINGS = {'/data/reborn/state/session.json': SESSION}
DEFAULTS = answer(['/usr/bin/reborn', '--default-settings'],
                  json.dumps({'session_schema': 3, 'settings': {'volume': 5}}))
PLAN_ID = '1234567890abcdef1234567890abcdef'
RESET = {'volatile': ['expires_monotonic_s', 'inode', 'mtime_ns'], 'patterns': [HEX32, HEX64]}
SESSION_PATH = '/data/reborn/state/session.json'
TAINTED = {**ENABLED, '/proc/sys/kernel/tainted': '1\n'}


def plan(**changes):
    """reset-plan.json as Python's plan() writes it, with its confirmation (compact sorted json.dumps)."""
    value = {'schema': 1, 'id': PLAN_ID, 'state': 'Planned', 'scope': 'caches', 'boot_id': 'boot-a',
             'data_generation': GENERATION, 'data_uuid': DATA_UUID, 'expires_monotonic_s': 1e12, 'targets': [],
             'stop_services': ['S90reborn', 'S42y2-readiness'], 'restart': 'owner reboot after completion',
             'preserved': ['SSH identity', 'entropy']}
    tamper = changes.pop('tamper', None)
    value.update(changes)
    value['confirmation'] = hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    if tamper:
        value.update(tamper)
    return value


def plan_without(*keys, **changes):
    """A plan without `keys`, its confirmation over what is left (a plan Python's execute reads further)."""
    value = {k: v for k, v in plan(**changes).items() if k not in keys and k != 'confirmation'}
    value['confirmation'] = hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    return value


def plan_files(value):
    return {'/data/system/platform/reset-plan.json': json.dumps(value)}


CACHES = plan()
SETTINGS_PLAN = plan(scope='settings', targets=[{'path': 'reborn/state/session.json', 'identity': {
    'device': 1, 'inode': 2, 'mode': 32768, 'size': len(SESSION), 'mtime_ns': 3}}])
QUARANTINE = '/data/reset-quarantine/' + PLAN_ID
RECORD = {'schema': 1, 'id': PLAN_ID, 'scope': 'caches', 'state': 'Complete', 'completed': [],
          'preserved': ['SSH identity'], 'quarantine': QUARANTINE, 'purge_confirmation': 'c' * 64}
PURGE = {**DATA, QUARANTINE + '/record.json': json.dumps(RECORD), QUARANTINE + '/000/a/b.txt': 'b',
         QUARANTINE + '/000/c.txt': 'c', QUARANTINE + '/001': 'one', QUARANTINE + '/notes.txt': 'kept'}

DMESG = ['dmesg']
HEALTH = ['rebornctl', 'health', '--json']
# Every automatic trigger at once.
FIRED = {**ENABLED, '/proc/sys/kernel/tainted': '512\n',
         '/data/system/platform/previous-boot-evidence.json': json.dumps({
             'previous_orderly_shutdown': False, 'previous_boot_id': 'boot-z', 'reset_cause': 'watchdog',
             'reset_cause_reason': 'wdt', 'previous_last_stage': 'application_ready',
             'loader': {'reset': 'warm', 'boot_reason': 3}, 'pmic': {}}),
         '/data/reborn/logs/panic-last.json': json.dumps({'version': '1.0.0-fixture', 'location': 'a.rs:1',
                                                          'message': 'boom', 'isolated': True}),
         '/data/updates/state.json': json.dumps({'schema': 2, 'state': 'RolledBack', 'sequence': 11000}),
         '/sys/power/suspend_stats/fail': '3\n', '/sys/power/suspend_stats/failed_resume': '1\n',
         '/sys/power/suspend_stats/last_failed_errno': '-5\n', '/sys/power/suspend_stats/last_failed_dev': 'msdc\n',
         '/sys/devices/platform/18070000.connectivity/status': 'state=on error=-110\n',
         '/sys/fs/ext4/mmcblk0p9/errors_count': '2\n',
         '/sys/fs/pstore/console-ramoops-0': 'x\n<0>[  12.345678] Kernel panic - not syncing: test\n'}
FIRED_COMMANDS = [answer(HEALTH, json.dumps({'subsystems': {'audio': {'state': 'failed', 'required': True},
                                                           'radio': {'state': 'failed', 'required': False}}})),
                  answer(DMESG, '[ 1.0] flip_done timed out\n[ 2.0] EXT4-fs error (device mmcblk0p9): x')]

CASES = [
    # report status / enable / disable / new-id
    ('report-status', ['report', 'status'], {}),
    ('report-status-everything', ['report', 'status'], {'files': {
        **KEY, STATE: state(last={'report_id': 'r', 'state': 'queued'}),
        '/proc/self/mountinfo': MOUNTS + INTERNAL + SD, '/media/internal/music/.keep': '',
        '/media/sd/.keep': '', '/run/y2/media-mount.json': json.dumps(
            {'boot_id': 'boot-a', 'mount_id': 32, 'read_only': 'protected'})}}),
    ('report-status-sd-unprotected', ['report', 'status'], {'files': {
        '/proc/self/mountinfo': MOUNTS + SD, '/media/sd/.keep': '',
        '/run/y2/media-mount.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 32, 'read_only': 'filesystem_errors'})}}),
    ('report-status-bad-key', ['report', 'status'], {'files': {
        '/usr/share/y2linux/report-upload.json': '{"upload_key": "short"}', STATE: '[1, 2]'}}),
    ('report-enable', ['report', 'enable'], {'patterns': [UUID]}),
    ('report-enable-again', ['report', 'enable'], {'files': ENABLED}),
    ('report-enable-marks-seen', ['report', 'enable'], {'patterns': [UUID], 'commands': FIRED_COMMANDS,
                                                       'files': {**FIRED, STATE: state(enabled=False)}}),
    ('report-disable', ['report', 'disable'], {'files': ENABLED}),
    ('report-new-id', ['report', 'new-id'], {'patterns': [UUID], 'files': ENABLED}),
    ('report-new-id-fresh', ['report', 'new-id', '--to', 'sd'], {'patterns': [UUID]}),
    # report check / work
    ('report-check-disabled', ['report', 'check'], {}),
    ('report-check-nothing', ['report', 'check', '--topic', 'wifi'], {'files': ENABLED}),
    ('report-check-daily-limit', ['report', 'check'], {'commands': FIRED_COMMANDS, 'files': {
        **FIRED, STATE: state(daily={DAY: 5, '2020-01-01': 2})}}),
    ('report-check-reported', ['report', 'check'], {'files': {
        **ENABLED, '/proc/sys/kernel/tainted': '1\n', STATE: state(
            daily={DAY: 1}, boot_triggers={'boot-a': ['kernel_warning'], 'boot-old': ['manual']})}}),
    ('report-check-seen', ['report', 'check'], {'commands': FIRED_COMMANDS, 'files': {
        **FIRED, STATE: state(seen={'pstore_crash': '|12.345678|', 'tainted': 'boot-a:512', 'unclean': 'boot-z',
                                    'reborn_failed': 'boot-a:audio', 'update': '11000:RolledBack',
                                    'suspend_fail': 'boot-a:3:1:0', 'conn_error': 'boot-a:-110', 'dark': 'boot-a',
                                    'storage': 'boot-a:True:2', 'reborn_panic': hashlib.sha256(json.dumps(
                                        ['1.0.0-fixture', 'a.rs:1', 'boom']).encode()).hexdigest()[:16]})}}),
    ('report-check-old-crash-key', ['report', 'check'], {'files': {
        **ENABLED, '/sys/fs/pstore/console-ramoops-0': 'Kernel panic\n', STATE: state(seen={'pstore': 'x'}),
        '/etc/y2linux/release.json': '{"reborn_version": "1.0"}',
        '/data/reborn/logs/panic-last.json': json.dumps({'version': '0.9', 'location': 'x', 'message': 'y'})}}),
    ('report-check-routine-sleep', ['report', 'check'], {'files': {
        **ENABLED, '/sys/power/suspend_stats/fail': '4\n', '/sys/power/suspend_stats/last_failed_errno': '-16\n',
        '/run/y2/application-ready.json': '{"boot_id": "boot-a", "first_frame": true}'},
        'commands': [answer(DMESG, 'DSI register reads still lag')]}),
    # Shapes of reports.json and session.json Python reads as it finds them: an error, a traceback (an
    # AttributeError, TypeError or KeyError: nothing on stdout, exit 1) or nothing at all when the shape
    # is never used, after the same changes (check's seen markers are saved before the queue raises).
    ('report-check-bad-daily', ['report', 'check'], {'files': {STATE: state(daily=[])}}),
    ('report-check-bad-queue', ['report', 'check'], {'files': {
        **ENABLED, SESSION_PATH: '{"model": {"queue": 5}}'}}),
    ('report-check-fired-bad-daily', ['report', 'check'], {'commands': FIRED_COMMANDS, 'files': {
        **FIRED, STATE: state(daily=[])}}),
    ('report-check-fired-reported-text', ['report', 'check'], {'files': {
        **TAINTED, STATE: state(boot_triggers={'boot-a': 'kernel_warning,x'})}}),
    ('report-check-fired-reported-number', ['report', 'check'], {'files': {
        **TAINTED, STATE: state(boot_triggers={'boot-a': 7})}}),
    ('report-check-fired-bad-triggers', ['report', 'check'], {'files': {
        **TAINTED, STATE: state(boot_triggers=[])}}),
    ('report-check-fired-bad-seen', ['report', 'check'], {'commands': FIRED_COMMANDS, 'files': {
        **FIRED, STATE: state(seen=[])}}),
    ('report-check-bad-seen-quiet', ['report', 'check'], {'files': {**ENABLED, STATE: state(seen='x')}}),
    ('report-check-fired-settings-text', ['report', 'check'], {'files': {
        **TAINTED, SESSION_PATH: '{"model": {"settings": "ab"}}'}}),
    ('report-check-fired-settings-number', ['report', 'check'], {'files': {
        **TAINTED, SESSION_PATH: '{"model": {"settings": 3}}'}}),
    ('report-check-health-list', ['report', 'check'], {'files': ENABLED, 'commands': [
        answer(HEALTH, '[1]')]}),
    ('report-enable-bad-seen', ['report', 'enable'], {'patterns': [UUID], 'commands': FIRED_COMMANDS,
                                                    'files': {**FIRED, STATE: state(enabled=False, seen=[])}}),
    ('report-send-bad-daily', ['report', 'send'], {'files': {
        **READY, STATE: state(daily=5)}}),
    ('report-work', ['report', 'work'], {**UPLOAD, 'files': {**READY, '/run/y2/network.json': None}}),
    ('report-work-disabled', ['report', 'work'], {}),
    # report upload: why it waits
    ('report-upload-no-key', ['report', 'upload'], {}),
    ('report-upload-not-enabled', ['report', 'upload'], {'files': KEY}),
    ('report-upload-offline', ['report', 'upload'], {'files': {
        **READY, '/run/y2/network.json': json.dumps({'state': 'Online', 'boot_id': 'boot-old'})}}),
    ('report-upload-no-clock', ['report', 'upload'], {'files': {**READY, '/run/y2/time.json': None}}),
    ('report-upload-updating', ['report', 'upload'], {'files': {
        **READY, '/data/updates/state.json': '{"state": "Queued"}'}}),
    ('report-upload-maintenance', ['report', 'upload'], {'files': {
        **READY, '/data/system/platform/maintenance-pending': 'x'}}),
    ('report-upload-battery-low', ['report', 'upload'], {'files': {
        **READY, '/run/y2/power.json': json.dumps({'boot_id': 'boot-a', 'battery': {'percent': 14.0}})}}),
    ('report-upload-battery-other-boot', ['report', 'upload'], {'files': {
        **READY, '/run/y2/power.json': json.dumps({'boot_id': 'boot-old', 'battery': {'charging': True}})}}),
    # report upload: the receiver's answers
    ('report-upload-sent', ['report', 'upload'], {**UPLOAD, 'files': {
        **READY, STATE: state(last={'report_id': '0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a', 'state': 'failed',
                                    'reason': 'HTTP_500', 'next_attempt': 1.5},
                              uploads={A: {'attempts': 2, 'next': 0, 'error': 'HTTP_500'}, 'gone.tar.gz': {}})},
        'commands': [http(201, '{"report_id": "x", "status": "stored"}')]}),
    ('report-upload-refused', ['report', 'upload'], {**UPLOAD, 'files': {**READY, BAD: 'not a gzip'},
                                                    'commands': [http(413, '{"error": "too_large"}')]}),
    ('report-upload-refused-no-error', ['report', 'upload'], {**UPLOAD, 'files': READY,
                                                             'commands': [http(400, '[1]')]}),
    ('report-upload-server-error', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [http(503, 'oops')]}),
    ('report-upload-full-day', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [http(429)]}),
    ('report-upload-redirect', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [http(302, '')]}),
    ('report-upload-no-answer', ['report', 'upload'], {**UPLOAD, 'files': READY}),
    ('report-upload-no-name', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [
        http(0, '', 6, 'Could not resolve host: reports.schulzcode.dev')]}),
    ('report-upload-refused-connection', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [
        http(0, '', 7, 'Failed to connect to reports.schulzcode.dev port 443: Connection refused')]}),
    ('report-upload-timeout', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [http(0, '', 28, 'Timeout')]}),
    ('report-upload-certificate', ['report', 'upload'], {**UPLOAD, 'files': READY, 'commands': [http(0, '', 60, 'x')]}),
    ('report-upload-bad-attempts', ['report', 'upload'], {**UPLOAD, 'files': {
        **READY, STATE: state(uploads={A: {'attempts': 'x'}})}, 'commands': [http(503, 'oops')]}),
    ('report-upload-bad-battery', ['report', 'upload'], {'files': {
        **READY, '/run/y2/power.json': json.dumps({'boot_id': 'boot-a', 'battery': 'full'})}}),
    ('report-work-bad-battery', ['report', 'work'], {'files': {
        **READY, '/run/y2/power.json': json.dumps({'boot_id': 'boot-a', 'battery': [1]})}}),
    ('report-upload-not-due', ['report', 'upload'], {**UPLOAD, 'files': {
        **READY, STATE: state(uploads={A: {'attempts': 1, 'next': 9e12}, B: {'attempts': 5, 'next': 9e12}})},
        'commands': [http(201)]}),
    # report send / save
    ('report-send-not-enabled', ['report', 'send', '--topic', 'sound'], {}),
    ('report-save-needs-to', ['report', 'save'], {}),
    ('report-save-not-mounted', ['report', 'save', '--to', 'internal', '--topic', 'battery'], {}),
    ('report-save-sd-not-mounted', ['report', 'save', '--to', 'sd'], {'files': {
        '/proc/self/mountinfo': MOUNTS + SD}}),
    # reset plan
    ('reset-plan-no-scope', ['reset', 'plan'], {}),
    ('reset-plan-unverified', ['reset', 'plan', '--scope', 'settings'], {}),
    ('reset-plan-settings', ['reset', 'plan', '--scope', 'settings'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, **SETTINGS, '/data/reborn/state/scrobbles.jsonl': '{}\n'}}),
    ('reset-plan-full-user', ['reset', 'plan', '--scope', 'full-user', '--confirm', 'x'], {
        **RESET, 'commands': BLKID, 'files': {**DATA, '/data/bluetooth/bluez/AA/info': 'Name=x\n',
                                              '/data/bluetooth/bluealsa/x': '', '/data/music/a.flac': ''}}),
    ('reset-plan-bonds', ['reset', 'plan', '--scope', 'bluetooth-bonds'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/bluetooth/bluez/AA/info': 'Name=x\n', '/data/bluetooth/preferred-audio': 'AA'}}),
    ('reset-plan-library', ['reset', 'plan', '--scope', 'library'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/reborn/library.db': 'x'}}),
    ('reset-plan-network', ['reset', 'plan', '--scope', 'network'], {**RESET, 'commands': BLKID, 'files': DATA}),
    ('reset-plan-busy', ['reset', 'plan', '--scope', 'network'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/proc/1234/cmdline': '/usr/bin/reborn\0--x\0', '/proc/77/cmdline': 'sh\0/usr/sbin/wpa_supplicant/\0',
        '/proc/80/cmdline': 'other\0'}}),
    ('reset-plan-pending', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/system/platform/maintenance-pending': 'x'}}),
    ('reset-plan-updating', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/updates/state.json': '{"schema": 2, "state": "Staged"}'}}),
    ('reset-plan-bad-journal', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/updates/state.json': '{"schema": 7}'}}),
    # A journal without a state: update.status's KeyError, after the volume check (blkid).
    ('reset-plan-journal-no-state', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/updates/state.json': '{"schema": 1}'}}),
    ('reset-plan-odd-pid', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/proc/12a/cmdline': '/usr/bin/reborn\0'}}),
    ('reset-plan-pid-underscore', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/proc/1_2/cmdline': '/usr/bin/reborn\0'}}),
    ('reset-plan-wrong-uuid', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'files': DATA, 'commands': [
        answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], 'other')]}),
    ('reset-plan-unsafe', ['reset', 'plan', '--scope', 'caches'], {**RESET, 'commands': BLKID, 'files': {
        **DATA, '/data/cache': 'a file is fine'}}),
    # reset execute / resume
    ('reset-execute-no-confirm', ['reset', 'execute'], {'files': plan_files(CACHES)}),
    ('reset-execute-wrong-confirm', ['reset', 'execute', '--confirm', 'f' * 64], {
                                                                               'files': plan_files(CACHES)}),
    ('reset-execute-changed', ['reset', 'execute', '--confirm', CACHES['confirmation']], {
        'files': plan_files(plan(tamper={'scope': 'full-user'}))}),
    ('reset-execute-stale', ['reset', 'execute', '--confirm', plan(boot_id='boot-z')['confirmation']], {
        'files': plan_files(plan(boot_id='boot-z'))}),
    ('reset-execute-full-user', ['reset', 'execute', '--confirm', plan(scope='full-user')['confirmation']], {
        'files': plan_files(plan(scope='full-user'))}),
    ('reset-execute-expired', ['reset', 'execute', '--confirm', plan(expires_monotonic_s=1.5)['confirmation']], {
        'files': plan_files(plan(expires_monotonic_s=1.5))}),
    ('reset-execute-unverified', ['reset', 'execute', '--confirm', CACHES['confirmation']], {
        'files': plan_files(CACHES)}),
    ('reset-execute-generation', ['reset', 'execute', '--confirm', plan(data_generation='x')['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **plan_files(plan(data_generation='x'))}}),
    ('reset-execute-null-id-wrong', ['reset', 'execute', '--confirm', 'f' * 64], {
        'files': plan_files(plan(id=None))}),
    ('reset-execute-no-generation', ['reset', 'execute', '--confirm', plan(data_generation=None)['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **plan_files(plan(data_generation=None))}}),
    ('reset-execute-no-generation-key', ['reset', 'execute', '--confirm', plan_without('data_generation')['confirmation']],
     {'commands': BLKID, 'files': {**DATA, **plan_files(plan_without('data_generation'))}}),
    ('reset-execute-text-targets', ['reset', 'execute', '--confirm', plan(targets='ab')['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **plan_files(plan(targets='ab'))}}),
    ('reset-execute-number-path', ['reset', 'execute', '--confirm', plan(targets=[{'path': 5, 'identity': None}])[
        'confirmation']], {'commands': BLKID, 'files': {
            **DATA, **plan_files(plan(targets=[{'path': 5, 'identity': None}]))}}),
    ('reset-execute-no-identity', ['reset', 'execute', '--confirm', plan(targets=[{'path': 'cache'}])['confirmation']],
     {'commands': BLKID, 'files': {**DATA, **plan_files(plan(targets=[{'path': 'cache'}]))}}),
    # A resume reads the targets only after the pending flag and the Applying record are written.
    ('reset-resume-no-targets', ['reset', 'resume', '--confirm', plan(targets=None)['confirmation']], {
        **RESET, 'commands': BLKID, 'files': {**DATA, **plan_files(plan(targets=None))}}),
    ('reset-resume-number-targets', ['reset', 'resume', '--confirm', plan(targets=3)['confirmation']], {
        **RESET, 'commands': BLKID, 'files': {**DATA, **plan_files(plan(targets=3))}}),
    # Without `preserved` the targets are already moved when the KeyError comes.
    ('reset-execute-no-preserved', ['reset', 'execute', '--confirm', plan_without('preserved')['confirmation']], {
        **RESET, 'commands': BLKID, 'files': {**DATA, **plan_files(plan_without('preserved'))}}),
    ('reset-execute-caches', ['reset', 'execute', '--confirm', CACHES['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **plan_files(CACHES)}}),
    ('reset-execute-target-changed', ['reset', 'execute', '--confirm', SETTINGS_PLAN['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **SETTINGS, **plan_files(SETTINGS_PLAN)}}),
    ('reset-resume-settings', ['reset', 'resume', '--confirm', SETTINGS_PLAN['confirmation']], {
        'commands': BLKID, 'files': {
            **DATA, **SETTINGS, **plan_files(SETTINGS_PLAN), '/data/reset-quarantine/' + PLAN_ID + '/settings-result.json':
            json.dumps({'sha256': hashlib.sha256(SESSION.encode()).hexdigest()})}}),
    ('reset-resume-settings-changed', ['reset', 'resume', '--confirm', SETTINGS_PLAN['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **SETTINGS, **plan_files(SETTINGS_PLAN)}}),
    ('reset-resume-other-volume', ['reset', 'resume', '--confirm', plan(data_uuid=None)['confirmation']], {
        'commands': BLKID, 'files': {**DATA, **plan_files(plan(data_uuid=None))}}),
    ('reset-resume-finished', ['reset', 'resume', '--confirm', CACHES['confirmation']], {
        'commands': BLKID, 'files': {
            **DATA, **plan_files(CACHES), '/data/system/platform/maintenance-pending': 'x',
            '/data/system/platform/maintenance-state.json': json.dumps({'id': PLAN_ID, 'state': 'Complete', 'x': 1})}}),
    # reset purge
    ('reset-purge-no-id', ['reset', 'purge'], {}),
    ('reset-purge-unverified', ['reset', 'purge', '--id', PLAN_ID], {}),
    ('reset-purge-no-quarantine', ['reset', 'purge', '--id', PLAN_ID, '--confirm', 'x'], {
        'commands': BLKID, 'files': DATA}),
    ('reset-purge-wrong-confirm', ['reset', 'purge', '--id', PLAN_ID, '--confirm', 'x'], {
        'commands': BLKID, 'files': PURGE}),
    ('reset-purge', ['reset', 'purge', '--id', PLAN_ID, '--confirm', 'c' * 64], {
        'commands': BLKID, 'files': PURGE}),
    # json.loads's JSONDecodeError text as the failure; a record that is no dict: a traceback.
    ('reset-purge-bad-record', ['reset', 'purge', '--id', PLAN_ID, '--confirm', 'x'], {
        'commands': BLKID, 'files': {**PURGE, QUARANTINE + '/record.json': '{'}}),
    ('reset-purge-list-record', ['reset', 'purge', '--id', PLAN_ID, '--confirm', 'x'], {
        'commands': BLKID, 'files': {**PURGE, QUARANTINE + '/record.json': '[1]'}}),
    # export-state / export-diagnostics
    ('export-state-unverified', ['export-state', '--include-database'], {}),
    ('export-diagnostics', ['export-diagnostics'], {}),
    ('export-diagnostics-limit', ['export-diagnostics'], {'commands': BLKID, 'files': {
        **DATA, **{'/data/exports/y2-diagnostics-%d-x.tar.gz' % i: '' for i in range(8)}}}),
    ('export-diagnostics-not-a-directory', ['export-diagnostics'], {'commands': BLKID, 'files': {
        **DATA, '/data/exports': 'a file'}}),
]

# Cases tests/test_platform_reports.py runs with archive normalisation (`archives`) or a plan made first
# (`plan_first`: the scope) or a library database (`library`) or retained archives and a large log (`retained`) made first.
EXTRA_CASES = [
    # An expiry that is no number: Python's TypeError, a traceback (exit 1, nothing on stdout).
    ('reset-execute-plan-no-expiry', ['reset', 'execute', '--confirm', plan(expires_monotonic_s='x')['confirmation']],
     {'files': plan_files(plan(expires_monotonic_s='x'))}),
    ('reset-execute-plan-missing-expiry', ['reset', 'execute', '--confirm', plan_without('expires_monotonic_s')[
        'confirmation']], {'files': plan_files(plan_without('expires_monotonic_s'))}),
    ('reset-execute-number-id', ['reset', 'execute', '--confirm', plan(id=5)['confirmation']],
     {'files': plan_files(plan(id=5))}),
    ('report-check-fired-odd-count', ['report', 'check'], {'commands': FIRED_COMMANDS, 'files': {
        **FIRED, STATE: state(daily={DAY: 'x'})}}),
    ('report-check-seen-late', ['report', 'check'], {'files': {
        **ENABLED, '/sys/fs/ext4/mmcblk0p9/errors_count': '2\n', STATE: state(seen=None)}}),
    ('report-check-fired-queue-dict', ['report', 'check'], {'files': {
        **TAINTED, SESSION_PATH: '{"model": {"queue": {"a": 1}}}'}}),
    ('report-upload-bad-next', ['report', 'upload'], {**UPLOAD, 'files': {
        **READY, STATE: state(uploads={A: {'next': 'soon'}})}}),
    ('report-upload-bad-last', ['report', 'upload'], {**UPLOAD, 'files': {**READY, STATE: state(last='x')},
                                                     'commands': [http(201, '{}')]}),
    # A manual report is queued before its count fails (TypeError): it stays queued, reports.json unchanged.
    ('report-send-odd-count', ['report', 'send'], {**UPLOAD, 'archives': True, 'files': {
        **READY, '/run/y2/network.json': None, STATE: state(daily={DAY: 'x'})}}),
    # dict() of a list of pairs: the settings as Python reads them.
    ('report-check-settings-pairs', ['report', 'check'], {**UPLOAD, 'archives': True, 'files': {
        **TAINTED, SESSION_PATH: '{"model": {"settings": [["volume", 3], "tz"], "queue": "ab"}}'}}),
    # The saved session as json.loads reads it: its JSONDecodeError, or a traceback for a list with 'schema'.
    ('reset-execute-settings-bad-json', ['reset', 'execute'], {**RESET, 'plan_first': 'settings', 'commands': BLKID,
                                                               'files': {**DATA, SESSION_PATH: '{"a": '}}),
    ('reset-execute-settings-list', ['reset', 'execute'], {**RESET, 'plan_first': 'settings', 'commands': BLKID,
                                                           'files': {**DATA, SESSION_PATH: '["schema"]'}}),
    ('reset-execute-defaults-list', ['reset', 'execute'], {**RESET, 'plan_first': 'settings', 'commands': BLKID + [
        answer(['/usr/bin/reborn', '--default-settings'], '[1]')], 'files': {**DATA, **SETTINGS}}),
    ('report-send', ['report', 'send', '--topic', 'wifi'], {**UPLOAD, 'archives': True, 'files': {
        **READY, '/data/system/platform/reports/' + A: None, '/data/system/platform/reports/' + B: None,
        '/data/system/platform/reports/' + C: None, '/data/network/wpa_supplicant.conf': 'network={\n ssid="Home"\n}\n',
        '/data/logs/system.log': 'joined Home at 192.168.1.40 from aa:bb:cc:dd:ee:ff\n'},
        'commands': [http(201, '{"report_id": "x"}')]}),
    ('report-send-queued', ['report', 'send', '--topic', 'crash'], {**UPLOAD, 'archives': True, 'library': True,
                                                                  'files': {
        **READY, '/run/y2/network.json': None, '/data/network/wpa_supplicant.conf': 'network={\n ssid="Home"\n'
        ' psk="secret passphrase"\n}\n', '/data/logs/system.log': 'joined Home with secret passphrase, '
        'me@example.com 192.168.1.40 aa:bb:cc:dd:ee:ff /media/sd/Music/Some Artist/song.flac\n',
        **SETTINGS, '/run/y2/wifi-events.json': '{"ssid": "Cafe Wifi"}',
        '/data/reborn/logs/events.jsonl': '{"title": "Ready", "n": 1}\n{"ssid": ["Home"]}\nnot json Some Artist\n'}}),
    ('report-send-retained', ['report', 'send'], {**UPLOAD, 'archives': True, 'retained': True, 'files': {
        **READY, '/run/y2/network.json': None, '/data/network/wpa_supplicant.conf': 'network={\n ssid=486f6d65\n}\n'}}),
    ('report-check-queues', ['report', 'check'], {**UPLOAD, 'archives': True, 'commands': FIRED_COMMANDS,
                                                  'files': FIRED}),
    ('report-save-internal', ['report', 'save', '--to', 'internal'], {**SAVED, 'files': {
        **ENABLED, '/proc/self/mountinfo': MOUNTS + INTERNAL, '/media/internal/music/.keep': ''}}),
    ('report-save-sd', ['report', 'save', '--to', 'sd', '--topic', 'other'], {**SAVED, 'files': {
        '/proc/self/mountinfo': MOUNTS + SD, '/media/sd/.keep': '', '/run/y2/media-mount.json': json.dumps(
            {'boot_id': 'boot-a', 'mount_id': 32, 'read_only': 'protected'})},
        'commands': [answer(['mount', '-o', 'remount,rw', '/media/sd'])]}),
    ('export-state', ['export-state', '--include-database', '--include-network'], {
        'archives': True, 'library': True, 'commands': BLKID, 'volatile': ['wall_timestamp', 'monotonic_ns', 'bytes'],
        'patterns': [HEX32, HEX64], 'files': {**DATA, **SETTINGS, '/data/network/wpa_supplicant.conf': 'network={}\n'}}),
    # The tester export: the snapshot and health projected, log tails as event classes, dmesg unanswered.
    ('export-diagnostics-complete', ['export-diagnostics'], {
        'archives': True, 'commands': BLKID, 'volatile': ['bytes'],
        'patterns': [(r'y2-diagnostics-\d+-[0-9a-f]{32}', 'y2-diagnostics-<id>'), HEX64], 'files': {
            **DATA, '/data/logs/system.log': 'msdc0: cmd timeout error=-110\njoined Home at 192.168.1.40\n',
            '/data/reborn/logs/reborn.log': 'bluetooth reconnect failed ret: 5\n'}}),
    ('export-state-plain', ['export-state'], {'archives': True, 'commands': BLKID,
                                              'volatile': ['wall_timestamp', 'monotonic_ns', 'bytes'],
                                              'patterns': [HEX32, HEX64], 'files': DATA}),
    ('reset-execute-settings', ['reset', 'execute'], {**RESET, 'plan_first': 'settings', 'commands': BLKID + [DEFAULTS],
                                                      'files': {**DATA, **SETTINGS, '/data/reborn/state/lastfm.json': '{}'}}),
    ('reset-execute-network', ['reset', 'execute'], {**RESET, 'plan_first': 'network', 'commands': BLKID,
                                                     'files': {**DATA, '/data/network/wpa_supplicant.conf': 'x'}}),
    ('reset-execute-full-user-erase-music', ['reset', 'execute', '--erase-user-music'], {
        **RESET, 'plan_first': 'full-user', 'commands': BLKID, 'files': {
            **DATA, '/data/music/a.flac': 'x', '/data/bluetooth/bluez/AA/info': 'Name=x\n', '/data/logs/a': 'x'}}),
    ('reset-execute-defaults-unavailable', ['reset', 'execute'], {**RESET, 'plan_first': 'settings',
                                                                  'commands': BLKID, 'files': {**DATA, **SETTINGS}}),
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path)."""
    for number, partition, controller in (('259:5', 'mmcblk0p9', '11230000.mmc'), ('259:6', 'mmcblk0p8', '11230000.mmc'),
                                          ('259:7', 'mmcblk1p1', '11240000.mmc')):
        device = root / 'sys/devices/platform' / controller / 'mmc_host/mmc0/mmc0:aaaa/block' / partition
        device.mkdir(parents=True, exist_ok=True)
        (device / 'dev').write_text(number + '\n')
        link = root / 'sys/dev/block' / number
        link.parent.mkdir(parents=True, exist_ok=True)
        if not link.is_symlink():
            link.symlink_to(os.path.relpath(device, link.parent))
    queue = root / QUEUE.lstrip('/')
    queue.mkdir(parents=True, exist_ok=True)
    queue.chmod(0o700)
    for name, data in ARCHIVES.items():
        path = queue / name
        path.write_bytes(data)
        path.chmod(0o600)
