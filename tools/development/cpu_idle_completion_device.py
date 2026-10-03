#!/usr/bin/env python3
"""Device half of the CPU qualification. Only supported kernel/user APIs."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import struct
import subprocess
import sys
import time
import wave

P = Path
CPU = P('/sys/devices/system/cpu')
POLICY = CPU/'cpufreq/policy0'
COORD = P('/sys/module/system_idle/parameters/enabled')
BUDGET = P('/sys/module/spm/parameters/dormant_budget')
C3 = CPU/'cpu0/cpuidle/state2/disable'
BACKSTOP = P('/sys/firmware/y2_pm/backstop_s')


def read(path):
    try:
        return P(path).read_text().strip()
    except OSError:
        return None


def write(path, value):
    P(path).write_text(str(value)+'\n')


def fields(text):
    return dict(re.findall(r'(\w+)=([^\s]+)', text or ''))


def number(value):
    return int(value, 0) if value is not None else None


def boot():
    return read('/proc/sys/kernel/random/boot_id')


def states():
    return {str(p.relative_to(CPU)): {n: read(p/n) for n in
            ('name', 'disable', 'usage', 'time', 'rejected')}
            for p in CPU.glob('cpu[0-9]*/cpuidle/state*')}


def params(module):
    return {p.name: read(p) for p in P('/sys/module', module, 'parameters').glob('*')}


def spm(name='state'):
    paths = list(P('/sys/bus/platform/devices').glob('*.spm/'+name))
    if not paths:
        paths = list(P('/sys/bus/platform/drivers/y2-spm').glob('*/'+name))
    if len(paths) != 1:
        raise RuntimeError('SPM diagnostic unavailable/ambiguous: '+name)
    return read(paths[0])


def metrics():
    stat = read('/proc/stat') or ''
    cpu = next(x for x in stat.splitlines() if x.startswith('cpu '))
    irq = next(x for x in stat.splitlines() if x.startswith('intr '))
    ctxt = next(x for x in stat.splitlines() if x.startswith('ctxt '))
    return {'monotonic_ns': time.monotonic_ns(),
            'raw_ns': time.clock_gettime_ns(time.CLOCK_MONOTONIC_RAW),
            'cpu': list(map(int, cpu.split()[1:])), 'irq': int(irq.split()[1]),
            'ctxt': int(ctxt.split()[1]), 'online': read(CPU/'online'),
            'opp': read(POLICY/'scaling_cur_freq'), 'states': states(),
            'opp_residency': read(POLICY/'stats/time_in_state'),
            'thermal': {p.parent.name: read(p) for p in P('/sys/class/thermal').glob('thermal_zone*/temp')}}


def snapshot():
    return {'boot': boot(), 'metrics': metrics(), 'taint': read('/proc/sys/kernel/tainted'),
            'cirq': fields(read('/sys/devices/platform/10204000.interrupt-latch/state')),
            'modules': {m: params(m) for m in
                        ('idle', 'clocks', 'local_timer', 'cirq', 'system_idle', 'cpu_dvfs', 'pwrap', 'mm_clocks')},
            'spm': spm(), 'preflight': spm('dormant_preflight'),
            'mmc': {p.parent.name: read(p) for p in P('/sys/bus/platform/devices').glob('*.mmc/y2_runtime_pm')},
            'ext4': {p.parent.name: read(p) for p in P('/sys/fs/ext4').glob('*/errors_count')},
            'interrupts': read('/proc/interrupts'), 'timer_list': read('/proc/timer_list'),
            'clock_summary': read('/sys/kernel/debug/clk/clk_summary'),
            'journal': {n: read('/sys/firmware/y2_pm/'+n) for n in
                        ('state', 'previous', 'devices', 'devices_previous', 'backstop', 'retention', 'reset_status')}}


def delta(a, b, key):
    if a.get(key) is None or b.get(key) is None:
        raise RuntimeError('missing counter '+key)
    return number(b[key])-number(a[key])


def dormant_verdict(a, b):
    """A reset return alone is insufficient: require all checked restores."""
    changes = {n: delta(a, b, n) for n in
               ('dormant_entries', 'dormant_resumes', 'dormant_successes',
                'dormant_residency_us', 'dormant_restore_failures', 'dormant_failures')}
    return {'pass': changes['dormant_entries'] == changes['dormant_resumes'] ==
                    changes['dormant_successes'] == 1 and
                    changes['dormant_residency_us'] > 0 and
                    changes['dormant_restore_failures'] == changes['dormant_failures'] == 0 and
                    number(b.get('dormant_broken')) == 0 and number(b.get('dormant_wake_result')) == 0,
            'changes': changes}


class Qualification:
    def __init__(self, config):
        self.config = config
        self.directory = P(config['device_directory'])
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.result = {'schema': 'org.y2linux.cpu-idle-qualification/v1',
                       'start_boot': boot(), 'source': json.loads(read('/etc/y2linux/versions.json')),
                       'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                       'phases': {}, 'commands': [], 'done': False}

    def save(self):
        temporary = self.directory/'progress.tmp'
        with temporary.open('w') as f:
            json.dump(self.result, f, indent=2)
            f.flush()
            os.fsync(f.fileno())
        temporary.replace(self.directory/'progress.json')

    def cmd(self, *args, timeout=30):
        start = time.monotonic()
        r = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
        self.result['commands'].append({'host_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                                       'boot': boot(), 'command': args, 'output': r.stdout,
                                       'stderr': r.stderr, 'exit': r.returncode,
                                       'duration_s': time.monotonic()-start})
        if r.returncode:
            raise RuntimeError('command failed: '+repr(args))
        return r.stdout

    def ctl(self, *args):
        return json.loads(self.cmd('rebornctl', *args, '--json'))

    def screen(self, off):
        if bool(self.ctl('status')['screen_off']) != off:
            target = next(p for p in P('/sys/class/input').glob('event*/device/name')
                          if 'pmic' in p.read_text().lower())
            fd = os.open('/dev/input/'+target.parent.parent.name, os.O_WRONLY)
            try:
                for value in (1, 0):
                    os.write(fd, struct.pack('llHHi', 0, 0, 1, 116, value))
                    os.write(fd, struct.pack('llHHi', 0, 0, 0, 0, 0))
                    time.sleep(.015)
            finally:
                os.close(fd)
            time.sleep(1)
        if bool(self.ctl('status')['screen_off']) != off:
            raise RuntimeError('Reborn screen transition failed')

    def phase(self, name, function):
        self.result['stage'] = name
        self.save()
        try:
            value = function()
            self.result['phases'][name] = value
            if value.get('pass') is not True:
                raise RuntimeError('qualification failed: '+name)
        except Exception as e:
            self.result['phases'].setdefault(name, {'pass': False, 'error': repr(e)})
            self.save()
            raise
        self.save()

    def window(self, seconds):
        a = metrics()
        time.sleep(seconds)
        b = metrics()
        elapsed = (b['monotonic_ns']-a['monotonic_ns'])/1e9
        ticks = sum(b['cpu'])-sum(a['cpu'])
        idle = sum(b['cpu'][3:5])-sum(a['cpu'][3:5])
        return {'before': a, 'after': b, 'seconds': elapsed,
                'cpu_util_percent': 100*(ticks-idle)/ticks if ticks else None,
                'irq_per_s': (b['irq']-a['irq'])/elapsed,
                'ctxt_per_s': (b['ctxt']-a['ctxt'])/elapsed,
                'monotonic_raw_drift_ns': (b['monotonic_ns']-b['raw_ns'])-(a['monotonic_ns']-a['raw_ns'])}

    def c1(self):
        write(COORD, 'N')
        time.sleep(2)
        affinity = os.sched_getaffinity(0)
        samples = {}
        before = states()
        try:
            for cpu in sorted(affinity):
                os.sched_setaffinity(0, {cpu})
                latencies = []
                for _ in range(100):
                    t = time.monotonic_ns()
                    time.sleep(.001)
                    latencies.append((time.monotonic_ns()-t)/1e6)
                samples[str(cpu)] = sorted(latencies)
            os.sched_setaffinity(0, affinity)
            window = self.window(12)
            after = states()
            rows = {k: {'entries': delta(v, after[k], 'usage'),
                        'residency_us': delta(v, after[k], 'time'),
                        'rejected': delta(v, after[k], 'rejected'), 'enabled': after[k]['disable'] == '0'}
                    for k, v in before.items() if v['name'] == 'WFI'}
            return {'pass': len(rows) == len(affinity) and all(
                    r['entries'] > 0 and r['residency_us'] > 0 and r['enabled'] and r['rejected'] == 0
                    for r in rows.values()) and all(x[95] < 3 for x in samples.values()) and
                    abs(window['monotonic_raw_drift_ns']) < 5000000, 'cores': rows, 'latencies_ms': samples, 'window': window}
        finally:
            os.sched_setaffinity(0, affinity)

    def hotplug(self):
        rows = []
        try:
            for cycle in range(3):
                for online, order in ((0, (3, 2, 1)), (1, (1, 2, 3))):
                    for cpu in order:
                        write(CPU/('cpu%d/online' % cpu), online)
                        time.sleep(.25)
                        power = spm()
                        copies = fields(power).get('power', '').split('/')
                        # Exact MT6582 FC1/FC2/FC3 mapping, confirmed by
                        # same-boot physical hotplug: CPU3 clears bit9.
                        bit = {1: 0x800, 2: 0x400, 3: 0x200}[cpu]
                        if len(copies) != 2 or any(bool(number(v) & bit) != bool(online) for v in copies):
                            raise RuntimeError('hotplug power copies disagree: '+power)
                        rows.append({'cycle': cycle, 'cpu': cpu, 'online': online,
                                     'topology': read(CPU/'online'), 'power': power})
            return {'pass': read(CPU/'online') == '0-3', 'transitions': rows}
        finally:
            for cpu in (1, 2, 3):
                write(CPU/('cpu%d/online' % cpu), 1)

    def dvfs(self):
        frequencies = [598000, 747500, 1040000, 1196000, 1300000]
        available = set(map(int, read(POLICY/'scaling_available_frequencies').split()))
        if not set(frequencies) <= available:
            raise RuntimeError('guarded OPP missing')
        rows = []
        for f in frequencies:
            write(POLICY/'scaling_min_freq', 598000)
            write(POLICY/'scaling_max_freq', f)
            write(POLICY/'scaling_min_freq', f)
            time.sleep(1)
            if any(number(v) >= 75000 for v in metrics()['thermal'].values() if v is not None):
                raise RuntimeError('bounded OPP thermal guard reached')
            rows.append({'frequency': f, 'actual': number(read(POLICY/'scaling_cur_freq')),
                         'voltage_uv': number(read('/sys/module/pwrap/parameters/cpu_voltage_uv')),
                         'dvfs': params('cpu_dvfs'), 'pwrap': params('pwrap'), 'thermal': metrics()['thermal']})
        return {'pass': all(r['actual'] == r['frequency'] and r['voltage_uv'] ==
                           (1150000 if r['frequency'] <= 1040000 else
                            1200000 if r['frequency'] == 1196000 else 1250000) for r in rows), 'opps': rows}

    def storage(self):
        before = snapshot()
        rows = []
        directories = [P('/data')]
        if os.path.ismount('/media/sd'):
            directories.append(P('/media/sd'))
        for directory in directories:
            file = directory/('.cpu-idle-integrity-'+self.directory.name)
            data = hashlib.sha256(str(file).encode()).digest()*4096
            expected = hashlib.sha256(data).hexdigest()
            try:
                for n in range(8):
                    with file.open('wb') as f:
                        f.write(data); f.flush(); os.fsync(f.fileno())
                        if hasattr(os, 'posix_fadvise'):
                            os.posix_fadvise(f.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
                    actual = hashlib.sha256(file.read_bytes()).hexdigest()
                    rows.append({'media': str(directory), 'round': n, 'sha256': actual})
                    if actual != expected:
                        raise RuntimeError('storage checksum mismatch')
            finally:
                file.unlink(missing_ok=True)
        after = snapshot()
        return {'pass': before['ext4'] == after['ext4'] and all(
                number(fields(x).get('error')) == 0 and number(fields(x).get('last_mismatch')) == 0
                for x in after['mmc'].values()), 'before': before, 'after': after, 'rounds': rows,
                'sd_absent': not os.path.ismount('/media/sd')}

    def parking_c2(self):
        self.screen(True)
        self.cmd('y2-radio', 'wifi', 'off-runtime')
        self.cmd('y2-radio', 'bluetooth', 'off-runtime')
        write(COORD, 'Y')
        trace = []
        end = time.monotonic()+1200
        while time.monotonic() < end:
            trace.append({'online': read(CPU/'online'), 'coordinator': params('system_idle'), 'time': time.monotonic()})
            self.result['parking_progress'] = trace[-1]
            self.save()
            if trace[-1]['online'] == '0' and number(params('system_idle').get('parked_mask')) == 0xe:
                break
            time.sleep(10)
        else:
            raise RuntimeError('natural parking not eligible: '+repr(trace[-1]))
        a = snapshot()
        window = self.window(60)
        b = snapshot()
        key = next(k for k, v in a['metrics']['states'].items() if k.startswith('cpu0/') and v['name'] == 'SLIDLE')
        entries = delta(a['metrics']['states'][key], b['metrics']['states'][key], 'usage')
        residency = delta(a['metrics']['states'][key], b['metrics']['states'][key], 'time')
        clk = b['modules']['clocks']
        return {'pass': entries > 0 and residency > 0 and number(clk.get('slow_restore_failures')) == 0 and
                number(b['modules']['idle'].get('slow_failures')) == 0,
                'trace': trace, 'before': a, 'after': b, 'entries': entries, 'residency_us': residency,
                'window': window, 'idle_dmesg': self.cmd('dmesg')}

    def c3_cycles(self):
        self.cmd('sync')
        write(POLICY/'scaling_min_freq', 598000)
        write(POLICY/'scaling_max_freq', 747500)
        role = next(P('/sys/class/usb_role').glob('*/role'))
        original_role = read(role)
        rows = []
        try:
            write(role, 'none')
            time.sleep(6)
            write(BACKSTOP, 10)
            write(BUDGET, 1)
            preflight = spm('dormant_preflight')
            unmet = fields(preflight).get('unmet', '').strip(',')
            if unmet:
                raise RuntimeError('C3 prerequisites: '+unmet+'\n'+preflight)
            near_before = fields(spm())
            write(C3, 0)
            for _ in range(20):
                time.sleep(.001)
            write(C3, 1)
            near_after = fields(spm())
            self.result['near_deadline_fallback'] = {'before': near_before, 'after': near_after}
            if delta(near_before, near_after, 'dormant_entries'):
                raise RuntimeError('too-close deadline entered C3')
            for cycle in range(20):
                # One real cpu_suspend call per cycle. No retry after an entry/restore failure.
                write(BACKSTOP, 10)
                write(BUDGET, 1)
                before = snapshot()
                a = fields(before['spm'])
                self.result['c3_armed'] = {'cycle': cycle, 'before': before}
                self.save()
                deadline = (.025, .05, .1, .2)[cycle % 4]
                write(C3, 0)
                start = time.monotonic_ns()
                time.sleep(deadline)
                write(C3, 1)
                write(BUDGET, 0)
                after = snapshot()
                verdict = dormant_verdict(a, fields(after['spm']))
                key = 'cpu0/cpuidle/state2'
                verdict['cpuidle_entries'] = delta(before['metrics']['states'][key], after['metrics']['states'][key], 'usage')
                verdict['cpuidle_residency_us'] = delta(before['metrics']['states'][key], after['metrics']['states'][key], 'time')
                verdict['pass'] &= (boot() == self.result['start_boot'] and verdict['cpuidle_entries'] == 1 and
                                    verdict['cpuidle_residency_us'] > 0 and after['taint'] == '0' and
                                    number(after['modules']['local_timer']['context_failures']) == 0 and
                                    number(after['cirq']['restore_failures']) == 0 and
                                    number(after['cirq']['clone_failures']) == 0 and
                                    number(after['cirq']['entries']) == number(after['cirq']['flushes']) and
                                    number(fields(after['spm'])['dormant_wake']) & (1 << 4) != 0 and
                                    number(after['modules']['local_timer']['handoff_remaining_ns']) >= 2000000 and
                                    number(after['modules']['local_timer']['context_saves']) ==
                                    number(after['modules']['local_timer']['context_restores']))
                rows.append({'cycle': cycle, 'deadline_s': deadline, 'elapsed_ns': time.monotonic_ns()-start,
                             'before': before, 'after': after, 'verdict': verdict})
                self.result['c3_cycles'] = rows
                self.save()
                if not verdict['pass']:
                    raise RuntimeError('C3 first failed bounded cycle; disabled, no repeat')
            if self.config.get('enable_qualified_runtime'):
                write(BUDGET, -1)
                write(C3, 0)
                window = self.window(60)
                final = snapshot()
                if number(fields(final['spm'])['dormant_broken']):
                    raise RuntimeError('C3 runtime restore failed')
                if delta(fields(rows[-1]['after']['spm']), fields(final['spm']), 'dormant_successes') <= 0:
                    raise RuntimeError('C3 runtime policy did not produce entries')
                self.result['c3_runtime_window'] = window
            return {'pass': True, 'cycles': rows, 'preflight': preflight,
                    'physical_state': 'WORKING_AND_REPEATEDLY_OBSERVED'}
        except Exception:
            write(C3, 1)
            write(BUDGET, 0)
            raise
        finally:
            write(BACKSTOP, 0)
            write(role, original_role)
            time.sleep(8)

    def playback(self):
        previous = self.ctl('status').get('playback', {})
        directory = P('/data/music')/('.CPU-IDLE-qualification-'+self.directory.name)
        directory.mkdir()
        file = directory/'silent-wake.wav'
        try:
            with wave.open(str(file), 'wb') as f:
                f.setparams((2, 2, 44100, 0, 'NONE', 'not compressed'))
                f.writeframes(b'\0'*(44100*4*20))
            self.ctl('scan')
            end = time.monotonic()+60
            track = None
            while time.monotonic() < end:
                with sqlite3.connect('file:/data/reborn/library.db?mode=ro', uri=True) as db:
                    track = db.execute('select id from tracks where path=? and deleted=0', (str(file),)).fetchone()
                if track:
                    break
                time.sleep(1)
            if not track:
                raise RuntimeError('qualification fixture not indexed')
            self.ctl('play', str(track[0]))
            time.sleep(2)
            a = self.ctl('audio')
            time.sleep(8)
            b = self.ctl('audio')
            leases = params('workload')
            ma = a.get('audio', {}).get('metrics', {})
            mb = b.get('audio', {}).get('metrics', {})
            clean = all(ma.get(n) == mb.get(n) for n in
                        ('audio_xruns', 'audio_recoveries', 'ffmpeg_decode_errors', 'playback_errors'))
            return {'pass': self.ctl('status').get('playback', {}).get('state') == 'playing' and clean and
                    mb.get('ffmpeg_frames_decoded', 0) > ma.get('ffmpeg_frames_decoded', 0) and
                    'PlaybackNormal' in (leases.get('leases') or ''),
                    'before': a, 'after': b, 'leases': leases, 'online': read(CPU/'online')}
        finally:
            self.ctl('stop')
            file.unlink(missing_ok=True)
            directory.rmdir()
            self.ctl('scan')
            # Restore the stopped owner's selection through normal control APIs.
            if previous.get('state') == 'stopped' and previous.get('track_id') is not None:
                with sqlite3.connect('file:/data/reborn/library.db?mode=ro', uri=True) as db:
                    valid = db.execute('select id from tracks where id=? and deleted=0',
                                       (previous['track_id'],)).fetchone()
                if valid:
                    self.ctl('play', str(previous['track_id']))
                    self.ctl('stop')

    def run(self):
        if boot() != self.config['expected_boot'] or json.loads(read('/etc/y2linux/versions.json')) != self.config['expected_versions']:
            raise RuntimeError('identity changed before device job; no hardware mutation')
        saved = {'screen': bool(self.ctl('status')['screen_off']), 'coordinator': read(COORD),
                 'policy': {n: read(POLICY/n) for n in ('scaling_governor', 'scaling_min_freq', 'scaling_max_freq')},
                 'c3': read(C3), 'budget': read(BUDGET),
                 'backstop': number(fields(read(BACKSTOP)).get('armed_s')),
                 'radio': self.config.get('original_radio') or {'wifi': P('/sys/class/net/wlan0').exists(),
                           'bluetooth': json.loads(self.cmd('y2-platform', 'status'))['bluetooth'].get('runtime_enabled') is True}}
        if saved['backstop'] is None:
            raise RuntimeError('RGU qualification control missing; no mutation')
        self.result['saved'] = saved
        self.result['initial'] = snapshot()
        self.result['initial_dmesg'] = self.cmd('dmesg')
        self.save()
        c3_pass = False
        try:
            for command in (('uname', '-a'), ('y2-platform', 'status'), ('y2-platform', 'capabilities'),
                            ('y2-platform', 'health'), ('dmesg',)):
                self.cmd(*command)
            write(C3, 1); write(BUDGET, 0)
            timer = params('local_timer')
            if timer.get('ready') != 'Y' or timer.get('events_ready') != 'Y' or timer.get('broadcast_admission') != 'gpt4_sole_owner':
                raise RuntimeError('GPT6/GPT4/PPI29 foundation not admitted')
            if read('/sys/devices/system/clocksource/clocksource0/current_clocksource') != 'arch_sys_counter':
                raise RuntimeError('architectural clocksource unavailable')
            self.phase('C1', self.c1)
            verified = fields(read('/sys/module/local_timer/parameters/cpus'))
            if any(verified.get('cpu%d_cntfrq' % c) != '13000000' or verified.get('cpu%d_error' % c) != '0' for c in range(4)):
                raise RuntimeError('per-core CNTFRQ/PPI29 initialization failed')
            self.phase('hotplug', self.hotplug)
            self.phase('DVFS', self.dvfs)
            write(POLICY/'scaling_min_freq', 598000)
            write(POLICY/'scaling_max_freq', saved['policy']['scaling_max_freq'])
            write(POLICY/'scaling_governor', 'schedutil')
            self.screen(False)
            self.result['screen_on_idle'] = self.window(20)
            self.screen(True)
            self.result['screen_off_before_parking'] = self.window(20)
            self.phase('C2_parking', self.parking_c2)
            self.phase('storage_after_C2', self.storage)
            try:
                self.phase('C3', self.c3_cycles)
                c3_pass = True
            except Exception as e:
                self.result['C3_failure'] = repr(e)
                self.result['C3_failure_snapshot'] = snapshot()
                # Continue independent regressions while keeping the broken path disabled.
            self.phase('storage_after_C3', self.storage)
            self.screen(False)
            time.sleep(2)
            self.phase('screen_wake', lambda: {'pass': read(CPU/'online') == '0-3',
                                              'online': read(CPU/'online'), 'status': self.ctl('status')})
            self.phase('DVFS_after_C3', self.dvfs)
            write(POLICY/'scaling_min_freq', 598000)
            write(POLICY/'scaling_max_freq', saved['policy']['scaling_max_freq'])
            write(POLICY/'scaling_governor', 'schedutil')
            self.phase('playback_workload_wake', self.playback)
        except Exception as e:
            self.result['error'] = repr(e)
        finally:
            cleanup_errors = []
            actions = [(lambda: write(C3, 0 if c3_pass and not self.result.get('error') and self.config.get('enable_qualified_runtime') else 1 if self.result.get('C3_failure') or self.result.get('error') else saved['c3'])),
                       (lambda: write(BUDGET, -1 if c3_pass and not self.result.get('error') and self.config.get('enable_qualified_runtime') else 0 if self.result.get('C3_failure') or self.result.get('error') else saved['budget'])),
                       (lambda: write(BACKSTOP, saved['backstop'])),
                       (lambda: write(POLICY/'scaling_min_freq', 598000)),
                       (lambda: write(POLICY/'scaling_max_freq', saved['policy']['scaling_max_freq'])),
                       (lambda: write(POLICY/'scaling_min_freq', saved['policy']['scaling_min_freq'])),
                       (lambda: write(POLICY/'scaling_governor', saved['policy']['scaling_governor'])),
                       (lambda: self.cmd('y2-radio', 'wifi', 'on-runtime' if saved['radio']['wifi'] else 'off-runtime')),
                       (lambda: self.cmd('y2-radio', 'bluetooth', 'on-runtime' if saved['radio']['bluetooth'] else 'off-runtime')),
                       (lambda: self.screen(saved['screen'])), (lambda: write(COORD, saved['coordinator']))]
            for action in actions:
                try:
                    action()
                except Exception as e:
                    cleanup_errors.append(repr(e))
            self.result['cleanup_errors'] = cleanup_errors
            self.result['final'] = snapshot()
            self.result['final_dmesg'] = self.cmd('dmesg')
            new_lines = self.result['final_dmesg'][len(self.result.get('initial_dmesg', '')):]
            self.result['filesystem_irq_errors'] = [line for line in new_lines.splitlines() if re.search(
                r'EXT4-fs error|Buffer I/O error|mmc.*(?:timeout|CRC error)|irq.*nobody cared|BUG:|Oops:', line, re.I)]
            self.result['end_boot'] = boot()
            self.result['done'] = True
            self.result['pass'] = not self.result.get('error') and c3_pass and not cleanup_errors and boot() == self.result['start_boot'] and not self.result['filesystem_irq_errors']
            if not self.result['pass']:
                write(C3, 1); write(BUDGET, 0)
            self.save()


if __name__ == '__main__':
    os.umask(0o077)
    Qualification(json.loads(P(sys.argv[1]).read_text())).run()
