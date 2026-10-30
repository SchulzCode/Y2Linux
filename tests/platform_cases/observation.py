"""Golden cases of port group `observation` (see tests/platform_cases/__init__.py): status (every section
and option), health, boot-evidence, network-check, collect, bench-storage and bench-library.

The shared fixture gains only paths the base status does not read (block devices behind /sys/dev/block/179:19..,
a Reborn process 4242); every case builds its player state with `files`, records with a clock reading are made
when the case runs."""
import json
import os
import time

BOOT = 'boot-a'
EPOCH = 'b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d'
BSSID = 'aa:bb:cc:dd:ee:ff'
# The eMMC (11230000.mmc) partitions 19-21 and the SD card (11240000.mmc), as sysfs links them.
EMMC = '/sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0'
SD = '/sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:aaaa/block/mmcblk1'
SD_WHOLE = '/sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:bbbb/block/mmcblk2'
UUIDS = {'/': '79324c69-6e75-4801-8000-000000000101', '/data': '79324c69-6e75-4801-8000-000000000102',
         '/media/internal': '79324c69-6e75-4801-8000-000000000103'}
SD_UUID = '1234-ABCD'
SD_INSTANCE = 'block:179:33:diskseq:12'
# Clock readings in measurements and records (equal behaviour, different values).
TIMING = ['p50_ms', 'p95_ms', 'p99_ms', 'max_ms', 'MB_per_second', 'operations_per_second', 'duration_ns',
          'captured_monotonic_s', 'elapsed_seconds', 'cpu_busy_percent', 'monotonic_us']


def answer(argv, output='', ok=True, reason=None):
    return {'argv': argv, 'ok': ok, 'reason': None if ok else (reason or 'command_failed'),
            'output': output if ok else None}


def blkid(source, uuid):
    return answer(['blkid', '-p', '-s', 'UUID', '-o', 'value', source], uuid)


def fresh(value, key='monotonic_s', ns=False, us=False):
    """A record stamped with the clock when the case runs."""
    def make():
        stamp = time.monotonic_ns() if ns else time.monotonic() * 1e6 if us else time.monotonic()
        return json.dumps({**value, key: int(stamp) if (ns or us) else stamp})
    return make


MOUNTS = ('22 1 179:19 / / ro - ext4 /dev/mmcblk0p19 ro\n'
          '30 22 179:20 / /data rw,noatime - ext4 /dev/mmcblk0p20 rw\n'
          '40 30 179:33 / /media/sd ro,nosuid - vfat /dev/mmcblk1p1 ro\n'
          '41 30 179:21 / /media/internal rw,noatime - ext4 /dev/mmcblk0p21 rw\n')
VOLUMES = [blkid('/dev/mmcblk0p19', UUIDS['/']), blkid('/dev/mmcblk0p20', UUIDS['/data']),
           blkid('/dev/mmcblk1p1', SD_UUID), blkid('/dev/mmcblk0p21', UUIDS['/media/internal'])]
SD_IDENTITY = {'cid': '1b534d454231515430', 'geometry': {'partition': '1', 'start': '8192', 'size': '1000'},
               'filesystem': 'vfat', 'uuid': SD_UUID}


def sd_identity(identity=SD_IDENTITY):
    import hashlib
    return {**identity, 'id': hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()}


def claim(**changes):
    return json.dumps({'boot_id': BOOT, 'mount_id': 40, 'device_id': '179:33', 'uuid': SD_UUID,
                       'source_instance': SD_INSTANCE, 'identity': sd_identity(), 'read_only': 'protected',
                       **changes})


PLAYER = {
    '/proc/self/mountinfo': MOUNTS,
    '/run/y2-data-device': '/dev/mmcblk0p20\n',
    '/media/sd/.keep': '', '/media/internal/.keep': '',
    '/run/y2/media-mount.json': claim(),
    '/run/y2/internal-media-setup.json': json.dumps({'boot_id': BOOT, 'state': 'Ready', 'reason': None,
                                                     'files': 3, 'bytes': 9, 'other': 1}),
    '/run/y2/media.json': json.dumps({'state': 'Ready', 'reason': None}),
    '/run/y2/media-lifecycle.json': json.dumps({'state': 'Mounted'}),
    '/sys/kernel/debug/mmc0/ios': 'clock: 200000000 Hz\n',
    '/sys/class/block/11230000.mmc-blk/stat': '1 2 3 4\n', '/sys/class/block/11230000.mmc-blk/size': '7634944\n',
    '/sys/class/block/11230000.mmc-blk/device/type': 'MMC\n',
    '/sys/class/block/11230000.mmc-blk/device/ioerr_cnt': '0\n',
    '/sys/class/block/other-blk/size': '5\n',
    '/sys/bus/platform/devices/11230000.mmc/y2_performance':
        'cap_hz=200000000 actual_hz=187500000 transport_errors=0 fallbacks=1 clock_error=-3 other=4\n',
    '/sys/bus/platform/devices/11230000.mmc/y2_storage': 'mode=hs200 bus_width=8 tuning=0x1f crc_events=0 a=-2\n',
    '/sys/bus/platform/devices/11230000.mmc/y2_runtime_pm': 'enabled=1 delay_ms=200\n',
    '/sys/bus/platform/devices/11230000.mmc/power/runtime_status': 'active\n',
    '/sys/bus/platform/devices/11230000.mmc/y2_tune_diag': 'phase=3\n',
}
DATA_READY = {'/proc/self/mountinfo': MOUNTS, '/media/sd/.keep': '', '/media/internal/.keep': ''}

CPU = {
    '/proc/stat': 'cpu  100 0 50 1000 5 0 1 0 0 0\ncpu0 100 0 50 1000 5 0 1 0 0 0\ncpu1 7 x\nintr 99 1 2\nctxt 5\n'
                  'processes 77\nprocs_running 1\nprocs_blocked 0\nsoftirq 3 1\n',
    '/proc/loadavg': '0.10 0.20 0.30 1/80 999\n',
    '/sys/devices/system/cpu/online': '0-1\n',
    '/sys/devices/system/cpu/cpufreq/policy0/affected_cpus': '0 1\n',
    '/sys/devices/system/cpu/cpufreq/policy0/scaling_governor': 'schedutil\n',
    '/sys/devices/system/cpu/cpufreq/policy0/scaling_max_freq': '1300000\n',
    '/sys/devices/system/cpu/cpufreq/policy0/scaling_available_frequencies': '598000 747500 1040000 1300000 1500000 x\n',
    '/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state': '598000 10\n1300000 20\n',
    '/sys/module/cpu_dvfs/parameters/qualification_max_khz': '1300000\n',
    '/sys/module/cpu_dvfs/parameters/bin_supported': '1\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/name': 'WFI\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/usage': '100\n', '/sys/devices/system/cpu/cpu0/cpuidle/state0/time': '50\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state0/rejected': '0\n', '/sys/devices/system/cpu/cpu0/cpuidle/state0/disable': '0\n',
    '/sys/devices/system/cpu/cpu1/cpuidle/state0/name': 'WFI\n',
    '/sys/devices/system/cpu/cpu1/cpuidle/state0/usage': '7\n', '/sys/devices/system/cpu/cpu1/cpuidle/state0/time': '9\n',
    '/sys/devices/system/cpu/cpu1/cpuidle/state0/rejected': '1\n', '/sys/devices/system/cpu/cpu1/cpuidle/state0/disable': '0\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state1/name': 'SLIDLE\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state1/usage': '4\n', '/sys/devices/system/cpu/cpu0/cpuidle/state1/disable': '0\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state2/usage': '2\n', '/sys/devices/system/cpu/cpu0/cpuidle/state2/time': '8\n',
    '/sys/devices/system/cpu/cpu0/cpuidle/state2/rejected': '1\n', '/sys/devices/system/cpu/cpu0/cpuidle/state2/latency': '300\n',
    '/sys/module/idle/parameters/slow_reason': 'clock\n', '/sys/module/idle/parameters/fast': '1\n',
    '/sys/module/clocks/parameters/slow_blocker_names': 'MSDC0,BTIF\n',
    '/sys/module/clocks/parameters/slow_blockers': str((1 << 12) | (1 << 14) | (1 << 20) | (1 << 22)) + '\n',
    '/sys/module/clocks/parameters/unused_handoff': 'done\n',
    '/sys/bus/platform/devices/11230000.mmc/power/runtime_status': 'suspended\n',
    '/sys/bus/platform/devices/18070000.connectivity/power/runtime_status': 'active\n',
    '/sys/devices/platform/10006000.power-controller/dormant_preflight':
        'prerequisite_spm=1 prerequisite_cirq=0 unmet=,cirq,timer, peri_blockers=0x300c01 infra_blockers=0xa0 '
        'disp0=0b11 disp1=9 bad=x\n',
    '/sys/devices/platform/10006000.power-controller/state': 'pc=3 uart_sleep_count=4 uart_power_on=1 other=2\n',
    '/sys/module/spm/parameters/dormant_budget': '12\n',
    '/sys/module/mm_clocks/parameters/display_gate': 'closed\n',
    '/run/y2/cpu-idle-policy.json': json.dumps({'state': 'Enabled', 'boot_id': BOOT}),
    '/sys/firmware/y2_pm/state': 'idle\n', '/sys/firmware/y2_pm/devices': 'cb=1\n',
    '/sys/bus/platform/drivers/y2-usb/11200000.usb/status':
        'transfer=dma dma_tx=10 dma_rx=x pio_rx=2 irq_count=99 detached=1 reconnect_retries=2 runtime_held=0\n',
    '/sys/bus/platform/drivers/y2-usb/11200000.usb/rxdiag': 'rx_bytes=100 short=3 bad=-1\nsecond=9\n',
    '/sys/module/workload/parameters/leases': 'none\n',
    '/sys/module/local_timer/parameters/admission': 'on\n',
    '/sys/devices/system/clocksource/clocksource0/current_clocksource': 'arch_sys_counter\n',
    '/sys/devices/system/clockevents/clockevent0/current_device': 'arch_sys_timer\n',
    '/sys/module/system_idle/parameters/mode': '2\n',
    '/proc/timer_list': 'Timer List Version: v0.9\ncpu: 0\n clock 0:\n  .hres_active    : 1\n  .nohz : 1\n'
                        '  .tick_stopped   : 0\ncpu: 1\n  .hres_active    : 1\n  .nohz : 0\ncpu: x\n',
}
MEMORY = {'/proc/vmstat': 'nr_free_pages 100\npgfault 7 8\nempty\n', '/proc/pressure/memory': 'some avg10=0.00\n'}
THERMAL = {
    '/sys/class/thermal/thermal_zone0/type': 'cpu\n', '/sys/class/thermal/thermal_zone0/temp': '41000\n',
    '/sys/class/thermal/thermal_zone0/trip_point_0_temp': '85000\n',
    '/sys/class/thermal/thermal_zone0/trip_point_0_type': 'passive\n',
    '/sys/class/thermal/thermal_zone0/trip_point_1_temp': '95000\n',
    '/sys/class/thermal/thermal_zone1/type': 'pmic\n',
    '/sys/class/thermal/cooling_device0/type': 'cpufreq\n', '/sys/class/thermal/cooling_device0/cur_state': '0\n',
    '/sys/class/thermal/cooling_device0/max_state': '3\n',
}
POWER = {'/run/y2/power.json': fresh({'boot_id': BOOT, 'state': 'Normal', 'battery': {
    'current_ua': -120000, 'percent': 81, 'source': 'ui_soc', 'confidence': 'high', 'temperature_millicelsius': 30000}},
    'monotonic_ns', ns=True), '/sys/firmware/y2_boot/normal_boot': '1\n'}
SYSTEM = {
    '/run/y2/ssh.json': fresh({'boot_id': BOOT, 'state': 'Listening'}),
    '/run/y2/boot-stages.jsonl': '{"stage": "platform_start", "monotonic_s": 8.7}\nnot json\n[1]\n'
                                 '{"stage": "services_started", "monotonic_s": 23.7}\n',
    '/sys/class/udc/11200000.usb/state': 'configured\n', '/sys/class/udc/11200000.usb/current_speed': 'high-speed\n',
    '/sys/class/rtc/rtc0/name': 'mt6397-rtc\n', '/sys/class/rtc/rtc0/since_epoch': '1788000000\n',
    '/sys/class/watchdog/watchdog0/identity': 'mtk-wdt\n', '/sys/class/watchdog/watchdog0/timeout': '31\n',
    '/sys/fs/pstore/dmesg-ramoops-0': 'x', '/sys/fs/pstore/.hidden': 'y',
    '/proc/sys/kernel/random/entropy_avail': '256\n',
    '/run/y2/usb-transfer.json': json.dumps({'state': 'Idle'}),
    '/sys/class/net/usb0/operstate': 'up\n', '/sys/class/net/usb0/address': '02:00:00:00:00:01\n',
    '/sys/class/power_supply/BAT0/charging_state': 'stock_request_ua=500000 class_ceiling_ua=-1 thermal_level=2 x=1\n',
    '/sys/class/power_supply/BAT0/constant_charge_current': '450000\n',
    '/sys/class/power_supply/y2-usb-presence/usb_type': 'Unknown [SDP] DCP CDP\n',
    '/sys/class/power_supply/y2-usb-input/current_max': '500000\n',
    '/sys/kernel/config/usb_gadget/y2/configs/c.1/strings': 'not a link',
    '/run/y2/mtp-roots.json': json.dumps({'roots': ['/data/music']}),
    '/run/y2/application-ready.json': json.dumps({'boot_id': BOOT, 'state': 'Ready'}),
    '/data/reborn/logs/supervisor-last.json': json.dumps({'exit': 0}),
    '/data/system/platform/previous-boot-evidence.json': json.dumps({
        'captured_on_boot_id': 'boot-z', 'previous_boot_id': 'boot-y', 'reset_cause': None, 'retained': {'files': []},
        'pmic': {'state': 'Ready'}, 'logs': [{'name': 'system.log', 'tail_sha256': 'ab', 'tail_lines': ['x']}]}),
    '/data/updates/state.json': json.dumps({'schema': 2.0, 'state': 'Queued', 'failure': None}),
    '/data/updates/download.json': json.dumps({'state': 'Staging'}),
    '/data/updates/.stage.lock': '',
    '/proc/locks': '1: FLOCK  ADVISORY  WRITE 999 b3:09:1 0 EOF\nbad line\n',
    '/data/updates/last-check.json': json.dumps({'result': 'current'}),
    '/etc/y2linux/update-channel.json': json.dumps({'name': 'stable', 'manifest_url': 'https://x'}),
    '/data/updates/channel.json': json.dumps({'name': 'beta', 'other': 1}),
    '/run/y2/mtp-events.json': json.dumps({'error': 28.0}),
    '/run/y2/internal-media.json': json.dumps({'state': 'Ready', 'boot_id': BOOT, 'mount_id': 41,
                                               'device_id': '179:21'}),
}
AUDIO = {
    '/proc/asound/cards': ' 0 [Y2Audio        ]: Y2Audio - Y2Audio\n                      Y2Audio\n',
    '/proc/asound/pcm': '00-00: playback 1\n',
    '/etc/y2linux/audio-qualified.json': json.dumps({'schema': 'org.y2linux.audio-qualification/v1', 'card': 'Y2Audio',
                                                     'channels': 2.0, 'qualified_formats': ['S16_LE'],
                                                     'qualified_rates': [48000, 44100]}),
    '/etc/y2linux/audio-enabled.json': json.dumps({'profile': 'default'}),
    '/proc/asound/card0/pcm0p/sub0/hw_params': 'access: RW_INTERLEAVED\nrate: 44100\n',
}
WPA = ['wpa_cli', '-i', 'wlan0', 'status']
IP = ['ip', '-j', '-4', 'addr', 'show', 'dev', 'wlan0']
ROUTE = ['ip', '-j', '-4', 'route', 'show', 'default', 'dev', 'wlan0']
POLL = ['wpa_cli', '-i', 'wlan0', 'signal_poll']
ADDRESS = json.dumps([{'ifname': 'wlan0', 'addr_info': [
    {'family': 'inet', 'local': '192.168.1.40', 'prefixlen': 24, 'scope': 'global'},
    {'local': '169.254.1.1', 'scope': 'link'}]}])
ROUTES = json.dumps([{'dst': 'default', 'gateway': '192.168.1.1', 'dev': 'wlan0'}])
RADIO = {'/sys/class/net/wlan0/statistics/rx_bytes': '1000\n', '/sys/class/net/wlan0/statistics/tx_bytes': 'x\n',
         '/run/y2/wifi-power.json': json.dumps({'enabled': True}), '/run/y2/coexistence.json': json.dumps({'bt': 1})}
DNS = fresh({'ok': True, 'boot_id': BOOT, 'epoch': EPOCH, 'address': '192.168.1.40', 'bssid': BSSID})
ONLINE = [answer(WPA, 'bssid=%s\nssid=home\nwpa_state=COMPLETED\nip_address=192.168.1.40' % BSSID),
          answer(IP, ADDRESS), answer(ROUTE, ROUTES), answer(POLL, 'RSSI=-55\nLINKSPEED=65')]

OBSERVE = ['/usr/sbin/y2-bt-observe']
PCM = {'Device': '/org/bluez/hci0/dev_ZZ', 'Transport': 'A2DP-source', 'Mode': 'sink', 'Format': 0x8210,
       'Rate': 44100, 'Channels': 2, 'Codec': 'SBC', 'Sequence': 4, 'Running': True,
       'CodecConfiguration': [0x24, 0x15, 2, 47],
       'EncoderStats': {'Active': 1, 'Bitpool': 47, 'AbrEnabled': 1, 'AbrAdjustments': 3, 'BitrateKbps': 552,
                        'Errors': 0}}


def observation(pcm=None, devices=None, powered=True, adapters=1, bluealsa_manager=True, **extra):
    bluez = {}
    for n in range(adapters):
        bluez['/org/bluez/hci%d' % n] = {'org.bluez.Adapter1': {'Powered': powered, 'Discovering': False,
                                                              'Pairable': True, 'Discoverable': False}}
    # Document order, not sorted: the device list keeps it.
    for path, device in (devices if devices is not None else [
            ('/org/bluez/hci0/dev_ZZ', {'Address': 'ZZ:ZZ:00:00:00:01', 'Paired': True, 'Connected': True}),
            ('/org/bluez/hci0/dev_AA', {'Address': 'AA:BB:CC:00:00:02', 'Paired': True, 'Connected': False})]):
        bluez[path] = {'org.bluez.Device1': device}
    value = {'schema': 1, 'stable_owners': True, 'bluez_owner': ':1.2', 'bluealsa_owner': ':1.3', 'bluez': bluez,
             'bluealsa': {'/org/bluealsa/hci0/dev_ZZ/a2dpsrc/sink': {'org.bluealsa.PCM1': {**PCM, **(pcm or {})}},
                          '/org/bluealsa/other': {'org.bluealsa.PCM1': {'Device': 'x'}}},
             'manager': {'Codecs': ['A2DP-source:SBC', 'a2dp-SOURCE:AAC', 'HFP:CVSD', 5], 'Version': 'v5.0.0'},
             'codec_pcm': '/org/bluealsa/hci0/dev_ZZ/a2dpsrc/sink', 'mutually_available_codecs': {'SBC': True}}
    if not bluealsa_manager:
        del value['manager']
    value.update(extra)
    return [answer(OBSERVE, json.dumps(value))]


CODEC_LIVE = {
    '/run/y2/bluealsa.pid': '777\n', '/proc/777/cmdline': '/usr/bin/bluealsad\0-p\0a2dp-source\0--sbc-quality=high\0',
    '/run/y2/codec-runtime.json': json.dumps({'boot_id': BOOT, 'arguments': ['-p', 'a2dp-source'], 'settings': {
        'schema': 1, 'experimental': False, 'sbc_quality': 'high', 'sbc_abr': True, 'ldac_quality': 'standard',
        'ldac_abr': True, 'aac_quality': 'high', 'aac_abr': True}}),
    '/run/y2/bt-reconnect.json': fresh({'boot_id': BOOT, 'state': 'Connected'}, 'monotonic_us', us=True),
}
REBORN = ['/usr/bin/rebornctl', 'metrics', '--json']
METRICS = answer(REBORN, json.dumps({'fps': 60, 'frame_ms': 12.5, 'nested': {'b': 1, 'a': [1, 2]}}))
EVERYTHING = {**PLAYER, **CPU, **MEMORY, **THERMAL, **POWER, **SYSTEM, **AUDIO, **RADIO, '/run/y2/dns.json': DNS,
              **CODEC_LIVE}
EVERYTHING_COMMANDS = [*VOLUMES, *ONLINE, *observation(), METRICS,
                       answer(['/sbin/ip', '-4', '-o', 'addr', 'show', 'dev', 'usb0'], '5: usb0 inet 10.0.0.2/24 x')]


def status(name, argv, **options):
    return (name, ['status', *argv], {'volatile': TIMING, **options})


CASES = [
    status('status-ported', []),
    status('status-json', ['--json']),
    *[status('status-section-' + section, [section], files=EVERYTHING, commands=EVERYTHING_COMMANDS,
             volatile=TIMING) for section in ('cpu', 'memory', 'thermal', 'power', 'storage', 'wifi', 'bluetooth',
                                              'system', 'audio', 'readiness', 'usb')],
    status('status-everything', [], files=EVERYTHING, commands=EVERYTHING_COMMANDS, volatile=TIMING),
    status('status-pid', ['--pid', '4242', '--pss']),
    status('status-pid-missing', ['--pid', '4242', '--pid', '1', '--pid', '4242']),
    status('status-reborn-pss', ['--reborn', '--pss'], files={'/run/reborn/process.pid': '4242\n'},
           commands=[METRICS]),
    status('status-reborn-wrong-exe', ['--pss', '--reborn'], files={'/run/reborn/process.pid': '4243\n'}),
    status('status-reborn-no-pid', ['--reborn'], files={'/run/reborn/process.pid': '1\n'},
           commands=[answer(REBORN, '[1, 2]')]),
    status('status-reborn-dedupe', ['wifi', '--pid', '4242', '--reborn', '--json'],
           files={'/run/reborn/process.pid': '4242\n'}, commands=[answer(REBORN, 'not json')]),
    status('status-reborn-section', ['memory', '--reborn'], files={'/run/reborn/process.pid': '4242\n'},
           commands=[METRICS]),
    status('status-interval', ['--interval', '0.1'], files=CPU),
    status('status-interval-zero', ['--interval', '0']),
    status('status-invalid-pid', ['--pid', '0']),
    status('status-invalid-pid-high', ['--pid', '4194305']),
    status('status-interval-small', ['--interval', '0.05']),
    status('status-interval-nan', ['--interval', 'nan']),
    status('status-too-many-pids', [arg for n in range(33) for arg in ('--pid', str(n + 1))]),
    # storage
    status('status-storage-player', ['storage'], files=PLAYER, commands=VOLUMES),
    status('status-storage-no-blkid', ['storage'], files=PLAYER),
    status('status-storage-sd-errors', ['storage'], files={**PLAYER, '/run/y2/media-mount.json': claim(
        read_only='filesystem_errors')}, commands=VOLUMES),
    status('status-storage-sd-checking', ['storage'], files={**PLAYER, '/run/y2/media-mount.json': claim(
        read_only='checking', identity=None)}, commands=VOLUMES),
    status('status-storage-sd-claim-changed', ['storage'], files={**PLAYER, '/run/y2/media-mount.json': claim(
        mount_id=39, identity={'id': 'other'})}, commands=VOLUMES),
    status('status-storage-sd-other-identity', ['storage'], files={**PLAYER, '/run/y2/media-mount.json': claim(
        identity={'id': 'other'})}, commands=VOLUMES),
    status('status-storage-sd-undersized', ['storage'], files={**PLAYER, SD + '/mmcblk1p1/size': '99999999999\n'},
           commands=VOLUMES),
    status('status-storage-sd-whole-disk', ['storage'], files={**PLAYER, '/proc/self/mountinfo': MOUNTS.replace(
        '179:33 / /media/sd ro,nosuid - vfat /dev/mmcblk1p1 ro', '179:40 / /media/sd rw - exfat /dev/mmcblk2 rw')},
        commands=[*VOLUMES, blkid('/dev/mmcblk2', '')]),
    status('status-storage-refused-card', ['storage'], files={
        '/run/y2/media.json': json.dumps({'reason': 'filesystem_check_failed', 'source_instance': SD_INSTANCE,
                                          'filesystem': 'exfat', 'filesystem_check': {'findings': ['timeout']}})}),
    status('status-storage-refused-card-gone', ['storage'], files={
        '/run/y2/media.json': json.dumps({'reason': 'filesystem_check_failed',
                                          'source_instance': 'block:179:33:diskseq:11'})}),
    status('status-storage-stacked', ['storage'], files={'/proc/self/mountinfo': MOUNTS + MOUNTS.splitlines()[1] + '\n'},
           commands=VOLUMES),
    status('status-storage-statvfs-failed', ['storage'], files={'/proc/self/mountinfo': MOUNTS}, commands=VOLUMES),
    status('status-storage-wrong-controller', ['storage'], files={**PLAYER, '/proc/self/mountinfo': MOUNTS.replace(
        '179:33', '179:20')}, commands=VOLUMES),
    status('status-storage-setup-other-boot', ['storage'], files={
        '/run/y2/internal-media-setup.json': json.dumps({'boot_id': 'old', 'state': 'Ready'})}),
    # wifi
    status('status-wifi-online', ['wifi'], files={**RADIO, '/run/y2/dns.json': DNS}, commands=ONLINE,
           volatile=TIMING),
    status('status-wifi-dns-stale', ['wifi'], files={**RADIO, '/run/y2/dns.json': json.dumps(
        {'ok': True, 'boot_id': BOOT, 'epoch': EPOCH, 'address': '192.168.1.40', 'bssid': BSSID, 'monotonic_s': 1})},
        commands=ONLINE, volatile=TIMING),
    status('status-wifi-signal-cached', ['wifi'], files={**RADIO, '/run/y2/dns.json': DNS, '/run/y2/network.json': fresh(
        {'rssi_dbm': -61, 'signal_observation': {'boot_id': BOOT, 'bssid': BSSID}})}, commands=ONLINE,
        volatile=TIMING),
    status('status-wifi-no-route', ['wifi'], files=RADIO, commands=[*ONLINE[:2], answer(ROUTE, '[]')],
           volatile=TIMING),
    status('status-wifi-wrong-password', ['wifi'], files={**RADIO, '/run/y2/wifi-failure.json': json.dumps({
        'boot_id': BOOT, 'credential': {'boot_id': BOOT, 'error': 'wrong_key', 'network_id': 3}})},
        commands=[answer(WPA, 'wpa_state=SCANNING')], volatile=TIMING),
    status('status-wifi-failure-reason', ['wifi'], files={**RADIO, '/run/y2/wifi-failure.json': fresh({
        'boot_id': BOOT, 'error': 'assoc_reject'})}, volatile=TIMING),
    status('status-wifi-associating', ['wifi'], files=RADIO, commands=[answer(WPA, 'wpa_state=4WAY_HANDSHAKE')],
           volatile=TIMING),
    status('status-wifi-disconnected', ['wifi'], files=RADIO, commands=[answer(WPA, 'wpa_state=DISCONNECTED')],
           volatile=TIMING),
    status('status-wifi-dhcp-error', ['wifi'], files={**RADIO, '/run/y2/dhcp.json': json.dumps(
        {'boot_id': BOOT, 'epoch': EPOCH, 'error': 'dhcp_no_lease'})},
        commands=[ONLINE[0], answer(IP, '[{"addr_info": []}]')], volatile=TIMING),
    status('status-wifi-acquiring', ['wifi'], files=RADIO, commands=[ONLINE[0], answer(IP, 'null')],
           volatile=TIMING),
    status('status-wifi-missing-local', ['wifi'], files=RADIO, commands=[
        ONLINE[0], answer(IP, '[{"addr_info": [{"scope": "global"}]}]'), answer(ROUTE, ROUTES)], volatile=TIMING),
    status('status-wifi-bad-route', ['wifi'], files=RADIO, commands=[ONLINE[0], answer(IP, ADDRESS),
                                                                     answer(ROUTE, 'x')], volatile=TIMING),
    status('status-wifi-ip-object', ['wifi'], files=RADIO,
           commands=[ONLINE[0], answer(IP, '{"a": 1}')], volatile=TIMING),
    status('status-wifi-signal-list', ['wifi'], files={**RADIO, '/run/y2/network.json': '{"signal_observation": [1]}'},
           commands=ONLINE, volatile=TIMING),
    # bluetooth
    status('status-bluetooth-ready', ['bluetooth'], files=CODEC_LIVE, commands=observation()),
    status('status-bluetooth-no-stats', ['bluetooth'], commands=observation(
        pcm={'EncoderStats': None, 'CodecConfiguration': [1, 2, 3], 'Rate': 4100, 'Channels': 1.0, 'Format': 7})),
    status('status-bluetooth-aac', ['bluetooth'], commands=observation(pcm={'Codec': 'AAC', 'EncoderStats': {
        'Active': 1, 'AbrEnabled': 0, 'QualityIndex': 2, 'AbrAdjustments': 5, 'BitrateKbps': 256}})),
    status('status-bluetooth-ldac', ['bluetooth'], commands=observation(pcm={'Codec': 'LDAC', 'Rate': 96000,
        'EncoderStats': {'Active': 1, 'AbrEnabled': 1, 'QualityIndex': 1, 'AbrAdjustments': 2, 'BitrateKbps': 0}})),
    status('status-bluetooth-preferred', ['bluetooth'], files={'/data/bluetooth/preferred-audio': 'aa:bb:cc:00:00:02\n'},
           commands=observation()),
    status('status-bluetooth-two-connected', ['bluetooth'], commands=observation(devices=[
        ('/b', {'Address': 'B', 'Connected': True}), ('/a', {'Address': 'A', 'Connected': True})])),
    status('status-bluetooth-no-pcm', ['bluetooth'], commands=observation(pcm={'Mode': 'source'})),
    status('status-bluetooth-off', ['bluetooth'], commands=observation(powered=1)),
    status('status-bluetooth-two-adapters', ['bluetooth'], commands=observation(adapters=2)),
    status('status-bluetooth-starting', ['bluetooth'], commands=observation(bluealsa_manager=False)),
    status('status-bluetooth-old-version', ['bluetooth'], commands=observation(**{
        'manager': {'Codecs': ['A2DP-source:AAC'], 'Version': 5}})),
    status('status-bluetooth-unstable', ['bluetooth'], commands=observation(stable_owners=False)),
    status('status-bluetooth-schema', ['bluetooth'], commands=observation(schema=1.0)),
    status('status-bluetooth-bad-policy', ['bluetooth'], files={'/data/bluetooth/codec-policy.json': '{"schema": 2}'},
           commands=observation()),
    status('status-bluetooth-bad-quality', ['bluetooth'], files={
        '/data/bluetooth/codec-policy.json': '{"schema": 1, "sbc_quality": "max"}'}, commands=observation()),
    status('status-bluetooth-arguments-int', ['bluetooth'], files={**CODEC_LIVE, '/run/y2/codec-runtime.json': json.dumps(
        {'boot_id': BOOT, 'arguments': 5})}, commands=observation()),
    status('status-bluetooth-arguments-text', ['bluetooth'], files={**CODEC_LIVE, '/run/y2/codec-runtime.json': json.dumps(
        {'boot_id': BOOT, 'arguments': '-p', 'settings': {'x': 1}})}, commands=observation()),
    status('status-bluetooth-inventory-list', ['bluetooth'], files={
        '/etc/y2linux/bluetooth-codecs.json': '{"schema": 1, "codecs": []}'}, commands=observation()),
    status('status-bluetooth-not-json', ['bluetooth'], commands=[answer(OBSERVE, 'nope')],
           files={'/run/y2/bt-reconnect.json': '{"state": "x"}'}),
    # power, system, audio, readiness
    status('status-power-stale', ['power'], files={'/run/y2/power.json': json.dumps(
        {'boot_id': BOOT, 'monotonic_ns': 5, 'battery': {'percent': 3}})}),
    status('status-power-bool-stamp', ['power'], files={'/run/y2/power.json': json.dumps(
        {'boot_id': BOOT, 'monotonic_ns': True})}),
    status('status-power-battery-list', ['power'], files={'/run/y2/power.json': fresh(
        {'boot_id': BOOT, 'battery': [1]}, 'monotonic_ns', ns=True)}),
    status('status-power-sleep-list', ['power'], files={
        '/data/system/platform/suspend-last.json': '{"product": [["state", "idle"]]}'}),
    status('status-system-player', ['system'], files={**SYSTEM, **PLAYER}, commands=[*VOLUMES, answer(
        ['/sbin/ip', '-4', '-o', 'addr', 'show', 'dev', 'usb0'], 'no address')]),
    status('status-system-update-schema', ['system'], files={'/data/updates/state.json': '{"schema": 3}'}),
    status('status-system-update-dangling', ['system'], files={'/data/updates/state.json': 'not json'}),
    status('status-system-update-idle', ['readiness'], files={'/data/updates/.keep': ''}),
    status('status-system-update-unhashable', ['readiness'], files={
        '/data/updates/state.json': '{"schema": 1, "state": ["Idle"]}'}),
    status('status-system-firmware-ready', ['system'], files={
        '/data/firmware/mt6582/' + name: 'x' * size for name, size in (
            ('WIFI_RAM_CODE_MT6582', 160368), ('WMT_SOC.cfg', 80), ('mt6572_82_patch_e1_0_hdr.bin', 33120),
            ('mt6572_82_patch_e1_1_hdr.bin', 17476), ('modem_1_2g_n.img', 2424376), ('WIFI.defaults', 512),
            ('BT.defaults', 64))}),
    status('status-system-firmware-installing', ['system'], files={
        '/run/y2/radio-firmware.json': '{"state": "Failed", "reason": "firmware_mismatch:x", "seen": [1, 2]}'}),
    status('status-system-evidence-bad-logs', ['system'], files={
        '/data/system/platform/previous-boot-evidence.json': '{"logs": {"a": 1}}'}),
    status('status-system-reset-unknown', ['system'], files={'/sys/firmware/y2_pm/reset_status': 'valid=1 raw=0x0\n'}),
    status('status-system-reset-invalid', ['system'], files={'/sys/firmware/y2_pm/reset_status': 'valid=0\n'}),
    status('status-audio-ready', ['readiness'], files=AUDIO),
    status('status-audio-float-rate', ['readiness'], files={**AUDIO, '/etc/y2linux/audio-qualified.json': json.dumps({
        'schema': 'org.y2linux.audio-qualification/v1', 'card': 'Y2Audio', 'channels': 2,
        'qualified_formats': ['S16_LE'], 'qualified_rates': [44100.0]})}),
    status('status-cpu-timer-crash', ['cpu'], files={'/proc/timer_list': 'cpu: 0\n  .nohz : 1\ncpu: '}),
    status('status-cpu-blank', ['cpu'], files={'/proc/stat': '', '/proc/timer_list': 'nothing\n'}),
    # int() of sysfs text Python reads: other decimal digits are ints, a superscript passes isdigit() and
    # then raises ValueError (main.py's record).
    status('status-cpu-timer-arabic-digit', ['cpu'], files={'/proc/timer_list': 'cpu: \u0661\n  .nohz : \u0662\n'}),
    status('status-cpu-timer-superscript', ['cpu'], files={'/proc/timer_list': 'cpu: \u00b2\n  .nohz : 1\n'}),
    status('status-storage-sd-superscript-size', ['storage'], files={**PLAYER, SD + '/mmcblk1p1/size': '\u00b2\n'},
           commands=VOLUMES),
    status('status-storage-controller-arabic', ['storage'], files={
        '/sys/bus/platform/devices/11230000.mmc/y2_performance': 'cap_hz=\u0665 clock_error=-1\n'}),
    # A receipt product that is a list of pairs: one with a key that is no text is refused only where the power
    # section is printed; a text or a pair of the wrong length is dict()'s ValueError.
    status('status-power-sleep-int-key', ['power'], files={'/data/system/platform/suspend-last.json': json.dumps(
        {'product': [[1, 'x'], ['state', 'restored'], ['request_boot_id', BOOT]]})}),
    status('status-cpu-sleep-int-key', ['cpu'], files={'/data/system/platform/suspend-last.json': json.dumps(
        {'product': [[1, 'x'], ['state', 'restored'], ['request_boot_id', BOOT]]})}),
    status('status-power-sleep-int-key-idle', ['power'], files={'/data/system/platform/suspend-last.json': json.dumps(
        {'product': [[None, 'x'], ['state', 'nap']]})}),
    status('status-power-sleep-pairs', ['power'], files={'/data/system/platform/suspend-last.json': json.dumps(
        {'product': ['st', {'state': 1, 'z': 2}, ['state', 'refused'], ['request_boot_id', BOOT]]})}),
    status('status-power-sleep-long-pair', ['power'], files={'/data/system/platform/suspend-last.json': json.dumps(
        {'product': [['state', 'idle'], ['a', 'b', 'c']]})}),
    status('status-power-sleep-text', ['power'], files={
        '/data/system/platform/suspend-last.json': '{"product": "restored"}'}),
    status('status-power-sleep-number', ['power'], files={'/data/system/platform/suspend-last.json': '{"product": 5}'}),
    status('status-power-sleep-number-pair', ['power'], files={
        '/data/system/platform/suspend-last.json': '{"product": [5]}'}),
    status('status-power-sleep-list-key', ['power'], files={
        '/data/system/platform/suspend-last.json': '{"product": [[[1], 2]]}'}),
    status('status-power-battery-null', ['power'], files={'/run/y2/power.json': fresh(
        {'boot_id': BOOT, 'battery': None}, 'monotonic_ns', ns=True)}),
    status('status-wifi-ip-text', ['wifi'], files=RADIO, commands=[ONLINE[0], answer(IP, '"ab"')], volatile=TIMING),
    status('status-wifi-ip-number-link', ['wifi'], files=RADIO, commands=[ONLINE[0], answer(IP, '[1]')],
           volatile=TIMING),
    status('status-wifi-ip-info-dict', ['wifi'], files=RADIO, commands=[
        ONLINE[0], answer(IP, '[{"addr_info": {"scope": "global"}}]')], volatile=TIMING),
    status('status-wifi-ip-info-null', ['wifi'], files=RADIO, commands=[
        ONLINE[0], answer(IP, '[{"addr_info": null}]'), answer(ROUTE, ROUTES)], volatile=TIMING),
    status('status-wifi-signal-text', ['wifi'], files={**RADIO, '/run/y2/network.json': '{"signal_observation": "x"}'},
           commands=ONLINE, volatile=TIMING),
    status('status-system-evidence-text-logs', ['system'], files={
        '/data/system/platform/previous-boot-evidence.json': '{"logs": "ab"}'}),
    status('status-system-evidence-null-logs', ['system'], files={
        '/data/system/platform/previous-boot-evidence.json': '{"logs": null, "pmic": 1}'}),
    status('status-system-evidence-list-log', ['system'], files={
        '/data/system/platform/previous-boot-evidence.json': '{"logs": [{"name": "a"}, [1]]}'}),
    status('status-system-update-dict-state', ['readiness'], files={
        '/data/updates/state.json': '{"schema": 1, "state": {"a": 1}}'}),
    # cli.py's parser.error after parsing (exit 2, usage on stderr): out-of-range, NaN or tiny intervals.
    status('status-interval-too-long', ['--interval', '11']),
    status('status-interval-tiny', ['--interval', '1e-05']),
    status('status-interval-negative-zero', ['cpu', '--interval', '-0']),
    status('status-huge-pid', ['memory', '--pid', '99999999999999999999']),  # beyond 64 bits: the parser refuses it
    # health
    ('health-ported', ['health'], {}),
    ('health-json', ['health', '--json'], {}),
    ('health-full', ['health', '--full'], {'commands': [answer(['y2-audio-contract'], 'ok')]}),
    ('health-player', ['health', '--json', '--full'], {'volatile': TIMING, 'files': {
        **EVERYTHING, '/sys/fs/ext4/mmcblk0p20/errors_count': '0\n', '/sys/fs/ext4/mmcblk0p19/errors_count': '0\n',
        '/dev/dri/renderD128': '', '/dev/dri/card0': '', '/dev/snd/pcmC0D0p': '', '/dev/input/event0': ''},
        'commands': [*EVERYTHING_COMMANDS, answer(['modetest', '-M', 'mediatek', '-c'], 'ok')]}),
    ('health-ext4-errors', ['health'], {'files': {
        '/sys/fs/ext4/mmcblk0p20/errors_count': '4\n', '/sys/fs/ext4/mmcblk0p19/errors_count': 'x\n'}}),
    ('health-ext4-unreadable', ['health'], {'files': {'/sys/fs/ext4/mmcblk0p20/errors_count': '-1\n'}}),
    ('health-player-storage', ['health'], {'files': PLAYER, 'commands': VOLUMES}),
]

# ---- boot-evidence -------------------------------------------------------------------------------------------

EVIDENCE = '/data/system/platform/previous-boot-evidence.json'
PMIC = '/sys/firmware/y2_pmic/power_status'
LOGS = {'/data/logs/system.log.0': ''.join('old line %d\n' % n for n in range(40)),
        '/data/logs/system.log': 'a' * 700 + '\nb\r\nc\x85d\n\xe9t\xe9\n'}
CASES += [
    ('boot-evidence', ['boot-evidence'], {'volatile': TIMING, 'files': {
        PMIC: 'valid=1 strup_con8=0x0010 top_rst_misc=0x0140 chr_con16=0x0001 strup_con5=0x0300 chrstatus=0x0002\n',
        '/sys/firmware/y2_boot/metadata': 'valid=1 boot_reason=4 lcm=1-st7701\n',
        '/sys/firmware/y2_pm/previous': 'suspend=1\n', '/sys/firmware/y2_pm/backstop_s': '30\n', **LOGS}}),
    ('boot-evidence-pstore', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/proc/self/mountinfo': '22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n'
                                '30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n'
                                '50 22 0:30 / /sys/fs/pstore rw - pstore pstore rw\n',
        '/sys/fs/pstore/dmesg-ramoops-0': 'panic\n' * 50, '/sys/fs/pstore/console-ramoops-0': 'console',
        '/sys/fs/pstore/' + 'p' * 120: 'long name', '/sys/fs/pstore/café': 'accent',
        '/sys/fs/pstore/subdir/x': 'not regular', '/sys/firmware/y2_pm/state/x': 'a directory',
        EVIDENCE: json.dumps({'captured_on_boot_id': 'boot-z', 'pmic': {'lk_long_press_record': False}}),
        PMIC: 'valid=1 strup_con8=0x4010\n', '/sys/firmware/y2_boot/metadata': 'valid=1 boot_reason=x\n'}}),
    ('boot-evidence-many-pstore', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/proc/self/mountinfo': '30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n'
                                '50 22 0:30 / /sys/fs/pstore rw - pstore pstore rw\n',
        **{'/sys/fs/pstore/entry-%02d' % n: str(n) for n in range(34)}}}),
    ('boot-evidence-pstore-missing', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/proc/self/mountinfo': '30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n'
                                '50 22 0:30 / /sys/fs/pstore rw - pstore pstore rw\n'}}),
    ('boot-evidence-rotation', ['boot-evidence'], {'volatile': TIMING, 'files': {
        **{'/data/system/platform/retained/boot-old%d.tar' % n: 'old' for n in range(9)},
        '/data/system/platform/retained/boot-dir.tar/x': '', '/data/system/platform/retained/notes.txt': ''}}),
    ('boot-evidence-again', ['boot-evidence'], {'files': {
        EVIDENCE: json.dumps({'captured_on_boot_id': BOOT, 'logs': [], 'kept': 'as it was'})}}),
    ('boot-evidence-missed-boundary', ['boot-evidence'], {'files': {
        '/data/system/platform/boot.json': json.dumps({'schema': 1, 'boot_id': BOOT})}}),
    ('boot-evidence-no-data', ['boot-evidence'], {'files': {'/data/.y2data-schema': '2\n'}}),
    ('boot-evidence-wrong-mount', ['boot-evidence'], {'files': {'/run/y2-data-device': '/dev/sda1\n'}}),
    ('boot-evidence-no-boot-id', ['boot-evidence'], {'files': {'/proc/sys/kernel/random/boot_id': ''}}),
    ('boot-evidence-odd-boot-id', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/proc/sys/kernel/random/boot_id': 'boot a!\n'}}),
    ('boot-evidence-retained-file', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/data/system/platform/retained': 'a file'}}),
    ('boot-evidence-logs-file', ['boot-evidence'], {'volatile': TIMING, 'files': {'/data/logs': 'x'}}),
    ('boot-evidence-log-directory', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/data/logs/system.log/x': ''}}),
    ('boot-evidence-bounded', ['boot-evidence'], {'volatile': TIMING, 'files': {
        '/data/logs/system.log.0': ('😀' * 600 + '\n') * 40, '/data/logs/system.log': ('🎵' * 600 + '\n') * 33}}),
]

# ---- network-check, collect, bench ---------------------------------------------------------------------------

PING = ['ping', '-I', 'usb0', '-c', '5', '-W', '2', '192.168.7.1']
IPERF = ['iperf3', '--bind-dev', 'usb0', '-c', '192.168.7.1', '-t', '3', '-J']
CASES += [
    ('network-check-offline', ['network-check', '--peer', '192.168.1.1'], {}),
    ('network-check-online', ['network-check', '--peer', '192.168.1.1', '--interface', 'wlan0'], {
        'volatile': TIMING, 'files': {**RADIO, '/run/y2/dns.json': DNS},
        'commands': [*ONLINE, answer(['ping', '-I', 'wlan0', '-c', '5', '-W', '2', '192.168.1.1'], '5 received')]}),
    ('network-check-usb', ['network-check', '--peer', '192.168.7.1', '--interface', 'usb0'], {
        'commands': [answer(PING, '5 packets transmitted')]}),
    ('network-check-usb-ping-fails', ['network-check', '--peer', '192.168.7.1', '--interface', 'usb0'], {}),
    ('network-check-throughput', ['network-check', '--peer', '192.168.7.1', '--seconds', '3', '--throughput',
                                  '--interface', 'usb0'], {'commands': [
        answer(PING, 'ok'), answer(IPERF, json.dumps({'end': {'sum_sent': {'bits_per_second': 9.5e7}}}))]}),
    ('network-check-throughput-error', ['network-check', '--peer', '192.168.7.1', '--seconds', '3', '--throughput',
                                        '--interface', 'usb0'], {'commands': [
        answer(PING, 'ok'), answer(IPERF, json.dumps({'end': {'sum_sent': {}}, 'error': 'busy'}))]}),
    ('network-check-throughput-not-json', ['network-check', '--peer', '192.168.7.1', '--seconds', '3',
                                           '--throughput', '--interface', 'usb0'], {'commands': [
        answer(PING, 'ok'), answer(IPERF, 'garbage')]}),
    ('network-check-throughput-fails', ['network-check', '--peer', '192.168.7.1', '--seconds', '3', '--throughput',
                                        '--interface', 'usb0'], {'commands': [answer(PING, 'ok')]}),
    ('network-check-workload', ['network-check', '--peer', '192.168.1.1'], {'files': {
        '/dev/y2-workload': ''}, 'patterns': [(r'(NetworkTransfer 3000\n)+', 'NetworkTransfer 3000\n')]}),
    ('network-check-bad-seconds', ['network-check', '--peer', '192.168.1.1', '--seconds', '0'], {}),
    # ipaddress.IPv4Address's AddressValueError, after the workload lease was taken.
    ('network-check-bad-peer', ['network-check', '--peer', '192.168.001.1'], {}),
    ('network-check-hostname', ['network-check', '--peer', 'router'], {}),
    # Arguments that are not UTF-8: the parser refuses them (exit 2).
    ('network-check-not-utf8', ['network-check', '--peer', '192.168.1.\udcff'], {}),
    ('collect-workload-not-utf8', ['collect', '--seconds', '1', '--interval', '1', '--workload', 'w\udcff'], {
        'volatile': [*TIMING, 'bytes']}),
    ('network-check-empty-peer', ['network-check', '--peer', ''], {}),
    ('network-check-peer-prefix', ['network-check', '--peer', '10.0.0.1/8'], {}),
    ('network-check-peer-octet', ['network-check', '--peer', '1.2.3.256'], {}),
    ('network-check-peer-long-octet', ['network-check', '--peer', '1.2.3.0004'], {}),
    ('network-check-peer-empty-octet', ['network-check', '--peer', '1..2.3'], {}),
    ('network-check-peer-arabic', ['network-check', '--peer', '\u0661.2.3.4'], {}),
    # Integers beyond 64 bits: the parser refuses them (exit 2).
    ('network-check-huge-seconds', ['network-check', '--peer', '192.168.1.1', '--seconds', '99999999999999999999'],
     {}),
    ('collect-huge-seconds', ['collect', '--seconds', '99999999999999999999'], {}),
    ('collect-huge-pid', ['collect', '--seconds', '1', '--interval', '1', '--pid', '-99999999999999999999'],
     {}),
    ('collect-one', ['collect', '--seconds', '1', '--interval', '1'], {
        'volatile': [*TIMING, 'bytes']}),
    ('collect-growth', ['collect', '--seconds', '2', '--interval', '1', '--pid', '4242', '--pss', '--workload', 'play'],
     {'volatile': [*TIMING, 'bytes', 'slope_kib_per_hour']}),
    ('collect-reborn', ['collect', '--seconds', '1', '--interval', '1', '--warmup', '1', '--reborn'], {
        'volatile': [*TIMING, 'bytes'], 'files': {'/run/reborn/process.pid': '4242\n'},
        'commands': [METRICS]}),
    ('collect-bounds', ['collect', '--seconds', '0'], {}),
    ('collect-warmup-bounds', ['collect', '--seconds', '1', '--warmup', '2'], {}),
    ('collect-label', ['collect', '--seconds', '1', '--workload', ''], {}),
    ('collect-label-control', ['collect', '--seconds', '1', '--workload', 'a\tb'], {}),
    ('collect-label-dash', ['collect', '--seconds', '1', '--interval', '1', '--workload', '-x'], {
        'volatile': [*TIMING, 'bytes']}),
    ('collect-invalid-pid', ['collect', '--seconds', '1', '--pid', '0'], {}),
    ('collect-too-many-pids', ['collect', '--seconds', '1', *[a for n in range(33) for a in ('--pid', str(n + 1))]],
     {}),
]
BENCH_TIMING = [*TIMING, 'cpu_busy_percent']
CASES += [
    ('bench-storage-unhealthy', ['bench-storage'], {}),
    ('bench-storage-internal', ['bench-storage', '--volume', '/media/internal'], {}),
    ('bench-storage-bounds', ['bench-storage', '--size-mib', '0'], {}),
    ('bench-storage-sd-missing', ['bench-storage', '--volume', '/media/sd', '--size-mib', '1'], {}),
    ('bench-storage-run', ['bench-storage', '--size-mib', '1', '--operations', '3', '--seconds', '120'], {
        'volatile': BENCH_TIMING, 'files': DATA_READY, 'commands': VOLUMES,
        'patterns': [(r'run-[0-9a-f]{32}', 'run-<hex>')]}),
    ('bench-storage-sd', ['bench-storage', '--volume', '/media/sd', '--size-mib', '1', '--operations', '1'], {
        'volatile': BENCH_TIMING, 'files': {**PLAYER, '/run/y2/media-mount.json': claim(read_only=None),
                                                            '/proc/self/mountinfo': MOUNTS.replace(
                                                                'ro,nosuid - vfat /dev/mmcblk1p1 ro',
                                                                'rw,nosuid - vfat /dev/mmcblk1p1 rw')},
        'commands': VOLUMES}),
    ('bench-library-unhealthy', ['bench-library'], {}),
    ('bench-library-internal', ['bench-library', '--volume', '/media/internal', '--scan'], {}),
    ('bench-library-no-child', ['bench-library'], {'files': DATA_READY, 'commands': VOLUMES}),
    ('bench-library-run', ['bench-library', '--tracks', '10000', '--scan'], {
        'files': DATA_READY, 'commands': [*VOLUMES, answer(
            ['/usr/bin/reborn-bench', '--scratch-fd', '5', '--tracks', '10000', '--scan'],
            json.dumps({'open_ms': 12.5, 'queries': [1, 2]}))]}),
    ('bench-library-not-json', ['bench-library'], {
        'files': DATA_READY, 'commands': [*VOLUMES, answer(
            ['/usr/bin/reborn-bench', '--scratch-fd', '5', '--tracks', '1000'], '')]}),
]


# Run by tests/test_platform_observation.py: boot-evidence below /data and the inputs the platform refuses (records
# it cannot read, which Python ended on with a traceback): exit 1 and the error record with this reason.
REFUSALS = {
    'status-cpu-timer-crash': 'invalid_timer_list',
    'status-power-battery-list': 'invalid_power_record',
    'status-power-battery-null': 'invalid_power_record',
    'status-power-sleep-int-key': 'invalid_sleep_receipt',
    'status-power-sleep-number': 'invalid_sleep_receipt',
    'status-power-sleep-number-pair': 'invalid_sleep_receipt',
    'status-power-sleep-list-key': 'invalid_sleep_receipt',
    'status-system-evidence-bad-logs': 'invalid_previous_boot_evidence',
    'status-system-evidence-text-logs': 'invalid_previous_boot_evidence',
    'status-system-evidence-null-logs': 'invalid_previous_boot_evidence',
    'status-system-evidence-list-log': 'invalid_previous_boot_evidence',
    'status-system-update-unhashable': 'invalid_update_journal',
    'status-system-update-dict-state': 'invalid_update_journal',
    'status-wifi-signal-list': 'invalid_network_record',
    'status-wifi-signal-text': 'invalid_network_record',
    'status-wifi-ip-text': 'invalid_ip_output',
    'status-wifi-ip-number-link': 'invalid_ip_output',
    'status-wifi-ip-info-dict': 'invalid_ip_output',
    'status-wifi-ip-object': 'invalid_ip_output',
}
EXTRA_CASES = [case for case in CASES if case[0] in REFUSALS or (
    case[1] == ['boot-evidence'] and case[0] not in ('boot-evidence-again', 'boot-evidence-missed-boundary',
                                                     'boot-evidence-no-data', 'boot-evidence-wrong-mount',
                                                     'boot-evidence-no-boot-id'))]
CASES = [case for case in CASES if case not in EXTRA_CASES]


def fixture(root):
    """Block devices the cases mount (not read by the base status) and a Reborn process."""
    def put(name, content=''):
        path = root / name.lstrip('/')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def link(name, target):
        path = root / name.lstrip('/')
        path.parent.mkdir(parents=True, exist_ok=True)
        os.symlink(os.path.relpath(root / target.lstrip('/'), path.parent), path)

    put(EMMC + '/diskseq', '7\n')
    for number in (19, 20, 21):
        partition = '%s/mmcblk0p%d' % (EMMC, number)
        put(partition + '/dev', '179:%d\n' % number)
        put(partition + '/partition', '%d\n' % number)
        put(partition + '/size', '2097152\n')
        link('/sys/dev/block/179:%d' % number, partition)
    put(SD + '/diskseq', '12\n')
    put(SD + '/device/cid', '1b534d454231515430\n')
    put(SD + '/mmcblk1p1/dev', '179:33\n')
    put(SD + '/mmcblk1p1/partition', '1\n')
    put(SD + '/mmcblk1p1/start', '8192\n')
    put(SD + '/mmcblk1p1/size', '1000\n')
    link('/sys/dev/block/179:33', SD + '/mmcblk1p1')
    put(SD_WHOLE + '/dev', '179:40\n')
    put(SD_WHOLE + '/diskseq', '0\n')
    put(SD_WHOLE + '/size', '64\n')
    link('/sys/dev/block/179:40', SD_WHOLE)
    put('/usr/bin/reborn', '')
    put('/usr/bin/other', '')
    fields = ['S', '1'] + [str(n) for n in range(2, 19)] + ['123456', '0', '0']
    for pid, program in ((4242, 'reborn'), (4243, 'other')):
        put('/proc/%d/stat' % pid, '%d (%s worker) %s\n' % (pid, program, ' '.join(fields)))
        put('/proc/%d/status' % pid, 'Name:\t%s\nThreads:\t7\nVmRSS:\t  27500 kB\nvoluntary_ctxt_switches:\t10\n'
                                     'nonvoluntary_ctxt_switches:\tx\n' % program)
        put('/proc/%d/smaps_rollup' % pid, 'Rss: 27500 kB\nPss:   24700 kB\n')
        link('/proc/%d/exe' % pid, '/usr/bin/' + program)
