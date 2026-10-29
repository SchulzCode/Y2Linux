"""Golden cases of port group `devices` (see tests/platform_cases/__init__.py): ssh-daemon, usb-mode,
transfer, codec-settings, time-set, time-bootstrap, rtc-alarm.

EXTRA_CASES (tests/test_platform_devices.py runs them): a successful `transfer begin` (its stage directory is
named by a random id, renamed to a fixed one before the comparison), `transfer commit` past its stage lock, an
ssh-daemon that starts dropbear (a direct exec: skipped on a host that has /usr/sbin/dropbear), and inputs the
platform ends on with a traceback.
"""
import json
import os
import time

KEY = {'/root/.ssh/authorized_keys': 'ssh-ed25519 AAAAC3Nza owner\n'}
TICK = {'Y2_DAEMON_TICKS': '1'}
IP = ['ip', '-j', '-4', 'addr', 'show', 'dev', 'usb0']
USB0 = json.dumps([{'ifindex': 5, 'ifname': 'usb0', 'addr_info': [
    {'family': 'inet', 'local': '10.42.0.1', 'prefixlen': 24}]}])


def answer(argv, output='', ok=True):
    return {'argv': argv, 'ok': ok, 'reason': None if ok else 'command_failed', 'output': output if ok else None}


# A verified /data: its own mount (device 259:1, whose sysfs link fixture() makes) and Y2DATA's UUID.
MOUNTS = ('22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
          '30 22 259:1 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n')
DATA = {'/proc/self/mountinfo': MOUNTS}
BLKID = [answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], '79324c69-6e75-4801-8000-000000000102')]
ID = '0123456789abcdef0123456789abcdef'
STAGE = '/data/transfer-staging/' + ID
HASH = '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824'  # sha256(b'hello')
MANIFEST = json.dumps({'schema': 1, 'id': ID, 'name': 'Album/song.flac', 'bytes': 5, 'sha256': HASH,
                       'data_generation': 'boot-a:30:259:1', 'state': 'Staging',
                       'record': {'workload': 'usb-transfer', 'result': None, 'failure': None}})
BEGIN = ['transfer', 'begin', '--name', 'Album/song.flac', '--bytes', '5', '--sha256', HASH]

POLICY = '/data/bluetooth/codec-policy.json'
INVENTORY = '/etc/y2linux/bluetooth-codecs.json'
SETTINGS = {'schema': 1, 'experimental': False, 'sbc_quality': 'high', 'sbc_abr': True, 'ldac_quality': 'standard',
            'ldac_abr': True, 'aac_quality': 'high', 'aac_abr': True}
LIVE = {'/run/y2/bluealsa.pid': '4242\n', '/proc/4242/cmdline': '/usr/bin/bluealsad\0-p\0a2dp-source\0',
        '/run/y2/codec-runtime.json': json.dumps({'boot_id': 'boot-a', 'settings': SETTINGS,
                                                  'arguments': ['-p', 'a2dp-source']})}

DATE = ['/bin/busybox', 'date', '-u', '-s']
HWCLOCK = ['/bin/busybox', 'hwclock', '-w', '-u']
RTC = '/sys/class/rtc/rtc0/'
LAST = '/data/system/platform/time-last.json'


def anchor(source):
    """/run/y2/time.json of this boot as ntp-event writes it, made when the case runs."""
    return lambda: json.dumps({'schema': 1, 'boot_id': 'boot-a', 'epoch': time.time(), 'monotonic_s': time.monotonic(),
                               'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME), 'source': source})


CASES = [
    # usb-mode
    ('usb-mode-mtp', ['usb-mode', 'mtp'], {}),
    ('usb-mode-developer', ['usb-mode', 'developer'], {}),
    ('usb-mode-invalid', ['usb-mode', 'mass-storage'], {}),
    # time-set (#53)
    ('time-set', ['time-set', '1788100000'], {'commands': [
        answer(DATE + ['@1788100000']), answer(HWCLOCK)]}),
    ('time-set-no-rtc-write', ['time-set', '1788100000'], {'commands': [answer(DATE + ['@1788100000'])],
                                                           'files': {'/etc/y2linux/rtc-policy.json': '{}'}}),
    ('time-set-restarts-ntp', ['time-set', '1788100000'], {'files': {'/run/y2/ntp.pid': '77\n'},
                                                           'commands': [answer(DATE + ['@1788100000']),
                                                                        answer(['/etc/init.d/S43y2-time', 'start'])]}),
    ('time-set-out-of-range', ['time-set', '1700000000'], {}),
    ('time-set-negative', ['time-set', '-5'], {}),
    ('time-set-network-time', ['time-set', '1788100000'], {'files': {'/run/y2/time.json': anchor('ntp')}}),
    ('time-set-manual-again', ['time-set', '1788100000'], {'commands': [answer(DATE + ['@1788100000'])],
                                                           'files': {'/run/y2/time.json': anchor('manual')}}),
    ('time-set-date-fails', ['time-set', '1788100000'], {}),
    ('time-set-not-a-number', ['time-set', 'soon'], {}),
    # time-bootstrap
    ('time-bootstrap', ['time-bootstrap'], {}),
    ('time-bootstrap-retained', ['time-bootstrap'], {'files': {RTC + 'date': '2026-10-28\n'}}),
    ('time-bootstrap-manual', ['time-bootstrap'], {'files': {
        RTC + 'date': '2026-10-28\n', LAST: json.dumps({'epoch': 1788000500.5, 'source': 'manual'})}}),
    ('time-bootstrap-floor', ['time-bootstrap'], {'files': {'/etc/y2linux/build-epoch': '1900000000\n'},
                                                  'commands': [answer(DATE + ['@1900000000'])]}),
    ('time-bootstrap-floor-manual', ['time-bootstrap'], {'files': {
        '/etc/y2linux/build-epoch': '1900000000\n', LAST: json.dumps({'epoch': 1900000100.75, 'source': 'manual'})},
        'commands': [answer(DATE + ['@1900000100'])]}),
    ('time-bootstrap-date-fails', ['time-bootstrap'], {'files': {'/etc/y2linux/build-epoch': '1900000000\n'}}),
    ('time-bootstrap-no-floor', ['time-bootstrap'], {'files': {'/etc/y2linux/build-epoch': None, LAST: None}}),
    ('time-bootstrap-extra', ['time-bootstrap', 'now'], {}),
    # rtc-alarm
    ('rtc-alarm', ['rtc-alarm', '60'], {'files': {RTC + 'since_epoch': '1788000000\n'}}),
    ('rtc-alarm-disable', ['rtc-alarm', '0'], {'files': {RTC + 'since_epoch': '1788000000\n',
                                                                         RTC + 'wakealarm': '1788000300\n'}}),
    ('rtc-alarm-no-rtc', ['rtc-alarm', '60'], {}),
    ('rtc-alarm-too-far', ['rtc-alarm', '604801'], {}),
    ('rtc-alarm-negative', ['rtc-alarm', '-1'], {}),
    ('rtc-alarm-not-a-number', ['rtc-alarm', '1h'], {}),
    # codec-settings
    ('codec-settings', ['codec-settings'], {}),
    ('codec-settings-sbc', ['codec-settings', '--sbc-quality', 'xq+', '--sbc-abr', 'off'], {}),
    ('codec-settings-aac-ldac', ['codec-settings', '--aac-quality=standard', '--ldac-abr', 'off', '--aac-abr', 'on',
                                 '--ldac-quality', 'mobile'], {'files': {POLICY: json.dumps(
                                     {'schema': 1, 'experimental': True, 'sbc_quality': 'xq'})}}),
    ('codec-settings-no-ldac', ['codec-settings', '--ldac-quality', 'high'], {'files': {
        INVENTORY: json.dumps({'schema': 1, 'codecs': {'AAC': {'compiled_locally': True}}})}}),
    ('codec-settings-no-aac', ['codec-settings', '--aac-abr', 'off'], {'files': {
        INVENTORY: json.dumps({'schema': 1, 'codecs': {'AAC': {'compiled_locally': 0}, 'LDAC': {'compiled_locally': 1}}})}}),
    ('codec-settings-bad-policy', ['codec-settings'], {'files': {POLICY: '{"schema": 2}'}}),
    ('codec-settings-repair-refused', ['codec-settings', '--sbc-quality', 'high'], {'files': {
        POLICY: '{"schema": 1, "sbc_quality": "max"}'}}),
    ('codec-settings-bad-boolean', ['codec-settings', '--sbc-quality', 'xq'], {'files': {
        POLICY: '{"schema": 1, "aac_abr": 1}'}}),
    ('codec-settings-live', ['codec-settings'], {'files': LIVE}),
    ('codec-settings-live-pending', ['codec-settings', '--sbc-abr', 'off'], {'files': LIVE}),
    ('codec-settings-other-daemon', ['codec-settings'], {'files': {
        **LIVE, '/proc/4242/cmdline': '/usr/bin/bluetoothd\0'}}),
    ('codec-settings-invalid-choice', ['codec-settings', '--sbc-quality', 'max'], {}),
    ('codec-settings-abbreviated', ['codec-settings', '--sbc-q', 'xq'], {}),
    # transfer status
    ('transfer-status', ['transfer', 'status', '--id', ID], {'files': {STAGE + '/manifest.json': MANIFEST}}),
    ('transfer-status-missing', ['transfer', 'status', '--id', ID], {}),
    ('transfer-status-unreadable', ['transfer', 'status', '--id=' + ID], {'files': {
        STAGE + '/manifest.json': '{"state": '}}),
    ('transfer-status-list', ['transfer', 'status', '--id', ID], {'files': {STAGE + '/manifest.json': '[1, 2]'}}),
    ('transfer-status-bad-id', ['transfer', 'status', '--id', ID.upper()], {}),
    ('transfer-status-no-id', ['transfer', 'status'], {}),
    # transfer begin
    ('transfer-begin-no-data', BEGIN, {}),
    ('transfer-begin-hidden-name', ['transfer', 'begin', '--name', 'Album/.song', '--bytes', '5', '--sha256', HASH],
     {}),
    ('transfer-begin-no-name', ['transfer', 'begin', '--bytes', '5', '--sha256', HASH], {}),
    ('transfer-begin-deep-name', ['transfer', 'begin', '--name', '/'.join('abcdefghi'), '--bytes', '5', '--sha256', HASH],
     {}),
    # A name that is not UTF-8: the parser refuses it (exit 2) before transfer runs.
    ('transfer-begin-not-utf8', ['transfer', 'begin', '--name', 'A/\udcff\udcfe.flac', '--bytes', '5', '--sha256', HASH],
     {}),
    ('transfer-begin-no-hash', ['transfer', 'begin', '--name', 'a.flac', '--bytes', '5'], {}),
    ('transfer-begin-too-large', ['transfer', 'begin', '--name', 'a.flac', '--bytes', '536870913', '--sha256', HASH],
     {}),
    ('transfer-begin-empty', ['transfer', 'begin', '--name', 'a.flac', '--bytes', '0', '--sha256', HASH], {}),
    ('transfer-begin-staging-full', BEGIN, {'commands': BLKID, 'files': {
        **DATA, **{'/data/transfer-staging/%032x/manifest.json' % n: '{}' for n in range(8)}}}),
    ('transfer-begin-staging-file', BEGIN, {'commands': BLKID, 'files': {
        **DATA, '/data/transfer-staging': 'not a directory'}}),
    ('transfer-begin-wrong-uuid', BEGIN, {'files': DATA, 'commands': [
        answer(BLKID[0]['argv'], '79324c69-6e75-4801-8000-000000000999')]}),
    ('transfer-begin-read-only', BEGIN, {'commands': BLKID, 'files': {
        '/proc/self/mountinfo': MOUNTS.replace('rw,noatime', 'ro,noatime')}}),
    ('transfer-begin-bytes-not-int', ['transfer', 'begin', '--bytes', 'five'], {}),
    ('transfer-begin-repeated', ['transfer', 'begin', '--name', 'a', '--name', 'b'], {}),
    # transfer commit (the workload lease first, then the checks up to the stage lock)
    ('transfer-commit-bad-id', ['transfer', 'commit', '--id', 'nope'], {}),
    ('transfer-commit-no-data', ['transfer', 'commit', '--id', ID], {}),
    ('transfer-commit-no-staging', ['transfer', 'commit', '--id', ID], {'commands': BLKID,
                                                                         'files': DATA}),
    ('transfer-commit-no-stage', ['transfer', 'commit', '--id', ID], {'commands': BLKID, 'files': {
        **DATA, '/data/transfer-staging/other': ''}}),
    ('transfer-commit-stage-file', ['transfer', 'commit', '--id', ID], {'commands': BLKID, 'files': {
        **DATA, STAGE: 'not a directory'}}),
    # transfer discard
    ('transfer-discard', ['transfer', 'discard', '--id', ID], {'commands': BLKID, 'files': {
        **DATA, STAGE + '/manifest.json': MANIFEST, STAGE + '/payload.part': 'hello'}}),
    ('transfer-discard-extra-file', ['transfer', 'discard', '--id', ID], {'commands': BLKID, 'files': {
        **DATA, STAGE + '/manifest.json': MANIFEST, STAGE + '/notes.txt': 'kept'}}),
    ('transfer-discard-no-data', ['transfer', 'discard', '--id', ID], {}),
    ('transfer-discard-bad-id', ['transfer', 'discard', '--id', 'x'], {'commands': BLKID, 'files': DATA}),
    ('transfer-discard-no-staging', ['transfer', 'discard', '--id', ID], {'commands': BLKID,
                                                                           'files': DATA}),
    ('transfer-discard-no-stage', ['transfer', 'discard', '--id', ID], {'commands': BLKID, 'files': {
        **DATA, '/data/transfer-staging/other': ''}}),
    ('transfer-unknown-action', ['transfer', 'list'], {}),
    # ssh-daemon, one or two loop iterations (Y2_DAEMON_TICKS)
    ('ssh-no-key', ['ssh-daemon'], {'env': TICK}),
    ('ssh-no-interface', ['ssh-daemon'], {'env': TICK, 'files': KEY}),
    ('ssh-no-address', ['ssh-daemon'], {'env': TICK, 'files': KEY, 'commands': [answer(IP, '[]')]}),
    ('ssh-odd-address', ['ssh-daemon'], {'env': TICK, 'files': KEY, 'commands': [
        answer(IP, '[{"ifname": "usb0", "addr_info": 7}]')]}),
    ('ssh-no-data', ['ssh-daemon'], {'env': TICK, 'commands': [answer(IP, USB0)], 'files': {
        **KEY, '/data/.y2data-schema': None, '/usr/libexec/y2-sftp-server': '#!/bin/sh\n'}}),
    ('ssh-two-ticks', ['ssh-daemon'], {'env': {**TICK, 'Y2_DAEMON_TICKS': '2'}, 'files': KEY}),
    ('ssh-two-ticks-unavailable', ['ssh-daemon'], {'env': {**TICK, 'Y2_DAEMON_TICKS': '2'},
                                                   'commands': [answer(IP, USB0)]}),
    ('ssh-extra-argument', ['ssh-daemon', '--foreground'], {}),
]

# A verified Y2Media mounted and recorded Ready this boot (device 259:2), beside the verified /data.
MEDIA = {'/proc/self/mountinfo': MOUNTS + '31 22 259:2 / /media/internal rw,noatime - ext4 /dev/mmcblk0p8 rw\n',
         '/run/y2/internal-media.json': json.dumps({'schema': 1, 'state': 'Ready', 'boot_id': 'boot-a',
                                                    'mount_id': 31, 'device_id': '259:2'}),
         '/media/internal/music/Other/old.flac': 'old'}
MEDIA_BLKID = BLKID + [answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p8'],
                              '79324c69-6e75-4801-8000-000000000103')]
UPLOAD = {**DATA, STAGE + '/manifest.json': MANIFEST, STAGE + '/payload.part': 'hello'}
COMMIT = ['transfer', 'commit', '--id', ID]


def manifest(**changes):
    return json.dumps({**json.loads(MANIFEST), **changes})


# (name, argv, options) as CASES; `rename_stage`: the random stage directory is renamed before comparing.
EXTRA_CASES = [
    ('transfer-begin', BEGIN, {'commands': BLKID, 'files': DATA, 'rename_stage': True}),
    ('transfer-begin-again', BEGIN, {'commands': BLKID, 'rename_stage': True, 'files': {
        **DATA, '/data/transfer-staging/old/manifest.json': '{}'}}),
    ('transfer-commit', COMMIT, {'commands': BLKID, 'files': UPLOAD}),
    ('transfer-commit-internal', COMMIT, {'commands': MEDIA_BLKID, 'files': {**UPLOAD, **MEDIA}}),
    ('transfer-commit-internal-unverified', COMMIT, {'commands': BLKID, 'files': {**UPLOAD, **MEDIA}}),
    ('transfer-commit-committed', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(state='Committed')}}),
    ('transfer-commit-exists', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, '/data/music/Album/song.flac': 'mine'}}),
    ('transfer-commit-sealed-left', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/sealed.part': 'hel'}}),
    ('transfer-commit-wrong-hash', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/payload.part': 'HELLO'}}),
    ('transfer-commit-short', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/payload.part': 'hell'}}),
    ('transfer-commit-no-payload', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/payload.part': None}}),
    ('transfer-commit-other-generation', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(data_generation='boot-z:30:259:1')}}),
    ('transfer-commit-bad-manifest', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(bytes=True)}}),
    ('transfer-commit-bad-state', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(state='Uploading')}}),
    ('transfer-commit-bad-name', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(name='../x')}}),
    ('transfer-commit-record-not-dict', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': manifest(record=None)}}),
    ('transfer-commit-manifest-list', COMMIT, {'commands': BLKID, 'files': {
        **UPLOAD, STAGE + '/manifest.json': '[]'}}),
    # Python's TypeError/AttributeError (a traceback, nothing on stdout); a change takes the lock first.
    ('codec-settings-live-null-arguments', ['codec-settings'], {'files': {
        **LIVE, '/run/y2/codec-runtime.json': '{"boot_id": "boot-a", "arguments": null}'}}),
    ('codec-settings-live-number-arguments', ['codec-settings', '--sbc-abr', 'off'], {'files': {
        **LIVE, '/run/y2/codec-runtime.json': '{"boot_id": "boot-a", "arguments": 1.5}'}}),
    ('codec-settings-broken-inventory', ['codec-settings'], {'files': {
        INVENTORY: '{"schema": 1, "codecs": {"AAC": 3}}'}}),
    ('codec-settings-null-codecs', ['codec-settings', '--sbc-quality', 'xq'], {'files': {
        INVENTORY: '{"schema": 1, "codecs": null}'}}),
    ('codec-settings-list-ldac', ['codec-settings'], {'files': {
        INVENTORY: '{"schema": 1, "codecs": {"AAC": {}, "LDAC": []}}'}}),
    # Every prerequisite holds: dropbear starts (on a host without /usr/sbin/dropbear: FileNotFoundError).
    ('ssh-starts-dropbear', ['ssh-daemon'], {'env': TICK, 'commands': [answer(IP, USB0)],
                                             'files': KEY, 'needs_no_dropbear': True}),
]


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path)."""
    # The eMMC partitions the transfer cases mount (by their own mountinfo) as /data and /media/internal:
    # device numbers nothing else in the shared root uses.
    for number, partition in (('259:1', 'mmcblk0p9'), ('259:2', 'mmcblk0p8')):
        device = root / 'sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0' / partition
        device.mkdir(parents=True, exist_ok=True)
        (device / 'dev').write_text(number + '\n')
        link = root / 'sys/dev/block' / number
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(os.path.relpath(device, link.parent))
