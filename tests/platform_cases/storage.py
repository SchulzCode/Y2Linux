"""Differential cases of port group `storage` (see tests/platform_cases/__init__.py): `media` (every action),
`internal-media` (every action) and `space`.

The shared fixture gets one SD card (mmcblk1 with partition p1 on the removable-card controller, inserted, not
mounted) and the internal Y2Media partition (mmcblk0p8 at the stock geometry). A case mounts them by replacing
/proc/self/mountinfo and answers blkid/fsck/mount from its command list. A fixture command never changes the
tree, so a mount or umount that succeeded is still not seen in mountinfo: those cases end in the checks that
follow (mount_identity_failed, unmount_busy, old_storage_busy).
"""
import hashlib
import json

BLOCK = 'sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:aaaa'
DISK = BLOCK + '/block/mmcblk1'
PART = DISK + '/mmcblk1p1'
EMMC = 'sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0002/block/mmcblk0'
P8 = EMMC + '/mmcblk0p8'
CID = '035344534433324780a1b2c3d4016a00'
SD_UUID = '1234-ABCD'
INSTANCE = 'block:179:97:diskseq:9'
Y2MEDIA = '79324c69-6e75-4801-8000-000000000103'
SERIAL = 'B2C1-0A7F'
BASE_MOUNTS = ('22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
               '30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n')
SD_RO = '51 22 179:97 / /media/sd ro,nosuid,nodev,noexec,noatime - vfat /dev/mmcblk1p1 ro,fmask=0177,dmask=0077\n'
SD_RW = SD_RO.replace('ro,nosuid', 'rw,nosuid', 1)
INTERNAL = '52 22 179:8 / /media/internal rw,nosuid,nodev,noexec,noatime - ext4 /dev/mmcblk0p8 rw\n'
OLD = '53 22 179:8 / /run/y2/old-storage ro,nosuid - vfat /dev/mmcblk0p8 ro\n'


def identity(filesystem='vfat', uuid=SD_UUID):
    """media.media_identity of the fixture card's partition."""
    value = {'cid': CID, 'geometry': {'partition': '1', 'start': '8192', 'size': '62513152'},
             'filesystem': filesystem, 'uuid': uuid}
    value['id'] = hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()
    return value


def claim(read_only='protected', **changes):
    """/run/y2/media-mount.json as a mount of the fixture card writes it."""
    value = {'schema': 1, 'action': 'mount', 'state': 'Ready', 'reason': None, 'boot_id': 'boot-a',
             'device': '/dev/mmcblk1p1', 'filesystem': 'vfat', 'uuid': SD_UUID, 'cid': CID,
             'identity': identity(), 'source_instance': INSTANCE, 'mount_id': 51, 'device_id': '179:97',
             'read_only': read_only,
             'filesystem_check': {'ok': True, 'reason': None, 'output': '', 'elapsed_ms': 3, 'findings': []}}
    value.update(changes)
    return json.dumps(value)


def ok(argv, output=''):
    return {'argv': argv, 'ok': True, 'reason': None, 'output': output}


def failed(argv, output=None, reason='command_failed'):
    return {'argv': argv, 'ok': False, 'reason': reason, 'output': output}


def mounts(*lines):
    return {'/proc/self/mountinfo': BASE_MOUNTS + ''.join(lines)}


BLKID = ['blkid', '-p', '-o', 'export', '/dev/mmcblk1p1']
UUID_OF = ['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk1p1']
FSCK = ['fsck.fat', '-n', '/dev/mmcblk1p1']
MOUNT_RO = ['mount', '-t', 'vfat', '-o', 'ro,nosuid,nodev,noexec,noatime,fmask=0177,dmask=0077,utf8=1',
            '/dev/mmcblk1p1', '/media/sd']
VFAT = ok(BLKID, 'DEVNAME=/dev/mmcblk1p1\nUUID=%s\nTYPE=vfat\nPARTENTRY_NUMBER=1' % SD_UUID)
EXT4 = ok(BLKID, 'UUID=%s\nTYPE=ext4' % SD_UUID)
EXFAT = ok(BLKID, 'UUID=%s\nTYPE=exfat' % SD_UUID)
DIRTY = ('fsck.fat 4.2 (2021-01-31)\nDirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n'
         ' Automatically removing dirty bit.\nThere are differences between boot sector and its backup.\n'
         'This is mostly harmless. Differences: (offset:original/backup)\n  65:01/00\n  Not automatically fixing this.\n'
         'Leaving filesystem unchanged.\n/dev/mmcblk1p1: 12 files, 3/100 clusters')
DAMAGED = ('fsck.fat 4.2 (2021-01-31)\n/MUSIC/Song.mp3\n  File size is 4096 bytes, cluster chain length is 0.\n'
           "Free cluster summary wrong (5 vs. really 7)\nAuto-correcting.\nVolume label 'MY CARD' stored in root "
           "directory is not valid.\n 3:00/01, 65:01/00\nOrphaned long file name part \"x\"\nAuto-deleting.")
MOUNTED = {**mounts(SD_RO), '/run/y2/media-mount.json': claim(), '/media/sd/.keep': ''}
SD_STATUS = [VFAT, ok(UUID_OF, SD_UUID)]
REFUSED = json.dumps({'schema': 1, 'action': 'mount', 'state': 'Degraded', 'reason': 'filesystem_check_failed',
                      'boot_id': 'boot-a', 'device': '/dev/mmcblk1p1', 'filesystem': 'ext4', 'uuid': SD_UUID,
                      'cid': CID, 'identity': identity('ext4'), 'source_instance': INSTANCE,
                      'filesystem_check': {'ok': False, 'reason': 'command_failed', 'output': None,
                                           'elapsed_ms': 9, 'findings': ['other']}})
POWER = json.dumps({'boot_id': 'boot-a', 'battery': {'charging': False, 'status': 'Discharging', 'percent': 64}})
LIFECYCLE = [['mmcblk1', '179:96', '9', CID], ['mmcblk1p1', '179:97', '9', CID]]
# A FAT32 boot sector (512-byte sectors, 32 reserved, two FATs of 8 sectors): 24 KiB of tables.
BOOT = ('\x00' * 11 + '\x00\x02' + '\x08' + '\x20\x00' + '\x02' + '\x00' * 19 + '\x08\x00\x00\x00').ljust(512, '\x00')
CARD = BOOT + 'y' * 32768
SMALL = {DISK + '/size': '20000\n', '/dev/mmcblk1': ''}
REPAIR = ['fsck.fat', '-a', '-w', '/dev/mmcblk1p1']
WRITE_VOLATILE = ('reborn', 'usb', 'report', 'radiofw')

MEDIA = [
    ('media-status', ['media', 'status'], {'ported': True}),
    ('media-status-json', ['media', 'status', '--json'], {'ported': True}),
    ('media-status-refused', ['media', 'status'], {'ported': True, 'files': {'/run/y2/media.json': REFUSED}}),
    ('media-status-protected', ['media', 'status'], {'ported': True, 'files': MOUNTED, 'commands': SD_STATUS}),
    ('media-status-errors', ['media', 'status'], {'ported': True, 'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('filesystem_errors')}}),
    ('media-status-rw-undersized', ['media', 'status'], {'ported': True, 'commands': SD_STATUS, 'files': {
        **MOUNTED, **mounts(SD_RW), PART + '/size': '99999999999\n'}}),
    ('media-status-no-claim', ['media', 'status'], {'ported': True, 'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': None}}),
    ('media-status-other-card', ['media', 'status'], {'ported': True, 'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim(identity={'id': 'x'})}}),
    ('media-status-no-mountpoint', ['media', 'status'], {'ported': True, 'commands': SD_STATUS, 'files': {
        **MOUNTED, '/media/sd/.keep': None}}),
    ('media-mount', ['media', 'mount'], {'ported': True, 'commands': [VFAT, ok(FSCK), ok(MOUNT_RO)]}),
    ('media-mount-dirty', ['media', 'mount'], {'ported': True, 'commands': [VFAT, failed(FSCK, DIRTY)]}),
    ('media-mount-errors', ['media', 'mount'], {'ported': True, 'commands': [VFAT, failed(FSCK, DAMAGED)]}),
    ('media-mount-timeout', ['media', 'mount'], {'ported': True, 'commands': [
        EXFAT, failed(['fsck.exfat', '-n', '/dev/mmcblk1p1'], reason='command_timeout')]}),
    ('media-mount-exfat-errors', ['media', 'mount'], {'ported': True, 'commands': [
        EXFAT, failed(['fsck.exfat', '-n', '/dev/mmcblk1p1'])]}),
    ('media-mount-ext4-refused', ['media', 'mount'], {'ported': True, 'commands': [EXT4]}),
    ('media-mount-ext4', ['media', 'mount'], {'ported': True, 'commands': [
        EXT4, ok(['e2fsck', '-n', '/dev/mmcblk1p1'])]}),
    ('media-mount-always-writable', ['media', 'mount'], {'ported': True, 'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-always-writable': ''}}),
    ('media-mount-history', ['media', 'mount'], {'ported': True, 'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-admissions.json': json.dumps({'schema': 1, 'admissions': [
            {'time': 1, 'card': 'old%d' % i, 'ok': True} for i in range(10)] + ['not a dict']})}}),
    ('media-mount-history-null', ['media', 'mount'], {'ported': True, 'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-admissions.json': '{"admissions": null}'}}),
    ('media-mount-no-card', ['media', 'mount'], {'ported': True, 'files': {
        '/sys/class/block/mmcblk1': None, '/sys/class/block/mmcblk1p1': None}}),
    ('media-mount-no-identity', ['media', 'mount'], {'ported': True, 'commands': [ok(BLKID, 'TYPE=vfat')],
                                                     'files': {BLOCK + '/cid': None}}),
    ('media-mount-whole-disk', ['media', 'mount'], {'ported': True, 'commands': [
        ok(['blkid', '-p', '-o', 'export', '/dev/mmcblk1'], 'TYPE=exfat\nUUID=AB')], 'files': {
        '/sys/class/block/mmcblk1p1': None, PART + '/partition': None}}),
    ('media-mount-already', ['media', 'mount'], {'ported': True, 'files': MOUNTED}),
    ('media-mount-foreign', ['media', 'mount'], {'ported': True, 'files': mounts(SD_RO.replace('179:97', '8:1'))}),
    ('media-mount-file-in-the-way', ['media', 'mount'], {'ported': True, 'commands': [VFAT, ok(FSCK)],
                                                         'files': {'/media/sd': 'not a directory'}}),
    ('media-unmount-not-mounted', ['media', 'unmount'], {'ported': True}),
    ('media-unmount-busy', ['media', 'unmount'], {'ported': True, 'files': MOUNTED,
                                                  'commands': [ok(['umount', '/media/sd'])]}),
    ('media-unmount-fails', ['media', 'unmount'], {'ported': True, 'files': MOUNTED}),
    ('media-unmount-stacked', ['media', 'unmount'], {'ported': True, 'files': mounts(SD_RO, SD_RO.replace('51', '57', 1))}),
    ('media-unmount-usb', ['media', 'unmount'], {'ported': True, 'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'attached',
         'views': [{'path': '/media/internal/music'}, {'path': '/media/sd'}]})}}),
    ('media-check', ['media', 'check'], {'ported': True, 'commands': [VFAT, ok(FSCK, 'clean')]}),
    ('media-check-ext4', ['media', 'check'], {'ported': True, 'commands': [
        EXT4, failed(['e2fsck', '-f', '-n', '/dev/mmcblk1p1'])]}),
    ('media-check-mounted', ['media', 'check'], {'ported': True, 'files': MOUNTED}),
    ('media-reconcile', ['media', 'reconcile'], {'ported': True, 'commands': [VFAT, ok(FSCK)]}),
    ('media-reconcile-no-card', ['media', 'reconcile'], {'ported': True, 'files': {
        '/sys/class/block/mmcblk1': None, '/sys/class/block/mmcblk1p1': None,
        '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'attempts': True})}}),
    ('media-reconcile-same', ['media', 'reconcile'], {'ported': True, 'files': {
        '/run/y2/media-lifecycle.json': json.dumps({'schema': 1, 'inventory': LIFECYCLE, 'attempts': 1,
                                                    'retry': False, 'state': 'Ready', 'reason': None})}}),
    ('media-reconcile-retry', ['media', 'reconcile'], {'ported': True, 'files': {
        **MOUNTED, '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'attempts': 2, 'retry': True})}}),
    ('media-reconcile-protect', ['media', 'reconcile'], {'ported': True, 'files': {
        **MOUNTED, **mounts(SD_RW), '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'state': 'Ready'})},
        'commands': [ok(['mount', '-o', 'remount,ro', '/media/sd'])]}),
    ('media-verify-nothing', ['media', 'verify'], {'ported': True}),
    ('media-verify', ['media', 'verify'], {'ported': True, 'commands': [VFAT, ok(FSCK)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-writable', ['media', 'verify'], {'ported': True, 'commands': [VFAT, ok(FSCK)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking'), '/data/system/platform/sd-always-writable': ''}}),
    ('media-verify-errors', ['media', 'verify'], {'ported': True, 'commands': [VFAT, failed(FSCK, DAMAGED)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-timeout', ['media', 'verify'], {'ported': True, 'commands': [
        VFAT, failed(FSCK, reason='command_timeout')], 'files': {**MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-other-card', ['media', 'verify'], {'ported': True, 'commands': [VFAT], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking', source_instance='block:179:97:diskseq:3')}}),
    ('media-verify-bad-identity', ['media', 'verify'], {'ported': False, 'commands': [VFAT], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking', identity=['x'])}}),
    ('media-format-unconfirmed', ['media', 'format'], {'ported': True}),
    ('media-format-wrong-card', ['media', 'format', '--confirm-cid', 'other'], {'ported': True}),
    ('media-format', ['media', 'format', '--confirm-cid', CID], {'ported': True, 'files': SMALL}),
    ('media-format-not-writable', ['media', '--confirm-cid', CID, 'format'], {'ported': True, 'files': {
        DISK + '/size': '20000\n'}}),
    ('media-format-too-small', ['media', 'format', '--confirm-cid', CID], {'ported': True, 'files': {
        DISK + '/size': '100\n'}}),
    ('media-format-no-card', ['media', 'format', '--confirm-cid', CID], {'ported': True, 'files': {
        '/sys/class/block/mmcblk1': None}}),
    ('media-format-mounted', ['media', 'format', '--confirm-cid', CID], {'ported': True, 'files': {**MOUNTED, **SMALL},
                                                                          'commands': [ok(['umount', '/media/sd'])]}),
    ('media-grow-not-ext4', ['media', 'grow'], {'ported': True, 'commands': [VFAT]}),
    ('media-grow-check-fails', ['media', 'grow'], {'ported': True, 'commands': [EXT4]}),
    ('media-grow-resize-fails', ['media', 'grow'], {'ported': True, 'commands': [
        EXT4, ok(['e2fsck', '-f', '-p', '/dev/mmcblk1p1'])]}),
    ('media-grow', ['media', 'grow'], {'ported': True, 'commands': [
        EXT4, ok(['e2fsck', '-f', '-p', '/dev/mmcblk1p1']), ok(['resize2fs', '/dev/mmcblk1p1']),
        ok(['e2fsck', '-n', '/dev/mmcblk1p1'])]}),
    ('media-repair-not-marked', ['media', 'repair'], {'ported': True}),
    ('media-repair-on-battery', ['media', 'repair'], {'ported': True, 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER.replace('64', '12')}}),
    ('media-repair', ['media', 'repair'], {'ported': True, 'commands': [
        EXT4, ok(['e2fsck', '-p', '/dev/mmcblk1p1'], 'Truncating file to 0 bytes'), ok(['e2fsck', '-n', '/dev/mmcblk1p1']),
        ok(['mount', '-t', 'ext4', '-o', 'ro,nosuid,nodev,noexec,noatime', '/dev/mmcblk1p1', '/media/sd'])], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER, '/dev/mmcblk1p1': CARD}}),
    ('media-repair-vfat', ['media', 'repair'], {'ported': True, 'commands': [
        VFAT, failed(REPAIR, 'Reclaimed 3 unused clusters (98304 bytes).\n/A/b.mp3\n  Truncating file to 0 bytes'),
        failed(FSCK, DIRTY)], 'files': {
        '/run/y2/media.json': REFUSED.replace('"ext4"', '"vfat"').replace(identity('ext4')['id'], identity()['id']),
        '/run/y2/power.json': POWER.replace('false', 'true', 1), '/dev/mmcblk1p1': CARD,
        '/data/system/platform/sd-repair/old.img': 'old'}}),
    ('media-repair-timeout', ['media', 'repair'], {'ported': True, 'commands': [
        EXT4, failed(['e2fsck', '-p', '/dev/mmcblk1p1'], reason='command_timeout')], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER}}),
    ('media-repair-short-device', ['media', 'repair'], {'ported': True, 'commands': [VFAT], 'files': {
        '/run/y2/media.json': REFUSED.replace('"ext4"', '"vfat"').replace(identity('ext4')['id'], identity()['id']),
        '/run/y2/power.json': POWER, '/dev/mmcblk1p1': BOOT}}),
    ('media-repair-mounted', ['media', 'repair'], {'ported': True, 'commands': [ok(['umount', '/media/sd'])],
                                                   'files': {**MOUNTED, '/run/y2/power.json': POWER,
                                                             '/run/y2/media-mount.json': claim('check_timed_out')}}),
    ('media-repair-changed-card', ['media', 'repair'], {'ported': True, 'commands': [VFAT], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER}}),
    ('media-write-not-mounted', ['media', 'write'], {'ported': True}),
    ('media-write', ['media', 'write'], {'ported': True, 'files': MOUNTED, 'volatile': WRITE_VOLATILE,
                                         'commands': [ok(['mount', '-o', 'remount,rw', '/media/sd'])]}),
    ('media-write-long', ['media', 'write', '--holder', 'radiofw', '--seconds', '5000', '--holder', 'usb'], {
        'ported': True, 'files': MOUNTED, 'volatile': WRITE_VOLATILE}),
    ('media-write-release', ['media', 'write', '--holder', 'usb', '--release'], {'ported': True, 'files': {
        **MOUNTED, '/run/y2/sd-write.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 51, 'holders': {
            'usb': 1e12, 'reborn': 0, 'stranger': 1e12}})}, 'volatile': WRITE_VOLATILE}),
    ('media-write-held', ['media', 'write', '--holder', 'report', '--seconds', '0'], {'ported': True, 'files': {
        **MOUNTED, **mounts(SD_RW), '/run/y2/sd-write.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 51.0, 'holders': {
            'usb': 1e12}})}, 'volatile': WRITE_VOLATILE}),
    ('media-write-checking', ['media', 'write'], {'ported': True, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-write-errors', ['media', 'write'], {'ported': True, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('filesystem_errors')}}),
    ('media-write-unprotected', ['media', 'write'], {'ported': True, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim(None)}}),
    ('media-help', ['media', '--help'], {'ported': True}),
    ('media-no-action', ['media'], {'ported': True}),
    ('media-bad-action', ['media', 'eject'], {'ported': True}),
    ('media-bad-holder', ['media', 'write', '--holder', 'nobody'], {'ported': True}),
    ('media-seconds-joined', ['media', 'write', '--seconds=5'], {'ported': True}),
    ('media-seconds-negative', ['media', 'write', '--seconds', '-5'], {'ported': True}),
]

P8_BLKID = ['blkid', '-p', '-o', 'export', '/dev/mmcblk0p8']
VALID = ok(P8_BLKID, 'LABEL=Y2MEDIA\nUUID=%s\nTYPE=ext4' % Y2MEDIA)
STOCK = ok(P8_BLKID, 'LABEL=Y2\nUUID=%s\nTYPE=vfat' % SERIAL)
MKFS = ['mkfs.ext4', '-F', '-q', '-L', 'Y2MEDIA', '-U', Y2MEDIA, '-m', '0', '-J', 'size=64', '-b', '4096', '-I',
        '256', '-i', '16384', '-O', 'none,has_journal,ext_attr,resize_inode,dir_index,orphan_file,filetype,extent,'
        '64bit,flex_bg,metadata_csum_seed,sparse_super,large_file,huge_file,dir_nlink,extra_isize,metadata_csum',
        '-E', 'lazy_itable_init=1,lazy_journal_init=1,nodiscard', '/dev/mmcblk0p8']
MOUNTED_INTERNAL = {**mounts(INTERNAL), '/media/internal/.keep': ''}
MUSIC = {'/data/music/Artist/Album/01.mp3': 'one', '/data/music/02.flac': 'two two', '/data/music/Empty/.keep': ''}
SETUP = '/data/system/internal-media-setup.json'
RECORD = {'schema': 1, 'device': '/dev/mmcblk0p8', 'start': '3742720', 'size': '11460608', 'serial': SERIAL,
          'phase': 'formatting', 'files': 2, 'bytes': 9}
OLD_FILES = {'/run/y2/old-storage/Music/a.mp3': 'aaaa', '/run/y2/old-storage/Thumbs.db': 'x',
             '/run/y2/old-storage/.thumbnails/t.jpg': 't', '/run/y2/old-storage/Notes/b.txt': 'bbbbb'}
ON_BATTERY = {'/sys/class/power_supply/BAT0/status': 'Discharging\n'}
PROFILE = json.dumps({'schema': 1, 'voltage_soc': [[3400000, 0], [3800000, 50], [4200000, 100]],
                      'filter_seconds': 30, 'rest_seconds': 600, 'rest_band_uv': 10000, 'charging_offset_uv': 50000,
                      'seconds_per_percent': 60, 'provisional': True, 'source': 'fixture'})

INTERNAL_CASES = [
    ('internal-status', ['internal-media', 'status'], {'ported': True}),
    ('internal-status-ready', ['internal-media', 'status'], {'ported': True, 'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, '/run/y2/internal-media.json': json.dumps(
            {'state': 'Ready', 'boot_id': 'boot-a', 'mount_id': 52, 'device_id': '179:8'})}}),
    ('internal-status-no-partition', ['internal-media', 'status'], {'ported': True, 'files': {
        P8 + '/start': '0\n'}}),
    ('internal-mount-stock', ['internal-media', 'mount'], {'ported': True, 'commands': [STOCK]}),
    ('internal-mount-ready', ['internal-media', 'mount'], {'ported': True, 'commands': [VALID], 'files': MOUNTED_INTERNAL}),
    ('internal-mount-foreign', ['internal-media', 'mount'], {'ported': True, 'commands': [VALID], 'files': mounts(
        INTERNAL.replace('/dev/mmcblk0p8', '/dev/sda1'))}),
    ('internal-mount-check-fails', ['internal-media', 'mount'], {'ported': True, 'commands': [VALID]}),
    ('internal-mount-not-seen', ['internal-media', 'mount'], {'ported': True, 'commands': [
        VALID, ok(['e2fsck', '-p', '/dev/mmcblk0p8']),
        ok(['mount', '-t', 'ext4', '-o', 'rw,nosuid,nodev,noexec,noatime,noinit_itable', '/dev/mmcblk0p8',
            '/media/internal'])]}),
    ('internal-mount-no-partition', ['internal-media', 'mount'], {'ported': True, 'files': {
        '/sys/class/block/mmcblk0p8': None}}),
    ('internal-unmount-not-mounted', ['internal-media', 'unmount'], {'ported': True}),
    ('internal-unmount-busy', ['internal-media', 'unmount'], {'ported': True, 'files': MOUNTED_INTERNAL,
                                                              'commands': [ok(['umount', '/media/internal'])]}),
    ('internal-unmount-fails', ['internal-media', 'unmount'], {'ported': True, 'files': MOUNTED_INTERNAL}),
    ('internal-provision-unconfirmed', ['internal-media', 'provision'], {'ported': True, 'commands': [STOCK]}),
    ('internal-provision-wrong-serial', ['internal-media', 'provision', '--confirm-serial', 'X'], {
        'ported': True, 'commands': [STOCK]}),
    ('internal-provision-mkfs-fails', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'ported': True, 'commands': [STOCK]}),
    ('internal-provision-in-use', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'ported': True, 'commands': [STOCK], 'files': mounts(OLD)}),
    ('internal-provision-migrate', ['internal-media', 'provision'], {'ported': True, 'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC}}),
    ('internal-provision-no-migrate', ['internal-media', 'provision', '--no-migrate'], {
        'ported': True, 'commands': [VALID], 'files': {**MOUNTED_INTERNAL, **MUSIC}}),
    ('internal-provision-present', ['internal-media', 'provision'], {'ported': True, 'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, '/media/internal/music/02.flac': 'two two'}}),
    ('internal-provision-differs', ['internal-media', 'provision'], {'ported': True, 'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, '/media/internal/music/02.flac': 'other'}}),
    ('internal-provision-mkfs', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'ported': True, 'commands': [STOCK, ok(MKFS)]}),
    ('internal-setup-not-needed', ['internal-media', 'setup'], {'ported': True, 'commands': [VALID]}),
    ('internal-setup-not-stock', ['internal-media', 'setup'], {'ported': True, 'commands': [
        ok(P8_BLKID, 'TYPE=ext4\nLABEL=OTHER')]}),
    ('internal-setup-unconfirmed', ['internal-media', 'setup'], {'ported': True, 'commands': [STOCK]}),
    ('internal-setup-no-partition', ['internal-media', 'setup', '--automatic'], {'ported': True, 'files': {
        P8 + '/size': '1\n'}}),
    ('internal-setup-mounted', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK],
                                                                            'files': mounts(INTERNAL)}),
    ('internal-setup-waiting', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK],
                                                                            'files': {'/run/reborn-splash/.keep': ''}}),
    ('internal-setup-update', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK], 'files': {
        '/data/updates/state.json': '{"schema": 2.0, "state": "PendingHealth"}'}}),
    ('internal-setup-battery-unknown', ['internal-media', 'setup', '--automatic'], {
        'ported': True, 'commands': [STOCK], 'files': ON_BATTERY}),
    ('internal-setup-battery-low', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK], 'files': {
        **ON_BATTERY, '/etc/y2linux/battery-profile.json': PROFILE, '/sys/class/power_supply/BAT0/voltage_ocv': '3500000\n'}}),
    ('internal-setup-battery-enough', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK], 'files': {
        **ON_BATTERY, '/etc/y2linux/battery-profile.json': PROFILE, '/sys/class/power_supply/BAT0/voltage_ocv': 'x\n',
        '/sys/class/power_supply/BAT0/voltage_now': '3900000\n'}}),
    ('internal-setup-keeps-files', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [
        STOCK, ok(['umount', '/run/y2/old-storage'])], 'files': {**mounts(OLD), **OLD_FILES, '/run/reborn-splash/.keep': ''}}),
    ('internal-setup-nothing-kept', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK],
                                                                                 'files': mounts(OLD)}),
    ('internal-setup-confirmed', ['internal-media', 'setup', '--confirm-shown'], {'ported': True, 'commands': [STOCK], 'files': {
        '/run/y2/internal-media-setup.json': json.dumps({'state': 'Waiting', 'boot_id': 'boot-a', 'serial': SERIAL,
                                                         'files': 900, 'bytes': 10})}}),
    ('internal-setup-confirmed-earlier-boot', ['internal-media', 'setup', '--confirm-shown'], {
        'ported': True, 'commands': [STOCK], 'files': {'/run/y2/internal-media-setup.json': json.dumps(
            {'state': 'Waiting', 'boot_id': 'boot-z', 'serial': SERIAL})}}),
    ('internal-setup-serial', ['internal-media', 'setup', '--confirm-serial', SERIAL], {'ported': True, 'commands': [
        STOCK, ok(MKFS)], 'files': {**mounts(OLD), **OLD_FILES}}),
    ('internal-setup-resume', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, SETUP: json.dumps(RECORD), '/data/system/internal-media-kept/Notes/b.txt': 'bbbbb',
        '/data/system/internal-media-kept/a.mp3': 'aaaa', '/run/reborn-splash/.keep': ''}}),
    ('internal-setup-resume-mkfs', ['internal-media', 'setup'], {'ported': True, 'commands': [STOCK, ok(MKFS)], 'files': {
        SETUP: json.dumps(RECORD)}}),
    ('internal-setup-resume-changed', ['internal-media', 'setup'], {'ported': True, 'commands': [
        ok(P8_BLKID, 'TYPE=vfat\nUUID=0000-0000')], 'files': {SETUP: json.dumps(RECORD)}}),
    ('internal-setup-resume-staging', ['internal-media', 'setup', '--automatic'], {'ported': True, 'commands': [STOCK], 'files': {
        **ON_BATTERY, SETUP: json.dumps({**RECORD, 'phase': 'staging'})}}),
    ('internal-setup-resume-staging-charging', ['internal-media', 'setup'], {'ported': True, 'commands': [
        STOCK, ok(['umount', '/run/y2/old-storage'])], 'files': {
        **mounts(OLD), **OLD_FILES, SETUP: json.dumps({**RECORD, 'phase': 'staging'})}}),
    ('internal-setup-record-unreadable', ['internal-media', 'setup'], {'ported': True, 'commands': [STOCK], 'files': {
        SETUP: '[1, 2]'}}),
    ('internal-setup-record-mismatch', ['internal-media', 'setup'], {'ported': True, 'commands': [STOCK], 'files': {
        SETUP: json.dumps({**RECORD, 'start': 0})}}),
    ('internal-setup-record-phase', ['internal-media', 'setup'], {'ported': True, 'commands': [STOCK], 'files': {
        SETUP: json.dumps({**RECORD, 'phase': 'done'})}}),
    ('internal-help', ['internal-media', '--help'], {'ported': True}),
    ('internal-serial-missing', ['internal-media', 'setup', '--confirm-serial'], {'ported': True}),
]

DATA_READY = {'/proc/self/mountinfo': BASE_MOUNTS.replace('179:9', '179:8')}
DATA_UUID = ok(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], '79324c69-6e75-4801-8000-000000000102')
SPACE = [
    ('space', ['space'], {'ported': True}),
    ('space-cleanup', ['space', '--cleanup'], {'ported': True}),
    ('space-ready', ['space'], {'ported': True, 'files': DATA_READY, 'commands': [DATA_UUID]}),
    ('space-ready-cleanup', ['space', '--cleanup', '--cleanup'], {'ported': True, 'files': DATA_READY,
                                                                  'commands': [DATA_UUID]}),
    ('space-wrong-uuid', ['space'], {'ported': True, 'files': DATA_READY}),
    ('space-read-only', ['space'], {'ported': True, 'commands': [DATA_UUID], 'files': {
        '/proc/self/mountinfo': DATA_READY['/proc/self/mountinfo'].replace('/data rw', '/data ro')}}),
    ('space-no-data', ['space'], {'ported': True, 'files': {'/proc/self/mountinfo': BASE_MOUNTS.splitlines()[0]}}),
    ('space-help', ['space', '--help'], {'ported': True}),
    ('space-extra', ['space', 'now'], {'ported': True}),
]

CASES = MEDIA + INTERNAL_CASES + SPACE


def fixture(root):
    """Add the files these cases need below `root` (a pathlib.Path): one inserted, unmounted SD card and the
    internal Y2Media partition at the stock geometry."""
    def put(name, content):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def link(name, target):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(target)

    put(BLOCK + '/cid', CID + '\n')
    put(DISK + '/dev', '179:96\n')
    put(DISK + '/size', '62521344\n')
    put(DISK + '/diskseq', '9\n')
    link(DISK + '/device', '../../../mmc1:aaaa')
    put(PART + '/dev', '179:97\n')
    put(PART + '/partition', '1\n')
    put(PART + '/start', '8192\n')
    put(PART + '/size', '62513152\n')
    put(EMMC + '/dev', '179:0\n')
    put(EMMC + '/diskseq', '1\n')
    put(P8 + '/dev', '179:8\n')
    put(P8 + '/partition', '8\n')
    put(P8 + '/start', '3742720\n')
    put(P8 + '/size', '11460608\n')
    up = '../../devices/' + BLOCK[len('sys/devices/'):]
    link('sys/class/block/mmcblk1', up + '/block/mmcblk1')
    link('sys/class/block/mmcblk1p1', up + '/block/mmcblk1/mmcblk1p1')
    link('sys/dev/block/179:96', up + '/block/mmcblk1')
    link('sys/dev/block/179:97', up + '/block/mmcblk1/mmcblk1p1')
    emmc = '../../devices/' + EMMC[len('sys/devices/'):]
    link('sys/class/block/mmcblk0p8', emmc + '/mmcblk0p8')
    link('sys/dev/block/179:8', emmc + '/mmcblk0p8')
