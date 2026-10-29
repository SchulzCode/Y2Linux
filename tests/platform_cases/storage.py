"""Golden cases of port group `storage` (see tests/platform_cases/__init__.py): `media` (every action),
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
    ('media-status', ['media', 'status'], {}),
    ('media-status-json', ['media', 'status', '--json'], {}),
    ('media-status-refused', ['media', 'status'], {'files': {'/run/y2/media.json': REFUSED}}),
    ('media-status-protected', ['media', 'status'], {'files': MOUNTED, 'commands': SD_STATUS}),
    ('media-status-errors', ['media', 'status'], {'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('filesystem_errors')}}),
    ('media-status-rw-undersized', ['media', 'status'], {'commands': SD_STATUS, 'files': {
        **MOUNTED, **mounts(SD_RW), PART + '/size': '99999999999\n'}}),
    ('media-status-no-claim', ['media', 'status'], {'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': None}}),
    ('media-status-other-card', ['media', 'status'], {'commands': SD_STATUS, 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim(identity={'id': 'x'})}}),
    ('media-status-no-mountpoint', ['media', 'status'], {'commands': SD_STATUS, 'files': {
        **MOUNTED, '/media/sd/.keep': None}}),
    ('media-mount', ['media', 'mount'], {'commands': [VFAT, ok(FSCK), ok(MOUNT_RO)]}),
    ('media-mount-dirty', ['media', 'mount'], {'commands': [VFAT, failed(FSCK, DIRTY)]}),
    ('media-mount-errors', ['media', 'mount'], {'commands': [VFAT, failed(FSCK, DAMAGED)]}),
    ('media-mount-timeout', ['media', 'mount'], {'commands': [
        EXFAT, failed(['fsck.exfat', '-n', '/dev/mmcblk1p1'], reason='command_timeout')]}),
    ('media-mount-exfat-errors', ['media', 'mount'], {'commands': [
        EXFAT, failed(['fsck.exfat', '-n', '/dev/mmcblk1p1'])]}),
    ('media-mount-ext4-refused', ['media', 'mount'], {'commands': [EXT4]}),
    ('media-mount-ext4', ['media', 'mount'], {'commands': [
        EXT4, ok(['e2fsck', '-n', '/dev/mmcblk1p1'])]}),
    ('media-mount-always-writable', ['media', 'mount'], {'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-always-writable': ''}}),
    ('media-mount-history', ['media', 'mount'], {'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-admissions.json': json.dumps({'schema': 1, 'admissions': [
            {'time': 1, 'card': 'old%d' % i, 'ok': True} for i in range(10)] + ['not a dict']})}}),
    ('media-mount-history-null', ['media', 'mount'], {'commands': [VFAT, ok(FSCK)], 'files': {
        '/data/system/platform/sd-admissions.json': '{"admissions": null}'}}),
    ('media-mount-no-card', ['media', 'mount'], {'files': {
        '/sys/class/block/mmcblk1': None, '/sys/class/block/mmcblk1p1': None}}),
    ('media-mount-no-identity', ['media', 'mount'], {'commands': [ok(BLKID, 'TYPE=vfat')],
                                                     'files': {BLOCK + '/cid': None}}),
    ('media-mount-whole-disk', ['media', 'mount'], {'commands': [
        ok(['blkid', '-p', '-o', 'export', '/dev/mmcblk1'], 'TYPE=exfat\nUUID=AB')], 'files': {
        '/sys/class/block/mmcblk1p1': None, PART + '/partition': None}}),
    ('media-mount-already', ['media', 'mount'], {'files': MOUNTED}),
    ('media-mount-foreign', ['media', 'mount'], {'files': mounts(SD_RO.replace('179:97', '8:1'))}),
    ('media-mount-file-in-the-way', ['media', 'mount'], {'commands': [VFAT, ok(FSCK)],
                                                         'files': {'/media/sd': 'not a directory'}}),
    ('media-unmount-not-mounted', ['media', 'unmount'], {}),
    ('media-unmount-busy', ['media', 'unmount'], {'files': MOUNTED,
                                                  'commands': [ok(['umount', '/media/sd'])]}),
    ('media-unmount-fails', ['media', 'unmount'], {'files': MOUNTED}),
    ('media-unmount-stacked', ['media', 'unmount'], {'files': mounts(SD_RO, SD_RO.replace('51', '57', 1))}),
    ('media-unmount-usb', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'attached',
         'views': [{'path': '/media/internal/music'}, {'path': '/media/sd'}]})}}),
    # Views Python cannot read: AttributeError (no dict) or TypeError (not iterable), a traceback.
    ('media-unmount-usb-text-views', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': '/media/sd'})}}),
    ('media-unmount-usb-dict-views', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': {'path': '/media/sd'}})}}),
    ('media-unmount-usb-number-view', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': [{'path': '/x'}, 3]})}}),
    ('media-unmount-usb-null-views', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': None})}}),
    ('media-unmount-usb-number-views', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': 7})}}),
    ('media-unmount-usb-empty-text-views', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': ''})}}),
    ('media-unmount-usb-view-first', ['media', 'unmount'], {'files': {**MOUNTED, '/run/y2/usb-transfer.json': json.dumps(
        {'schema': 'org.y2linux.usb-transfer/v1', 'boot_id': 'boot-a', 'cable': 'not attached', 'views': [{'path': '/media/sd'}, 3]})}}),
    ('media-check', ['media', 'check'], {'commands': [VFAT, ok(FSCK, 'clean')]}),
    ('media-check-ext4', ['media', 'check'], {'commands': [
        EXT4, failed(['e2fsck', '-f', '-n', '/dev/mmcblk1p1'])]}),
    ('media-check-mounted', ['media', 'check'], {'files': MOUNTED}),
    ('media-reconcile', ['media', 'reconcile'], {'commands': [VFAT, ok(FSCK)]}),
    ('media-reconcile-no-card', ['media', 'reconcile'], {'files': {
        '/sys/class/block/mmcblk1': None, '/sys/class/block/mmcblk1p1': None,
        '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'attempts': True})}}),
    ('media-reconcile-same', ['media', 'reconcile'], {'files': {
        '/run/y2/media-lifecycle.json': json.dumps({'schema': 1, 'inventory': LIFECYCLE, 'attempts': 1,
                                                    'retry': False, 'state': 'Ready', 'reason': None})}}),
    ('media-reconcile-retry', ['media', 'reconcile'], {'files': {
        **MOUNTED, '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'attempts': 2, 'retry': True})}}),
    ('media-reconcile-protect', ['media', 'reconcile'], {'files': {
        **MOUNTED, **mounts(SD_RW), '/run/y2/media-lifecycle.json': json.dumps({'inventory': LIFECYCLE, 'state': 'Ready'})},
        'commands': [ok(['mount', '-o', 'remount,ro', '/media/sd'])]}),
    ('media-verify-nothing', ['media', 'verify'], {}),
    ('media-verify', ['media', 'verify'], {'commands': [VFAT, ok(FSCK)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-writable', ['media', 'verify'], {'commands': [VFAT, ok(FSCK)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking'), '/data/system/platform/sd-always-writable': ''}}),
    ('media-verify-errors', ['media', 'verify'], {'commands': [VFAT, failed(FSCK, DAMAGED)], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-timeout', ['media', 'verify'], {'commands': [
        VFAT, failed(FSCK, reason='command_timeout')], 'files': {**MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-verify-other-card', ['media', 'verify'], {'commands': [VFAT], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking', source_instance='block:179:97:diskseq:3')}}),
    # A claim Python cannot read crashes it (AttributeError: traceback, nothing on stdout, exit 1).
    ('media-verify-bad-identity', ['media', 'verify'], {'commands': [VFAT], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking', identity=['x'])}}),
    ('media-verify-text-identity', ['media', 'verify'], {'commands': [VFAT], 'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking', identity='x')}}),
    ('media-format-unconfirmed', ['media', 'format'], {}),
    ('media-format-wrong-card', ['media', 'format', '--confirm-cid', 'other'], {}),
    ('media-format', ['media', 'format', '--confirm-cid', CID], {'files': SMALL}),
    ('media-format-not-writable', ['media', '--confirm-cid', CID, 'format'], {'files': {
        DISK + '/size': '20000\n'}}),
    ('media-format-too-small', ['media', 'format', '--confirm-cid', CID], {'files': {
        DISK + '/size': '100\n'}}),
    ('media-format-no-card', ['media', 'format', '--confirm-cid', CID], {'files': {
        '/sys/class/block/mmcblk1': None}}),
    ('media-format-mounted', ['media', 'format', '--confirm-cid', CID], {'files': {**MOUNTED, **SMALL},
                                                                          'commands': [ok(['umount', '/media/sd'])]}),
    ('media-grow-not-ext4', ['media', 'grow'], {'commands': [VFAT]}),
    ('media-grow-check-fails', ['media', 'grow'], {'commands': [EXT4]}),
    ('media-grow-resize-fails', ['media', 'grow'], {'commands': [
        EXT4, ok(['e2fsck', '-f', '-p', '/dev/mmcblk1p1'])]}),
    ('media-grow', ['media', 'grow'], {'commands': [
        EXT4, ok(['e2fsck', '-f', '-p', '/dev/mmcblk1p1']), ok(['resize2fs', '/dev/mmcblk1p1']),
        ok(['e2fsck', '-n', '/dev/mmcblk1p1'])]}),
    ('media-repair-not-marked', ['media', 'repair'], {}),
    ('media-repair-on-battery', ['media', 'repair'], {'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER.replace('64', '12')}}),
    ('media-repair', ['media', 'repair'], {'commands': [
        EXT4, ok(['e2fsck', '-p', '/dev/mmcblk1p1'], 'Truncating file to 0 bytes'), ok(['e2fsck', '-n', '/dev/mmcblk1p1']),
        ok(['mount', '-t', 'ext4', '-o', 'ro,nosuid,nodev,noexec,noatime', '/dev/mmcblk1p1', '/media/sd'])], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER, '/dev/mmcblk1p1': CARD}}),
    ('media-repair-vfat', ['media', 'repair'], {'commands': [
        VFAT, failed(REPAIR, 'Reclaimed 3 unused clusters (98304 bytes).\n/A/b.mp3\n  Truncating file to 0 bytes'),
        failed(FSCK, DIRTY)], 'files': {
        '/run/y2/media.json': REFUSED.replace('"ext4"', '"vfat"').replace(identity('ext4')['id'], identity()['id']),
        '/run/y2/power.json': POWER.replace('false', 'true', 1), '/dev/mmcblk1p1': CARD,
        '/data/system/platform/sd-repair/old.img': 'old'}}),
    ('media-repair-timeout', ['media', 'repair'], {'commands': [
        EXT4, failed(['e2fsck', '-p', '/dev/mmcblk1p1'], reason='command_timeout')], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER}}),
    ('media-repair-short-device', ['media', 'repair'], {'commands': [VFAT], 'files': {
        '/run/y2/media.json': REFUSED.replace('"ext4"', '"vfat"').replace(identity('ext4')['id'], identity()['id']),
        '/run/y2/power.json': POWER, '/dev/mmcblk1p1': BOOT}}),
    ('media-repair-mounted', ['media', 'repair'], {'commands': [ok(['umount', '/media/sd'])],
                                                   'files': {**MOUNTED, '/run/y2/power.json': POWER,
                                                             '/run/y2/media-mount.json': claim('check_timed_out')}}),
    ('media-repair-changed-card', ['media', 'repair'], {'commands': [VFAT], 'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER}}),
    # A battery, identity or check that is no dict crashes Python: before the card is detached
    # (battery) or after it (identity, check), with no repair record written.
    ('media-repair-bad-battery', ['media', 'repair'], {'files': {
        '/run/y2/media.json': REFUSED, '/run/y2/power.json': POWER.replace('{"charging"', '"x", "y": {"charging"')}}),
    ('media-repair-bad-identity', ['media', 'repair'], {'commands': [EXT4], 'files': {
        '/run/y2/media.json': REFUSED.replace('"identity": {', '"identity": ["x"], "old": {'),
        '/run/y2/power.json': POWER}}),
    ('media-repair-bad-check', ['media', 'repair'], {'commands': [EXT4], 'files': {
        '/run/y2/media.json': json.dumps({**json.loads(REFUSED), 'filesystem_check': [1]}),
        '/run/y2/power.json': POWER}}),
    ('media-write-not-mounted', ['media', 'write'], {}),
    ('media-write-bad-holders', ['media', 'write'], {'files': {
        **MOUNTED, '/run/y2/sd-write.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 51, 'holders': [1]})}}),
    # Seconds beyond 64 bits: the parser refuses them (exit 2) before anything is remounted.
    ('media-write-huge-seconds', ['media', 'write', '--seconds', '99999999999999999999999'], {
        'files': MOUNTED, 'volatile': WRITE_VOLATILE,
        'commands': [ok(['mount', '-o', 'remount,rw', '/media/sd'])]}),
    ('media-write-huge-negative-seconds', ['media', 'write', '--seconds', '-99999999999999999999999'], {
        'files': MOUNTED, 'volatile': WRITE_VOLATILE,
        'commands': [ok(['mount', '-o', 'remount,rw', '/media/sd'])]}),
    ('media-write', ['media', 'write'], {'files': MOUNTED, 'volatile': WRITE_VOLATILE,
                                         'commands': [ok(['mount', '-o', 'remount,rw', '/media/sd'])]}),
    ('media-write-long', ['media', 'write', '--holder', 'radiofw', '--seconds', '5000', '--holder', 'usb'], {
        'files': MOUNTED, 'volatile': WRITE_VOLATILE}),
    ('media-write-release', ['media', 'write', '--holder', 'usb', '--release'], {'files': {
        **MOUNTED, '/run/y2/sd-write.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 51, 'holders': {
            'usb': 1e12, 'reborn': 0, 'stranger': 1e12}})}, 'volatile': WRITE_VOLATILE}),
    ('media-write-held', ['media', 'write', '--holder', 'report', '--seconds', '0'], {'files': {
        **MOUNTED, **mounts(SD_RW), '/run/y2/sd-write.json': json.dumps({'boot_id': 'boot-a', 'mount_id': 51.0, 'holders': {
            'usb': 1e12}})}, 'volatile': WRITE_VOLATILE}),
    ('media-write-checking', ['media', 'write'], {'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('checking')}}),
    ('media-write-errors', ['media', 'write'], {'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim('filesystem_errors')}}),
    ('media-write-unprotected', ['media', 'write'], {'files': {
        **MOUNTED, '/run/y2/media-mount.json': claim(None)}}),
    ('media-help', ['media', '--help'], {}),
    ('media-no-action', ['media'], {}),
    ('media-bad-action', ['media', 'eject'], {}),
    ('media-bad-holder', ['media', 'write', '--holder', 'nobody'], {}),
    ('media-seconds-joined', ['media', 'write', '--seconds=5'], {}),
    ('media-seconds-negative', ['media', 'write', '--seconds', '-5'], {}),
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
    ('internal-status', ['internal-media', 'status'], {}),
    ('internal-status-ready', ['internal-media', 'status'], {'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, '/run/y2/internal-media.json': json.dumps(
            {'state': 'Ready', 'boot_id': 'boot-a', 'mount_id': 52, 'device_id': '179:8'})}}),
    ('internal-status-no-partition', ['internal-media', 'status'], {'files': {
        P8 + '/start': '0\n'}}),
    ('internal-mount-stock', ['internal-media', 'mount'], {'commands': [STOCK]}),
    ('internal-mount-ready', ['internal-media', 'mount'], {'commands': [VALID], 'files': MOUNTED_INTERNAL}),
    ('internal-mount-foreign', ['internal-media', 'mount'], {'commands': [VALID], 'files': mounts(
        INTERNAL.replace('/dev/mmcblk0p8', '/dev/sda1'))}),
    ('internal-mount-check-fails', ['internal-media', 'mount'], {'commands': [VALID]}),
    ('internal-mount-not-seen', ['internal-media', 'mount'], {'commands': [
        VALID, ok(['e2fsck', '-p', '/dev/mmcblk0p8']),
        ok(['mount', '-t', 'ext4', '-o', 'rw,nosuid,nodev,noexec,noatime,noinit_itable', '/dev/mmcblk0p8',
            '/media/internal'])]}),
    ('internal-mount-no-partition', ['internal-media', 'mount'], {'files': {
        '/sys/class/block/mmcblk0p8': None}}),
    ('internal-unmount-not-mounted', ['internal-media', 'unmount'], {}),
    ('internal-unmount-busy', ['internal-media', 'unmount'], {'files': MOUNTED_INTERNAL,
                                                              'commands': [ok(['umount', '/media/internal'])]}),
    ('internal-unmount-fails', ['internal-media', 'unmount'], {'files': MOUNTED_INTERNAL}),
    ('internal-provision-unconfirmed', ['internal-media', 'provision'], {'commands': [STOCK]}),
    ('internal-provision-wrong-serial', ['internal-media', 'provision', '--confirm-serial', 'X'], {
        'commands': [STOCK]}),
    ('internal-provision-mkfs-fails', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'commands': [STOCK]}),
    ('internal-provision-in-use', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'commands': [STOCK], 'files': mounts(OLD)}),
    ('internal-provision-migrate', ['internal-media', 'provision'], {'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC}}),
    ('internal-provision-no-migrate', ['internal-media', 'provision', '--no-migrate'], {
        'commands': [VALID], 'files': {**MOUNTED_INTERNAL, **MUSIC}}),
    ('internal-provision-present', ['internal-media', 'provision'], {'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, '/media/internal/music/02.flac': 'two two'}}),
    ('internal-provision-differs', ['internal-media', 'provision'], {'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, '/media/internal/music/02.flac': 'other'}}),
    ('internal-provision-mkfs', ['internal-media', 'provision', '--confirm-serial', SERIAL], {
        'commands': [STOCK, ok(MKFS)]}),
    ('internal-setup-not-needed', ['internal-media', 'setup'], {'commands': [VALID]}),
    ('internal-setup-not-stock', ['internal-media', 'setup'], {'commands': [
        ok(P8_BLKID, 'TYPE=ext4\nLABEL=OTHER')]}),
    ('internal-setup-unconfirmed', ['internal-media', 'setup'], {'commands': [STOCK]}),
    ('internal-setup-no-partition', ['internal-media', 'setup', '--automatic'], {'files': {
        P8 + '/size': '1\n'}}),
    ('internal-setup-mounted', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK],
                                                                            'files': mounts(INTERNAL)}),
    ('internal-setup-waiting', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK],
                                                                            'files': {'/run/reborn-splash/.keep': ''}}),
    ('internal-setup-update', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK], 'files': {
        '/data/updates/state.json': '{"schema": 2.0, "state": "PendingHealth"}'}}),
    ('internal-setup-battery-unknown', ['internal-media', 'setup', '--automatic'], {
        'commands': [STOCK], 'files': ON_BATTERY}),
    ('internal-setup-battery-low', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK], 'files': {
        **ON_BATTERY, '/etc/y2linux/battery-profile.json': PROFILE, '/sys/class/power_supply/BAT0/voltage_ocv': '3500000\n'}}),
    ('internal-setup-battery-enough', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK], 'files': {
        **ON_BATTERY, '/etc/y2linux/battery-profile.json': PROFILE, '/sys/class/power_supply/BAT0/voltage_ocv': 'x\n',
        '/sys/class/power_supply/BAT0/voltage_now': '3900000\n'}}),
    ('internal-setup-keeps-files', ['internal-media', 'setup', '--automatic'], {'commands': [
        STOCK, ok(['umount', '/run/y2/old-storage'])], 'files': {**mounts(OLD), **OLD_FILES, '/run/reborn-splash/.keep': ''}}),
    ('internal-setup-nothing-kept', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK],
                                                                                 'files': mounts(OLD)}),
    ('internal-setup-confirmed', ['internal-media', 'setup', '--confirm-shown'], {'commands': [STOCK], 'files': {
        '/run/y2/internal-media-setup.json': json.dumps({'state': 'Waiting', 'boot_id': 'boot-a', 'serial': SERIAL,
                                                         'files': 900, 'bytes': 10})}}),
    ('internal-setup-confirmed-earlier-boot', ['internal-media', 'setup', '--confirm-shown'], {
        'commands': [STOCK], 'files': {'/run/y2/internal-media-setup.json': json.dumps(
            {'state': 'Waiting', 'boot_id': 'boot-z', 'serial': SERIAL})}}),
    ('internal-setup-serial', ['internal-media', 'setup', '--confirm-serial', SERIAL], {'commands': [
        STOCK, ok(MKFS)], 'files': {**mounts(OLD), **OLD_FILES}}),
    ('internal-setup-resume', ['internal-media', 'setup', '--automatic'], {'commands': [VALID], 'files': {
        **MOUNTED_INTERNAL, **MUSIC, SETUP: json.dumps(RECORD), '/data/system/internal-media-kept/Notes/b.txt': 'bbbbb',
        '/data/system/internal-media-kept/a.mp3': 'aaaa', '/run/reborn-splash/.keep': ''}}),
    ('internal-setup-resume-mkfs', ['internal-media', 'setup'], {'commands': [STOCK, ok(MKFS)], 'files': {
        SETUP: json.dumps(RECORD)}}),
    ('internal-setup-resume-changed', ['internal-media', 'setup'], {'commands': [
        ok(P8_BLKID, 'TYPE=vfat\nUUID=0000-0000')], 'files': {SETUP: json.dumps(RECORD)}}),
    ('internal-setup-resume-staging', ['internal-media', 'setup', '--automatic'], {'commands': [STOCK], 'files': {
        **ON_BATTERY, SETUP: json.dumps({**RECORD, 'phase': 'staging'})}}),
    ('internal-setup-resume-staging-charging', ['internal-media', 'setup'], {'commands': [
        STOCK, ok(['umount', '/run/y2/old-storage'])], 'files': {
        **mounts(OLD), **OLD_FILES, SETUP: json.dumps({**RECORD, 'phase': 'staging'})}}),
    ('internal-setup-record-unreadable', ['internal-media', 'setup'], {'commands': [STOCK], 'files': {
        SETUP: '[1, 2]'}}),
    ('internal-setup-record-mismatch', ['internal-media', 'setup'], {'commands': [STOCK], 'files': {
        SETUP: json.dumps({**RECORD, 'start': 0})}}),
    ('internal-setup-record-phase', ['internal-media', 'setup'], {'commands': [STOCK], 'files': {
        SETUP: json.dumps({**RECORD, 'phase': 'done'})}}),
    # A resume record without its phase (or a FAT's serial) is Python's KeyError after the 'Running' note.
    ('internal-setup-record-no-phase', ['internal-media', 'setup'], {'commands': [STOCK], 'files': {
        SETUP: json.dumps({k: v for k, v in RECORD.items() if k != 'phase'})}}),
    ('internal-setup-staging-no-serial', ['internal-media', 'setup'], {'commands': [STOCK], 'files': {
        SETUP: json.dumps({**{k: v for k, v in RECORD.items() if k != 'serial'}, 'phase': 'staging'})}}),
    ('internal-setup-staging-no-serial-ext4', ['internal-media', 'setup'], {'commands': [VALID],
                                                                             'files': {
        SETUP: json.dumps({**{k: v for k, v in RECORD.items() if k != 'serial'}, 'phase': 'staging'})}}),
    ('internal-setup-formatting-no-serial', ['internal-media', 'setup'], {'commands': [STOCK],
                                                                           'files': {
        SETUP: json.dumps({k: v for k, v in RECORD.items() if k != 'serial'})}}),
    ('internal-help', ['internal-media', '--help'], {}),
    ('internal-serial-missing', ['internal-media', 'setup', '--confirm-serial'], {}),
]

DATA_READY = {'/proc/self/mountinfo': BASE_MOUNTS.replace('179:9', '179:8')}
DATA_UUID = ok(['blkid', '-p', '-s', 'UUID', '-o', 'value', '/dev/mmcblk0p9'], '79324c69-6e75-4801-8000-000000000102')
SPACE = [
    ('space', ['space'], {}),
    ('space-cleanup', ['space', '--cleanup'], {}),
    ('space-ready', ['space'], {'files': DATA_READY, 'commands': [DATA_UUID]}),
    ('space-ready-cleanup', ['space', '--cleanup', '--cleanup'], {'files': DATA_READY,
                                                                  'commands': [DATA_UUID]}),
    ('space-wrong-uuid', ['space'], {'files': DATA_READY}),
    ('space-read-only', ['space'], {'commands': [DATA_UUID], 'files': {
        '/proc/self/mountinfo': DATA_READY['/proc/self/mountinfo'].replace('/data rw', '/data ro')}}),
    ('space-no-data', ['space'], {'files': {'/proc/self/mountinfo': BASE_MOUNTS.splitlines()[0]}}),
    ('space-help', ['space', '--help'], {}),
    ('space-extra', ['space', 'now'], {}),
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
