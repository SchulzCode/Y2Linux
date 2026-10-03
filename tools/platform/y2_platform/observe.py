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


def timer_runtime(ctx):
    if ctx.root == Path('/'):
        result = ctx.command(['/bin/cat', '/proc/timer_list'], timeout=0.2, limit=262144)
        text = result.get('output') if result['ok'] else None
    else:
        text = ctx.read('/proc/timer_list', limit=262144)
    cpus = []
    for block in re.split(r'^cpu:\s*', text or '', flags=re.M)[1:]:
        first = block.splitlines()[0]
        if not first.isdigit():
            continue
        fields = dict(re.findall(r'^\s*\.(hres_active|nohz|tick_stopped|nr_hangs)\s*:\s*(\d+)\s*$', block, flags=re.M))
        cpus.append({'cpu': int(first), **{key: int(value) for key, value in fields.items()}})
    def observed(key):
        values = [core.get(key) for core in cpus]
        return all(value > 0 if key == 'nohz' else value == 1 for value in values) if values and all(value is not None for value in values) else None
    return {'per_cpu': cpus, 'highres_active': observed('hres_active'),
            'no_hz_active': observed('nohz'), 'source': '/proc/timer_list',
            'reason': None if cpus else 'runtime_timer_state_unavailable'}


def idle_completion(ctx, states):
    """Counters come from cpuidle's returned state; absent is never a PASS."""
    result = {}
    for label, name in (('C1', 'WFI'), ('C2', 'SLIDLE'), ('C3', 'DORMANT')):
        rows = [state for state in states if state['name'] == name]
        if label != 'C1':
            rows = [state for state in rows if state['cpu'] == 'cpu0']
        def total(field):
            values = [row[field] for row in rows]
            return sum(values) if values and all(v is not None for v in values) else None
        result[label] = {'registered': bool(rows), 'per_cpu': rows,
                         'enabled': all(row['disabled'] == 0 for row in rows) if rows else None,
                         'entries': total('usage'), 'residency_us': total('time_us'),
                         'failures': total('rejected')}
    result['C2']['attribution'] = {
        p.name: read(p) for p in ctx.glob('/sys/module/idle/parameters/slow_*')}
    result['C2']['last_blocker'] = ctx.read('/sys/module/clocks/parameters/slow_blocker_names')
    mask = ctx.integer('/sys/module/clocks/parameters/slow_blockers')
    owners = []
    for bit, clock, owner, node in (
        (11, 'APDMA', 'connectivity / I2C DMA', '18070000.connectivity'), (12, 'MSDC0', 'eMMC', '11230000.mmc'),
        (13, 'MSDC1', 'SD', '11240000.mmc'), (14, 'MSDC2', 'unused controller', None),
        (20, 'BTIF', 'connectivity', '18070000.connectivity'),
        (21, 'I2C0', 'wheel', '11007000.i2c'), (22, 'I2C1', 'DAC', '11008000.i2c'),
        (23, 'I2C2', 'unused controller', None),
    ):
        if mask is not None and mask & (1 << bit):
            owners.append({'bit': bit, 'clock': clock, 'owner': owner,
                           'runtime_status': ctx.read('/sys/bus/platform/devices/'+node+'/power/runtime_status') if node else None})
    result['C2']['blocker_owners'] = owners if mask is not None else None
    result['C2']['shared_dma_owners'] = [
        {'name': node, 'runtime_status': ctx.read('/sys/bus/platform/devices/'+node+'/power/runtime_status')}
        for node in ('18070000.connectivity', '11007000.i2c', '11008000.i2c')]
    result['C3']['preflight'] = ctx.read('/sys/devices/platform/10006000.power-controller/dormant_preflight')
    result['C3']['boot_policy'] = ctx.json('/run/y2/cpu-idle-policy.json', {})
    result['C3']['unused_clock_handoff'] = ctx.read('/sys/module/clocks/parameters/unused_handoff')
    result['C3']['context'] = ctx.read('/sys/devices/platform/10006000.power-controller/state')
    result['C3']['uart_clock_handoff'] = ctx.read('/sys/module/clocks/parameters/uart_sleep_handoff')
    uart = dict(re.findall(r'(\w+)=([^\s]+)', result['C3']['context'] or ''))
    result['C3']['uart_sleep'] = {key: value for key, value in uart.items()
                                if key.startswith(('uart_sleep_', 'uart_power_', 'uart_r13_', 'uart_request_', 'uart_ack_'))}
    result['C3']['entry_budget'] = ctx.integer('/sys/module/spm/parameters/dormant_budget')
    result['C3']['display_idle'] = {p.name: read(p) for p in ctx.glob('/sys/module/mm_clocks/parameters/*')}
    preflight = dict(re.findall(r'(\w+)=([^\s]+)', result['C3']['preflight'] or ''))
    result['C3']['prerequisites'] = {k.removeprefix('prerequisite_'): v == '1'
                                   for k, v in preflight.items() if k.startswith('prerequisite_')}
    result['C3']['unmet'] = preflight.get('unmet', '').strip(',').split(',') if preflight.get('unmet') else []
    peripheral = {0: 'NFI', **{n+1: 'PWM'+str(n) for n in range(1, 8)}, 9: 'PWM',
                  10: 'USB0', 11: 'APDMA', 12: 'MSDC0', 13: 'MSDC1', 14: 'MSDC2', 15: 'NLI',
                  17: 'UART1', 18: 'UART2', 19: 'UART3', 20: 'BTIF', 21: 'I2C0', 22: 'I2C1',
                  23: 'I2C2', 25: 'SPI0'}
    display = dict(enumerate(('SMI_COMMON', 'SMI_LARB0', 'CMDQ', 'MUTEX', 'COLOR', 'BLS',
                             'DISP_WDMA', 'DISP_RDMA', 'OVL', 'MDP_TDSHP', 'MDP_WROT',
                             'MDP_WDMA', 'MDP_RSZ1', 'MDP_RSZ0', 'MDP_RDMA', 'BLS_26M',
                             'CAM_MDP', 'FAKE_ENG', 'MUTEX_32K')))
    result['C3']['blocker_owners'] = [] if result['C3']['preflight'] is not None else None
    for field, names, owner in (
        ('peri_blockers', peripheral, 'peripheral controller'),
        ('infra_blockers', {5: 'AFE', 7: 'L2C_SRAM', 13: 'TRNG', 15: 'CPUM'}, 'infrastructure clock owner'),
        ('disp0', display, 'DRM / inherited engine'),
        ('disp1', {0: 'DSI_ENGINE', 1: 'DSI_DIGITAL', 2: 'DPI_DIGITAL', 3: 'DPI_ENGINE'}, 'DRM / inherited engine'),
    ):
        try:
            mask = int(preflight[field], 0)
        except (KeyError, ValueError):
            mask = None
        if mask is None:
            continue
        for bit in range(32):
            if mask & (1 << bit):
                clock = names.get(bit, 'unknown')
                node = {'USB0': '11200000.usb', 'APDMA': '18070000.connectivity',
                        'BTIF': '18070000.connectivity', 'MSDC0': '11230000.mmc',
                        'MSDC1': '11240000.mmc', 'I2C0': '11007000.i2c', 'I2C1': '11008000.i2c'}.get(clock)
                result['C3']['blocker_owners'].append({'group': field, 'bit': bit, 'clock': clock,
                    'owner': owner, 'runtime_status': ctx.read('/sys/bus/platform/devices/'+node+'/power/runtime_status') if node else None})
    return result


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
            'cpuinfo_min_freq', 'cpuinfo_max_freq', 'cpuinfo_cur_freq', 'scaling_driver')}
        ceiling = ctx.integer('/sys/module/cpu_dvfs/parameters/qualification_max_khz')
        maximum = number(fields['scaling_max_freq'])
        admitted = min(ceiling, maximum) if ceiling is not None and maximum is not None else None
        fields['admitted_opp_khz'] = [int(value) for value in (fields['scaling_available_frequencies'] or '').split()
                                      if value.isdigit() and admitted is not None and int(value) <= admitted]
        fields['name'] = path.name
        fields['time_in_state'] = counters(read(path / 'stats/time_in_state')) or None
        fields['time_in_state_unit'] = 'USER_HZ_ticks'
        fields['frequency_unit'] = 'kHz'
        policies.append(fields)
    idle = []
    for path in ctx.glob('/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*'):
        idle.append({'cpu': path.parent.parent.name, 'state': path.name,
                     'name': read(path / 'name'), 'time_us': number(read(path / 'time')),
                     'usage': number(read(path / 'usage')),
                     'exit_latency_us': number(read(path / 'latency')),
                     'target_residency_us': number(read(path / 'residency')),
                     'rejected': number(read(path / 'rejected')), 'disabled': number(read(path / 'disable'))})
    totals = counters(stat)
    # This sysfs read is a suspend handshake: it blocks while a wakeup source
    # is active, even with O_NONBLOCK. Keep it outside the observer process and
    # use the existing bounded runner to kill/reap an interruptible stalled read.
    wakeup = ctx.command(['/bin/cat', str(ctx.path('/sys/power/wakeup_count'))],
                         timeout=0.1, limit=64)
    wakeup_count = number(wakeup['output']) if wakeup['ok'] else None
    if wakeup_count is not None and wakeup_count < 0:
        wakeup_count = None
    return {'online': ctx.read('/sys/devices/system/cpu/online'),
            'load_average': ctx.read('/proc/loadavg'), 'ticks': ticks or None,
            'ticks_unit': 'USER_HZ_ticks', 'utilization_percent': None,
            'policies': policies, 'idle': idle, 'idle_completion': idle_completion(ctx, idle),
            'voltage_uv': ctx.integer('/sys/module/pwrap/parameters/cpu_voltage_uv'),
            'stock_bin': ctx.read('/sys/module/cpu_dvfs/parameters/bin_supported'),
            'dvfs_ceiling_khz': ctx.integer('/sys/module/cpu_dvfs/parameters/qualification_max_khz'),
            'dvfs_fault': ctx.read('/sys/module/cpu_dvfs/parameters/voltage_fault'),
            'spm': ctx.read('/sys/devices/platform/10006000.power-controller/state'),
            'dormant_preflight': ctx.read('/sys/devices/platform/10006000.power-controller/dormant_preflight'),
            'cirq': ctx.read('/sys/devices/platform/10204000.interrupt-latch/state'),
            'suspend_stage': ctx.read('/run/y2/suspend-stage'),
            'suspend_persistent': {'current': ctx.read('/sys/firmware/y2_pm/state'),
                                   'previous_boot': ctx.read('/sys/firmware/y2_pm/previous'),
                                   'durable': ctx.json('/data/system/platform/suspend-last.json', None),
                                   'usb_restore': {p.parent.name: read(p, limit=8192) for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/status')},
                                   'radio_restore': ctx.read('/sys/devices/platform/18070000.connectivity/status'),
                                   # Fix02: awake SRAM proof, retention and callback trail.
                                   'retention': ctx.read('/sys/firmware/y2_pm/retention'),
                                   'selftest': ctx.read('/sys/firmware/y2_pm/selftest'),
                                   'backstop': ctx.read('/sys/firmware/y2_pm/backstop_s'),
                                   'device_callbacks': ctx.read('/sys/firmware/y2_pm/devices', limit=16384),
                                   'device_callbacks_previous_boot': ctx.read('/sys/firmware/y2_pm/devices_previous', limit=16384),
                                   # Fix03: decoded RGU cause of the reset that started this boot.
                                   'reset_status': ctx.read('/sys/firmware/y2_pm/reset_status')},
            'dvfs_diagnostics': {p.name: read(p) for p in ctx.glob('/sys/module/cpu_dvfs/parameters/*')},
            'voltage_ownership': ctx.read('/sys/module/pwrap/parameters/cpu_voltage_state'),
            'slidle_clock_owners': ctx.read('/sys/module/clocks/parameters/slow_blocker_names'),
            'pwrap_readiness': ctx.read('/sys/module/pwrap/parameters/pwrap_readiness'),
            'slidle_attribution': {p.name: read(p) for p in ctx.glob('/sys/module/clocks/parameters/slow_*')},
            'mmc_runtime_pm': {p.parent.name: read(p) for p in ctx.glob('/sys/bus/platform/devices/*.mmc/y2_runtime_pm')},
            'system_idle': {p.name: read(p) for p in ctx.glob('/sys/module/system_idle/parameters/*')},
            'radio_boot_retries': ctx.integer('/sys/devices/platform/18070000.connectivity/boot_retries'),
            'clock_blockers': {p.name: read(p) for p in ctx.glob('/sys/module/clocks/parameters/*blockers')},
            'idle_diagnostics': {p.name: read(p) for p in ctx.glob('/sys/module/idle/parameters/*')},
            'recovery_options': {p.name: read(p) for p in ctx.glob('/sys/module/cpu_options/parameters/*')},
            'counters': {k: totals.get(k) for k in ('ctxt', 'intr', 'softirq', 'processes',
                                                   'procs_running', 'procs_blocked')},
            'wakeup_count': wakeup_count,
            'wakeup_count_reason': None if wakeup_count is not None else
                                   (wakeup.get('reason') or 'counter_unavailable'),
            'throttling_reason': None,
            'workload_qos': {'implemented': ctx.path('/dev/y2-workload').exists(), 'lease_max_ms': 30000,
                             'interactive_max_ms': 500,
                             'leases': ctx.read('/sys/module/workload/parameters/leases')},
            'timer': {**timer_runtime(ctx), 'admission': {p.name: read(p) for p in ctx.glob('/sys/module/local_timer/parameters/*')}, 'clocksource': ctx.read('/sys/devices/system/clocksource/clocksource0/current_clocksource'),
                      'broadcast_clockevent': ctx.read('/sys/devices/system/clockevents/broadcast/current_device'),
                      'clockevents': {p.parent.name: read(p) for p in ctx.glob('/sys/devices/system/clockevents/clockevent*/current_device')}}}


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
    from .sleep import status as sleep_status
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
    battery = policy.get('battery', {})
    return {'supplies': supplies, 'units': {'voltage': 'uV', 'configured_current': 'uA'},
            'battery': battery,
            'measured_current_ua': battery.get('current_ua'), 'soc_percent': battery.get('percent'),
            'soc_source': battery.get('source'), 'soc_confidence': battery.get('confidence'),
            'pack_temperature': battery.get('temperature_millicelsius'),
            'unavailable_reason': 'not_exposed_by_qualified_y2_battery_driver',
            'normal_boot': ctx.integer('/sys/firmware/y2_boot/normal_boot'),
            'low_battery': policy, 'sleep': sleep_status(ctx)}


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


def storage_mode(text):
    """The driver's negotiated-mode line as typed fields; absent on older kernels."""
    if not text:
        return None
    mode = {}
    for key, value in re.findall(r'([a-z0-9_]+)=(\S+)', text):
        if re.fullmatch(r'-?\d+', value):
            mode[key] = int(value)
        elif re.fullmatch(r'0x[0-9a-f]+', value):
            mode[key] = int(value, 16)
        else:
            mode[key] = value
    return mode


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
            item['uuid'] = (answer['output'] or None) if answer['ok'] else None
            expected_uuid = {'/': '79324c69-6e75-4801-8000-000000000101',
                             '/data': '79324c69-6e75-4801-8000-000000000102'}.get(target)
            if expected_uuid and item['uuid'] != expected_uuid:
                item.update(state='Failed', reason='filesystem_identity_invalid')
            if target == '/media/sd':
                from .media import media_identity
                identity = media_identity(device, item['filesystem'], item['uuid'])
                item['identity'] = identity
                claim = ctx.json('/run/y2/media-mount.json', {})
                if not all(claim.get(k) == v for k, v in
                           (('boot_id', boot_id), ('mount_id', item['mount_id']),
                            ('device_id', item['device_id']), ('uuid', item['uuid']))):
                    item.update(state='Failed', reason='mount_claim_changed_or_missing')
                if item['state'] == 'Ready' and (identity is None or (claim.get('identity') is not None and claim['identity'] != identity)):
                    item.update(state='Failed', reason='media_identity_changed_or_missing')
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
    controllers = []
    for path in ctx.glob('/sys/bus/platform/devices/*.mmc/y2_performance'):
        values = dict(re.findall(r'(cap_hz|actual_hz|transport_errors|fallbacks|clock_error)=(-?\d+)', read(path) or ''))
        controllers.append({'name': path.parent.name, **{key: int(value) for key, value in values.items()},
                            'mode': storage_mode(read(path.parent / 'y2_storage', limit=4096))})
    return {'volumes': volumes, 'blocks': blocks, 'controllers': controllers,
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
    status_started = time.monotonic_ns()
    wpa = ctx.command(['wpa_cli', '-i', 'wlan0', 'status']) if radio else {'output': None}
    status_duration_ns = time.monotonic_ns() - status_started if radio else None
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
            # A missed control query does not prove a radio disconnect. Preserve
            # the actual failure and latency separately from event counters.
            'supplicant_observation': {'ok': wpa.get('ok') if radio else None,
                                      'reason': wpa.get('reason') if radio else 'radio_off',
                                      'duration_ns': status_duration_ns},
            'association': properties.get('wpa_state'), 'association_id': properties.get('bssid'),
            'ip_addresses': addresses,
            'default_route': default_route, 'dns_ready': dns_ready,
            'rssi_dbm': number(signal_values.get('RSSI')), 'traffic_counters': stats,
            'events': ctx.json('/run/y2/wifi-events.json'), 'reconnect_owner': 'wpa_supplicant',
            'power_save': ctx.json('/run/y2/wifi-power.json'),
            'coexistence': ctx.json('/run/y2/coexistence.json')}


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
    from .boot import RESET_STATUS, evidence_status, reset_cause
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
    dma = None
    sources = ctx.glob('/sys/bus/platform/drivers/y2-usb/*/status')
    if len(sources) == 1:
        text = read(sources[0], limit=8192) or ''
        fields = dict(re.findall(r'\b(transfer|dma_[a-z_]+)=([a-z_]+|-?\d+)\b', text))
        dma = {key: value if key == 'transfer' else number(value) for key, value in fields.items()}
    return {'uptime_seconds': (ctx.read('/proc/uptime') or '').split(' ')[0] or None,
            'versions': ctx.json('/etc/y2linux/versions.json'),
            'boot_history': ctx.json('/data/system/platform/boot.json'),
            'previous_boot_evidence': evidence_status(ctx),
            'application_readiness': ctx.json('/run/y2/application-ready.json'),
            'boot_stages': stages,
            **reset_cause(ctx.read(RESET_STATUS), 'no_qualified_retained_register'),
            'kernel_taint': ctx.integer('/proc/sys/kernel/tainted'),
            'kernel_warning_count': None,
            'kernel_warning_reason': 'no_dedicated_counter; retain_bounded_owner_dmesg_capture',
            'radio_core': ctx.read('/sys/devices/platform/18070000.connectivity/status'),
            'watchdogs': [{f: read(p / f) for f in ('identity', 'state', 'bootstatus', 'status', 'timeout')}
                          for p in ctx.glob('/sys/class/watchdog/watchdog*')],
            'pstore_files': [p.name for p in ctx.glob('/sys/fs/pstore/*')],
            'rtc': rtcs, 'time': clock, 'ssh': ssh,
            'entropy_available_bits': ctx.integer('/proc/sys/kernel/random/entropy_avail'),
            'crng_ready': clock['entropy_ready'], 'usb': {'mode': 'peripheral', 'udcs': udcs, 'dma': dma},
            'update': update_status(ctx),
            'reborn_supervisor': ctx.json('/data/reborn/logs/supervisor-last.json')}


def readiness(status):
    def item(state, reason=None):
        return {'state': state, 'reason': reason}
    volumes = status['storage']['volumes']
    wifi = status['wifi']
    profile = status['audio']['qualified_profile']
    audio_profile_valid = (isinstance(profile, dict) and
        profile.get('schema') == 'org.y2linux.audio-qualification/v1' and
        profile.get('card') == 'Y2Audio' and profile.get('channels') == 2 and
        isinstance(profile.get('qualified_formats'), list) and 'S16_LE' in profile['qualified_formats'] and
        isinstance(profile.get('qualified_rates'), list) and any(type(rate) is int and rate == 44100
                                                               for rate in profile['qualified_rates']))
    return {
        'storage': item(('Ready' if all(v['space_state'] == 'Normal' for v in volumes[:2]) else 'Degraded')
                        if all(v['state'] == 'Ready' for v in volumes[:2]) else 'Failed',
                        next((v.get('reason') for v in volumes[:2] if v['state'] != 'Ready'), None)),
        'wifi': item({'Online': 'Ready', 'Off': 'Unavailable', 'Failed': 'Failed',
                      'Authenticated': 'Degraded'}.get(wifi['state'], 'Starting'), wifi['reason']),
        'bluetooth': item(status['bluetooth']['state'], status['bluetooth'].get('reason')),
        'audio': item('Ready' if re.search(r'^\s*\d+\s+\[Y2Audio\s*\]', status['audio']['cards'] or '', re.M)
                      and audio_profile_valid
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
                        'headphone_jack_present': None,
                        'headphone_jack_reason': 'codec_irq_not_qualified',
                        'qualified_profile': ctx.json('/etc/y2linux/audio-qualified.json'),
                        'enabled_profile': ctx.json('/etc/y2linux/audio-enabled.json'),
                        'hw_params': {str(p.relative_to(ctx.root)): read(p)
                                      for p in ctx.glob('/proc/asound/card*/pcm*/sub*/hw_params')}}}
    result['readiness'] = readiness(result)
    result['record']['result'] = 'observed'
    result['record']['units'] = {'collection_duration': 'ns'}
    result['record']['collection_duration_ns'] = time.monotonic_ns() - start
    return result
