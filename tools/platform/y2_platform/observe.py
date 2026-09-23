"""Read-only platform snapshots. Null means unobserved, never zero or success."""
# SPDX-License-Identifier: GPL-2.0-only
import json
import os
from pathlib import Path
import re
import time
from .common import counters, key_values, number, read


def unescape(value):
    return re.sub(r'\\(040|011|012|134)', lambda m: chr(int(m[1], 8)), value)


def mountinfo(text):
    result = []
    for line in (text or '').splitlines():
        left, sep, right = line.partition(' - ')
        a, b = left.split(), right.split()
        if not sep or len(a) < 6 or len(b) < 3 or not a[0].isdigit():
            continue
        result.append({'mount_id': int(a[0]), 'device_id': a[2], 'root': unescape(a[3]),
                       'path': unescape(a[4]), 'options': a[5].split(','),
                       'filesystem': b[0], 'source': unescape(b[1]),
                       'super_options': b[2].split(',')})
    return result


def device_instance(ctx, device_id):
    try:
        path = ctx.path('/sys/dev/block/' + device_id).resolve(strict=True)
        meta = path.stat()
        return f'{meta.st_ino}:{meta.st_ctime_ns}'
    except OSError:
        return None


def cpu(ctx):
    stat = ctx.read('/proc/stat')
    ticks = {}
    for line in (stat or '').splitlines():
        fields = line.split()
        if fields and re.fullmatch(r'cpu[0-9]*', fields[0]):
            # guest/guest_nice are already included in user/nice, omit double count.
            ticks[fields[0]] = [number(v) for v in fields[1:9]]
    policies = []
    for path in ctx.glob('/sys/devices/system/cpu/cpufreq/policy*'):
        fields = {name: read(path / name) for name in (
            'affected_cpus', 'scaling_governor', 'scaling_cur_freq',
            'scaling_min_freq', 'scaling_max_freq', 'scaling_available_frequencies',
            'cpuinfo_min_freq', 'cpuinfo_max_freq')}
        fields['name'] = path.name
        fields['time_in_state'] = counters(read(path / 'stats/time_in_state')) or None
        fields['time_in_state_unit'] = 'USER_HZ_ticks'
        fields['frequency_unit'] = 'kHz'
        policies.append(fields)
    idle = []
    for path in ctx.glob('/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*'):
        idle.append({'cpu': path.parent.parent.name, 'state': path.name,
                     'name': read(path / 'name'), 'time_us': number(read(path / 'time')),
                     'usage': number(read(path / 'usage')), 'disabled': number(read(path / 'disable'))})
    totals = counters(stat)
    return {'online': ctx.read('/sys/devices/system/cpu/online'),
            'load_average': ctx.read('/proc/loadavg'), 'ticks': ticks or None,
            'ticks_unit': 'USER_HZ_ticks', 'utilization_percent': None,
            'policies': policies, 'idle': idle,
            'counters': {k: totals.get(k) for k in ('ctxt', 'intr', 'softirq', 'processes',
                                                   'procs_running', 'procs_blocked')},
            'wakeup_count': ctx.integer('/sys/power/wakeup_count'),
            'throttling_reason': None}


def utilization(before, after):
    result = {}
    for core, new in (after or {}).items():
        old = (before or {}).get(core)
        if old is None or len(old) != len(new) or len(new) < 5:
            continue
        if any(v is None for v in old + new):
            continue
        diff = [b - a for a, b in zip(old, new)]
        total = sum(diff)
        if min(diff) >= 0 and total > 0:
            result[core] = round(100 * (total - diff[3] - diff[4]) / total, 3)
    return result or None


def memory(ctx, pids=(), pss=False):
    values = {k: number(v.split()[0]) for k, v in key_values(ctx.read('/proc/meminfo')).items() if v}
    processes = []
    for pid in pids:
        if not isinstance(pid, int) or not 1 <= pid <= 4194304:
            raise ValueError('invalid pid')
        status = key_values(ctx.read(f'/proc/{pid}/status'))
        stat_fields = (ctx.read(f'/proc/{pid}/stat') or '').rsplit(') ', 1)[-1].split()
        smaps = key_values(ctx.read(f'/proc/{pid}/smaps_rollup')) if pss else {}
        processes.append({'pid': pid, 'name': status.get('Name'),
                          'start_ticks': number(stat_fields[19]) if len(stat_fields) > 19 else None,
                          'rss_kib': number(status.get('VmRSS', '').split(' ')[0]),
                          'pss_kib': number(smaps.get('Pss', '').split(' ')[0]),
                          'threads': number(status.get('Threads')),
                          'voluntary_context_switches': number(status.get('voluntary_ctxt_switches')),
                          'involuntary_context_switches': number(status.get('nonvoluntary_ctxt_switches'))})
    return {'unit': 'KiB', 'meminfo': values or None,
            'vmstat': counters(ctx.read('/proc/vmstat')) or None,
            'pressure': ctx.read('/proc/pressure/memory'), 'processes': processes}


def thermal(ctx):
    zones = []
    for path in ctx.glob('/sys/class/thermal/thermal_zone*'):
        trips = []
        for trip in sorted(path.glob('trip_point_*_temp')):
            trips.append({'name': trip.stem, 'temperature_millicelsius': number(read(trip)),
                          'type': read(trip.with_name(trip.name.replace('_temp', '_type')))})
        zones.append({'name': path.name, 'type': read(path / 'type'),
                      'temperature_millicelsius': number(read(path / 'temp')), 'trips': trips})
    cooling = [{'name': p.name, 'type': read(p / 'type'),
                'state': number(read(p / 'cur_state')), 'max_state': number(read(p / 'max_state'))}
               for p in ctx.glob('/sys/class/thermal/cooling_device*')]
    return {'zones': zones, 'cooling': cooling, 'battery_temperature': None,
            'battery_temperature_reason': 'no_qualified_pack_sensor'}


def power(ctx):
    supplies = []
    fields = ('type', 'status', 'health', 'online', 'present', 'usb_type',
              'voltage_now', 'voltage_min_design', 'voltage_max_design',
              'constant_charge_current', 'constant_charge_current_max',
              'constant_charge_voltage', 'constant_charge_voltage_max', 'input_current_limit')
    for path in ctx.glob('/sys/class/power_supply/*'):
        supply = {f: read(path / f) for f in fields}
        supply['name'] = path.name
        supplies.append(supply)
    policy = ctx.json('/run/y2/power.json', {})
    if (policy.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id') or
            not isinstance(policy.get('monotonic_ns'), int) or
            not 0 <= time.monotonic_ns() - policy['monotonic_ns'] < 5 * 10**9):
        policy = {'state': 'Unavailable', 'reason': 'policy_not_running_or_stale'}
    return {'supplies': supplies, 'units': {'voltage': 'uV', 'configured_current': 'uA'},
            'measured_current_ua': None, 'soc_percent': None, 'pack_temperature': None,
            'unavailable_reason': 'not_exposed_by_qualified_y2_battery_driver',
            'normal_boot': ctx.integer('/sys/firmware/y2_boot/normal_boot'),
            'low_battery': policy}


def space_state(available, total, readonly=False, error=False):
    # Product capacity budget, not electrical thresholds. Never delete user data.
    if error:
        return 'Failed'
    if readonly:
        return 'ReadOnlyRisk'
    if available is None or total is None:
        return 'Unavailable'
    if available < max(32 * 1024**2, total // 20):
        return 'CriticalSpace'
    if available < max(96 * 1024**2, total // 10):
        return 'LowSpace'
    return 'Normal'


def storage(ctx):
    mounts = mountinfo(ctx.read('/proc/self/mountinfo'))
    boot_id = ctx.read('/proc/sys/kernel/random/boot_id')
    volumes = []
    for target in ('/', '/data', '/media/sd'):
        found = [m for m in mounts if m['path'] == target]
        # Stacked mounts are ambiguous to consumers; fail closed.
        if len(found) != 1:
            volumes.append({'path': target, 'state': 'Unavailable', 'reason': 'not_uniquely_mounted',
                            'generation': None, 'space_state': 'Unavailable'})
            continue
        item = dict(found[0])
        item['generation'] = f"{boot_id}:{item['mount_id']}:{item['device_id']}" if boot_id else None
        device = ctx.path('/sys/dev/block/' + item['device_id'])
        real = str(device.resolve())
        expected = '11240000.mmc' if target == '/media/sd' else '11230000.mmc'
        device_exists = device.exists()
        item['source_instance'] = device_instance(ctx, item['device_id'])
        item['controller_valid'] = expected in real and device_exists
        item['state'] = 'Ready' if item['controller_valid'] else 'Failed'
        item['reason'] = None if item['controller_valid'] else 'source_missing_or_wrong_controller'
        item['uuid'] = None
        if item['controller_valid']:
            answer = ctx.command(['blkid', '-p', '-s', 'UUID', '-o', 'value', item['source']])
            item['uuid'] = answer['output'] if answer['ok'] else None
            expected_uuid = {'/': '79324c69-6e75-4801-8000-000000000101',
                             '/data': '79324c69-6e75-4801-8000-000000000102'}.get(target)
            if not item['uuid'] or (expected_uuid and item['uuid'] != expected_uuid):
                item.update(state='Failed', reason='filesystem_identity_invalid')
            if target == '/media/sd':
                claim = ctx.json('/run/y2/media-mount.json', {})
                if not all(claim.get(k) == v for k, v in
                           (('boot_id', boot_id), ('mount_id', item['mount_id']),
                            ('device_id', item['device_id']), ('uuid', item['uuid']))):
                    item.update(state='Failed', reason='mount_claim_changed_or_missing')
                if claim.get('source_instance') != item['source_instance']:
                    item.update(state='Failed', reason='block_instance_changed')
        readonly = 'ro' in item['options'] or 'ro' in item['super_options']
        try:
            vfs = os.statvfs(ctx.path(target))
            item.update(total_bytes=vfs.f_blocks * vfs.f_frsize,
                        available_bytes=vfs.f_bavail * vfs.f_frsize,
                        free_bytes=vfs.f_bfree * vfs.f_frsize,
                        available_inodes=vfs.f_favail)
            item['space_state'] = space_state(item['available_bytes'], item['total_bytes'], readonly,
                                             item['state'] == 'Failed')
            if vfs.f_files > 0 and vfs.f_favail == 0:
                item['space_state'] = 'CriticalSpace'
        except OSError:
            item['space_state'] = 'Failed'
            item.update(state='Failed', reason='statvfs_failed')
        if item['state'] == 'Ready' and readonly:
            item.update(state='Degraded', reason='read_only')
        volumes.append(item)
    blocks = []
    for path in ctx.glob('/sys/class/block/*'):
        if not any(c in str(path.resolve()) for c in ('11230000.mmc', '11240000.mmc')):
            continue
        blocks.append({'name': path.name, 'stat': read(path / 'stat'),
                       'stat_unit': 'Linux_block_stat_fields_sectors_512_bytes',
                       'size_sectors': number(read(path / 'size')),
                       'type': read(path / 'device/type'), 'name_id': read(path / 'device/name'),
                       'cid': read(path / 'device/cid'), 'serial': read(path / 'device/serial'),
                       'io_error_count': number(read(path / 'device/ioerr_cnt'))})
    return {'volumes': volumes, 'blocks': blocks,
            'bus_parameters': ctx.read('/sys/kernel/debug/mmc0/ios'),
            'sd_bus_parameters': ctx.read('/sys/kernel/debug/mmc1/ios')}


def wifi_state(radio, wpa, address, route, dns, dhcp_error=None):
    if not radio:
        return 'Off', 'radio_off'
    if not wpa:
        return 'Starting', 'supplicant_unavailable'
    state = wpa.get('wpa_state')
    if state == 'SCANNING':
        return 'Scanning', None
    if state in ('ASSOCIATING', 'ASSOCIATED', '4WAY_HANDSHAKE', 'GROUP_HANDSHAKE'):
        return 'Associating', None
    if state != 'COMPLETED':
        return 'Failed', wpa.get('failure_reason', 'not_authenticated')
    if not address:
        return ('Failed', dhcp_error) if dhcp_error else ('AcquiringIP', 'no_address')
    if not route:
        return 'Authenticated', 'no_default_route'
    if not dns:
        return 'Authenticated', 'dns_unavailable'
    return 'Online', None


def wifi(ctx):
    radio = ctx.path('/sys/class/net/wlan0').exists()
    wpa = ctx.command(['wpa_cli', '-i', 'wlan0', 'status']) if radio else {'output': None}
    properties = key_values(wpa['output'], '=')
    failure = ctx.json('/run/y2/wifi-failure.json', {})
    failed_time = failure.get('monotonic_s')
    if (failure.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
            isinstance(failed_time, (int, float)) and 0 <= time.monotonic() - failed_time < 120 and
            failure.get('error')):
        properties['failure_reason'] = failure['error']
    ip = ctx.command(['ip', '-j', '-4', 'addr', 'show', 'dev', 'wlan0']) if radio else {'output': None}
    routes = ctx.command(['ip', '-j', '-4', 'route', 'show', 'default', 'dev', 'wlan0']) if radio else {'output': None}
    try:
        addresses = [a['local'] for link in json.loads(ip['output'] or '[]')
                     for a in link.get('addr_info', []) if a.get('scope') == 'global']
        default_route = json.loads(routes['output'] or '[]')
    except (ValueError, TypeError, KeyError):
        addresses, default_route = [], []
    dns = ctx.json('/run/y2/dns.json', {})
    boot = ctx.read('/proc/sys/kernel/random/boot_id')
    # Configuration alone never constitutes DNS readiness. Match network epoch.
    dns_time = dns.get('monotonic_s')
    dns_ready = (isinstance(dns_time, (int, float)) and dns.get('ok') is True and dns.get('boot_id') == boot and
                 dns.get('address') in addresses and dns.get('bssid') == properties.get('bssid') and
                 0 <= time.monotonic() - dns_time < 120)
    dhcp = ctx.json('/run/y2/dhcp.json', {})
    error = dhcp.get('error') if dhcp.get('boot_id') == boot and dhcp.get('epoch') == ctx.read('/run/y2/wifi-epoch') else None
    state, reason = wifi_state(radio, properties, addresses, default_route, dns_ready, error)
    stats = {p.name: number(read(p)) for p in ctx.glob('/sys/class/net/wlan0/statistics/*')}
    signal_poll = ctx.command(['wpa_cli', '-i', 'wlan0', 'signal_poll']) if radio else {'output': None}
    signal_values = key_values(signal_poll['output'], '=')
    return {'state': state, 'reason': reason, 'radio_present': radio,
            'association': properties.get('wpa_state'), 'association_id': properties.get('bssid'),
            'ip_addresses': addresses,
            'default_route': default_route, 'dns_ready': dns_ready,
            'rssi_dbm': number(signal_values.get('RSSI')), 'traffic_counters': stats,
            'events': ctx.json('/run/y2/wifi-events.json'), 'reconnect_owner': 'wpa_supplicant'}


def bluetooth(ctx):
    result = ctx.command(['/usr/sbin/y2-bt-observe'], timeout=3, limit=262144)
    try:
        observed = json.loads(result['output'] or '')
        if isinstance(observed, dict) and observed.get('schema') == 1:
            from .bluetooth import normalize
            return normalize(ctx, observed)
    except (ValueError, TypeError):
        pass
    return {'state': 'Unavailable', 'reason': 'dbus_observation_unavailable',
            'adapter': None, 'selected_peer': None, 'transport': None,
            'negotiated_codec': None, 'pcm': None, 'reconnect_owner': 'y2-bt-reconnect',
            'reconnect': ctx.json('/run/y2/bt-reconnect.json')}


def system(ctx):
    from .update import status as update_status
    from .boot import evidence_status
    from .timekeeping import status as time_status
    clock = time_status(ctx)
    ssh = ctx.json('/run/y2/ssh.json', {})
    if ssh.get('boot_id') != ctx.read('/proc/sys/kernel/random/boot_id') or not isinstance(ssh.get('monotonic_s'), (int, float)) or not 0 <= time.monotonic() - ssh['monotonic_s'] < 10:
        ssh = {'state': 'Unavailable', 'reason': 'ssh_record_stale_or_absent'}
    stages = []
    for line in (ctx.read('/run/y2/boot-stages.jsonl') or '').splitlines()[-32:]:
        try:
            value = json.loads(line)
            if isinstance(value, dict):
                stages.append(value)
        except ValueError:
            continue
    udcs = [{'name': p.name, 'state': read(p / 'state'), 'speed': read(p / 'current_speed')}
            for p in ctx.glob('/sys/class/udc/*')]
    rtcs = [{f: read(p / f) for f in ('name', 'date', 'time', 'since_epoch', 'wakealarm')}
            for p in ctx.glob('/sys/class/rtc/rtc*')]
    return {'uptime_seconds': (ctx.read('/proc/uptime') or '').split(' ')[0] or None,
            'versions': ctx.json('/etc/y2linux/versions.json'),
            'boot_history': ctx.json('/data/system/platform/boot.json'),
            'previous_boot_evidence': evidence_status(ctx),
            'application_readiness': ctx.json('/run/y2/application-ready.json'),
            'boot_stages': stages,
            'reset_cause': None, 'reset_cause_reason': 'no_qualified_retained_register',
            'watchdogs': [{f: read(p / f) for f in ('identity', 'state', 'bootstatus', 'status', 'timeout')}
                          for p in ctx.glob('/sys/class/watchdog/watchdog*')],
            'pstore_files': [p.name for p in ctx.glob('/sys/fs/pstore/*')],
            'rtc': rtcs, 'time': clock, 'ssh': ssh,
            'entropy_available_bits': ctx.integer('/proc/sys/kernel/random/entropy_avail'),
            'crng_ready': clock['entropy_ready'], 'usb': {'mode': 'peripheral', 'udcs': udcs},
            'update': update_status(ctx),
            'reborn_supervisor': ctx.json('/data/reborn/logs/supervisor-last.json')}


def readiness(status):
    def item(state, reason=None):
        return {'state': state, 'reason': reason}
    volumes = status['storage']['volumes']
    wifi = status['wifi']
    return {
        'storage': item(('Ready' if all(v['space_state'] == 'Normal' for v in volumes[:2]) else 'Degraded')
                        if all(v['state'] == 'Ready' for v in volumes[:2]) else 'Failed',
                        next((v.get('reason') for v in volumes[:2] if v['state'] != 'Ready'), None)),
        'wifi': item({'Online': 'Ready', 'Off': 'Unavailable', 'Failed': 'Failed',
                      'Authenticated': 'Degraded'}.get(wifi['state'], 'Starting'), wifi['reason']),
        'bluetooth': item(status['bluetooth']['state'], status['bluetooth'].get('reason')),
        'audio': item('Ready' if re.search(r'^\s*\d+\s+\[', status['audio']['cards'] or '', re.M)
                      and status['audio']['qualified_profile']
                      else 'Unavailable', 'profile_does_not_imply_current_physical_acceptance'),
        'update': item({'Idle': 'Ready', 'Acknowledged': 'Ready', 'RolledBack': 'Degraded',
                        'Queued': 'Starting', 'PendingHealth': 'Starting', 'RollbackPending': 'Degraded',
                        'Failed': 'Failed', 'RescueRequired': 'Failed'}.get(status['system']['update'].get('state'), 'Unavailable'),
                       status['system']['update'].get('failure')),
    }


def snapshot(ctx, pids=(), pss=False, interval=0):
    start = time.monotonic_ns()
    cpu_before = cpu(ctx)
    if interval:
        time.sleep(interval)
    cpu_after = cpu(ctx) if interval else cpu_before
    if interval:
        cpu_after['utilization_percent'] = utilization(cpu_before['ticks'], cpu_after['ticks'])
    result = {'schema': 'org.y2linux.status/v1', 'record': ctx.record(),
              'cpu': cpu_after, 'memory': memory(ctx, pids, pss), 'thermal': thermal(ctx),
              'power': power(ctx), 'storage': storage(ctx), 'wifi': wifi(ctx),
              'bluetooth': bluetooth(ctx), 'system': system(ctx),
              'audio': {'cards': ctx.read('/proc/asound/cards'), 'pcm': ctx.read('/proc/asound/pcm'),
                        'qualified_profile': ctx.json('/etc/y2linux/audio-qualified.json'),
                        'hw_params': {str(p.relative_to(ctx.root)): read(p)
                                      for p in ctx.glob('/proc/asound/card*/pcm*/sub*/hw_params')}}}
    result['readiness'] = readiness(result)
    result['record']['result'] = 'observed'
    result['record']['units'] = {'collection_duration': 'ns'}
    result['record']['collection_duration_ns'] = time.monotonic_ns() - start
    return result
