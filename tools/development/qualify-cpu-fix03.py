#!/usr/bin/env python3
"""One owner-run CPU Final Fix03 qualification over authenticated SSH.

Default prints the plan and contacts nothing. --run executes it against the
installed candidate using the owner's existing SSH configuration (pinned host
keys, key-only accounts). --wifi-host is required: it is the independent
observer for USB stress and the transport for recovery evidence. Never flashes.
Only --allow-warm-reboot lets it issue one ordinary `reboot` to prove SRAM
retention; it never reboots otherwise. All receipts are private.

Order: awake checks first (identity, timers, QoS, MMC runtime gating, automatic
parking, SLIDLE with radios runtime-off, PWRAP/high OPP, USB loaded stress with
Wi-Fi observer, SRAM journal self-test). Only if they pass: charger-refusal
regression, backstopped staged freezer/devices/platform/processors/core, full
RTC suspend, then Power wake, then five RTC cycles.

Fix03 harness corrections from the Fix02 run: Back (158) drives the QoS input;
parking and SLIDLE entries are separate verdicts; the warm reboot falls back to
the Wi-Fi host; per-process CPU percent is no longer 10x too large; charger
refusal is read from the journal's failed stage, not a scrolled ring entry;
a launch step runs on exactly one probed host and is never retried.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time
import uuid

PLAN = [
    'identity: installed build/reborn/kernel/rootfs match the package manifest',
    'timers: GPT6/GPT4/PPI29/CNTFRQ admission, arch_sys_counter, highres, NO_HZ, 1-ms sleeps',
    'QoS: real Reborn input/playback/scan leases and screen-off interactive release',
    'MMC: eMMC/SD runtime autosuspend gates PERI clocks, I/O resumes with verified data',
    'coordinator: screen-off sustained idle parks CPU3->2->1, display wake restores without escalating the hold',
    'SLIDLE: radios runtime-off, parked, C2 entries with the bus-DCM baseline (0x00/0x0f) accepted',
    'PWRAP: raw readiness operands; if admitted, 1196/1300 MHz with 1.20/1.25 V readback',
    'USB: repeated loaded 1-MiB bidirectional transfers observed independently over Wi-Fi',
    'journal: awake SRAM self-test (optional: one ordinary warm reboot proves retention)',
    '--- only if all awake checks passed ---',
    'charger: classify USB-present vs charging-active; refusal expected only when active',
    'staged PM with RGU backstop: freezer, devices, platform, processors, core',
    'full RTC suspend with same boot ID, then Power wake (one deliberate press), then five RTC cycles',
    'any backstop reset: retained stage marks with ms stamps name the stalled resume call; RGU cause decoded',
    'post-resume: CPUs/timers/storage/USB/Wi-Fi/display/audio/Reborn restoration',
]

# Device-side helpers. Plain Python on the Y2; imports the installed platform
# package only for the shared charger classifier and status snapshot.
REMOTE = r'''
import json,os,pathlib,subprocess,time,struct,sqlite3,threading,sys,hashlib,re
P=pathlib.Path
sys.path.insert(0,'/usr/lib/y2-platform')

def read(path,limit=65536):
 try:return P(path).read_text()[:limit].strip()
 except OSError:return None

def write(path,value):
 P(path).write_text(value)

def ctl(*args):
 return json.loads(subprocess.check_output(['rebornctl',*args,'--json'],timeout=15))

def snapshot():
 return json.loads(subprocess.check_output(['y2-platform','status','cpu'],timeout=20))['cpu']

def boot():return read('/proc/sys/kernel/random/boot_id')

def key(name,code):
 devices=list(P('/sys/class/input').glob('event*/device/name'))
 target=next(p for p in devices if name in p.read_text())
 fd=os.open('/dev/input/'+target.parent.parent.name,os.O_WRONLY)
 try:
  for value in (1,0):
   os.write(fd,struct.pack('llHHi',0,0,1,code,value))
   os.write(fd,struct.pack('llHHi',0,0,0,0,0));time.sleep(.015)
 finally:os.close(fd)

def screen(off):
 if bool(ctl('status').get('screen_off'))!=off:key('pmic',116)
 time.sleep(.5)
 return ctl('status').get('screen_off')

def processes():
 out={}
 for p in P('/proc').glob('[0-9]*/stat'):
  try:
   raw=p.read_text();name=raw[raw.index('(')+1:raw.rindex(')')];f=raw[raw.rindex(')')+2:].split()
   out[p.parent.name]={'name':name,'ticks':int(f[11])+int(f[12])}
  except (OSError,ValueError,IndexError):pass
 return out

def mmc_state():
 return {p.parent.name:read(p) for p in P('/sys/bus/platform/devices').glob('*.mmc/y2_runtime_pm')}

def ext4_errors():
 return {p.parent.name:read(p) for p in P('/sys/fs/ext4').glob('*/errors_count')}

def charger():
 from y2_platform.charger_state import observe
 from y2_platform.common import Context
 return observe(Context('/'))

def usb_state():
 status={p.parent.name:read(p) for p in P('/sys/bus/platform/drivers/y2-usb').glob('*/status')}
 net={n:read('/sys/class/net/usb0/'+n) for n in ('carrier','operstate')}
 stats={p.name:read(p) for p in P('/sys/class/net/usb0/statistics').glob('*')}
 udc={p.parent.name:read(p) for p in P('/sys/class/udc').glob('*/state')}
 rt={str(p):read(p) for p in P('/sys/bus/platform/devices').glob('musb-hdrc*/power/runtime_status')}
 irq=[l for l in (read('/proc/interrupts') or '').splitlines() if 'musb' in l or 'mc' in l.split()[-1:]]
 dm=subprocess.run(['dmesg'],capture_output=True,text=True).stdout.splitlines()[-80:]
 return {'status':status,'net':net,'stats':stats,'udc':udc,'runtime':rt,'irq':irq,'dmesg':dm,
  'journal_devices':read('/sys/firmware/y2_pm/devices',16384),'boot':boot()}

def sleep_latency():
 rows=[]
 for cpu in sorted(os.sched_getaffinity(0)):
  os.sched_setaffinity(0,{cpu});samples=[]
  for _ in range(100):
   t=time.monotonic_ns();time.sleep(.001);samples.append((time.monotonic_ns()-t)/1e6)
  samples.sort();rows.append({'cpu':cpu,'median_ms':samples[50],'p95_ms':samples[95]})
 os.sched_setaffinity(0,set(range(os.cpu_count())))
 return rows
'''


def field(text, name):
    match = re.search(r'(?:^|\s)' + re.escape(name) + r'=(\S+)', text or '')
    return match.group(1) if match else None


def number(text, name):
    value = field(text, name)
    try:
        return int(value, 0) if value is not None else None
    except ValueError:
        return None


def mmc_verdict(before, after, io_after):
    """Runtime gating: both hosts suspended/gated at rest, I/O resumed them,
    no resume errors or restore failures. Values are y2_runtime_pm lines."""
    rows = {}
    ok = bool(before) and set(before) == set(after) == set(io_after)
    for name in sorted(before or {}):
        b, a, i = before[name], after.get(name), io_after.get(name)
        row = {'gated_at_rest': number(a, 'gated') == 1,
               'suspends_increase': (number(a, 'suspends') or 0) > (number(b, 'suspends') or 0),
               'resumed_for_io': (number(i, 'resumes') or 0) > (number(a, 'resumes') or 0),
               'error': number(i, 'error'), 'restored': number(i, 'restored')}
        rows[name] = row
        ok = ok and row['gated_at_rest'] and row['suspends_increase'] and not row['error']
    return {'pass': ok, 'hosts': rows}


def coordinator_verdict(state, idle, online):
    """Sustained idle parked every secondary. SLIDLE entries are reported but
    judged separately (radios on keep APDMA/BTIF as legitimate blockers)."""
    parked = number(state, 'parked_mask')
    try:
        entries = int((idle or {}).get('slow_entries') or 0)
    except ValueError:
        entries = 0
    return {'pass': parked == 0xe and online == '0',
            'parked_mask': parked, 'online': online, 'slidle_entries': entries,
            'last_reset': field(state, 'last_reset'), 'load_mc': number(state, 'load_mc'),
            'high_freq_permille': number(state, 'high_freq_permille')}


def wake_hold_verdict(state):
    """A display wake must not leave an escalated parking hold behind."""
    return {'pass': number(state, 'hold_ms') == 60000 and number(state, 'pressure_restores') == 0,
            'hold_ms': number(state, 'hold_ms'), 'pressure_restores': number(state, 'pressure_restores'),
            'wake_reclassified': number(state, 'wake_reclassified'), 'last_reset': field(state, 'last_reset')}


def slidle_verdict(samples):
    """Radios runtime-off: SLIDLE entries advance and the bus is never refused
    while the clock mask is zero."""
    def value(sample, group, name):
        try:
            return int((sample.get(group) or {}).get(name) or 0)
        except ValueError:
            return 0
    if not samples:
        return {'pass': False, 'reason': 'no_samples'}
    first, last = samples[0], samples[-1]
    entries = value(last, 'idle', 'slow_entries') - value(first, 'idle', 'slow_entries')
    bus = value(last, 'clk', 'slow_reject_bus') - value(first, 'clk', 'slow_reject_bus')
    return {'pass': entries > 0 and bus == 0, 'slidle_entries': entries, 'bus_rejects': bus,
            'restore_failures': value(last, 'clk', 'slow_restore_failures'),
            'clock_rejects': value(last, 'clk', 'slow_reject_clock') - value(first, 'clk', 'slow_reject_clock')}


def background_attribution(before, after, seconds, top=12):
    """CPU ticks per process over a window (USER_HZ=100) for source attribution."""
    rows = []
    for pid, row in (after or {}).items():
        start = (before or {}).get(pid)
        delta = row['ticks'] - (start['ticks'] if start and start['name'] == row['name'] else 0)
        if delta > 0:
            rows.append({'pid': pid, 'name': row['name'], 'cpu_ms': delta * 10,
                         'new_process': start is None, 'percent_of_one_core': round(delta * 10 * 100 / (max(seconds, 1) * 1000), 3)})
    rows.sort(key=lambda r: -r['cpu_ms'])
    spawned = sum(1 for pid in (after or {}) if pid not in (before or {}))
    return {'top': rows[:top], 'processes_started': spawned, 'window_s': seconds}


def open_callback(ring_text):
    """Last device PM callback that entered without leaving, from y2_pm/devices."""
    entered = {}
    for line in (ring_text or '').splitlines():
        m = re.match(r'(\d+) phase=(\d+) (enter|leave) result=(-?\d+)(?: ms=\d+)? device=(.*)$', line)
        if not m:
            continue
        key = (m.group(2), m.group(5))
        if m.group(3) == 'enter':
            entered[key] = int(m.group(1))
        else:
            entered.pop(key, None)
    if not entered:
        return None
    (phase, device), sequence = max(entered.items(), key=lambda item: item[1])
    return {'phase': int(phase), 'device': device, 'sequence': sequence}


def refused_prepare(ring_text):
    """Devices whose prepare callback returned -EBUSY in the retained ring."""
    return [m.group(2) for m in re.finditer(r'^\d+ phase=1 leave result=(-16)(?: ms=\d+)? device=(.*)$', ring_text or '', re.M)]


def charger_refused(journal, ring_text, dmesg):
    """The prepare refusal is the journal's first failure; the 24-entry ring
    may have scrolled past it, so it is supporting evidence only."""
    by_stage = field(journal, 'failed_stage') == 'DPM_PREPARED' and number(journal, 'error') == -16
    by_ring = any('charger' in name for name in refused_prepare(ring_text))
    by_log = bool(re.search(r'charger.*(-16|busy|refus)', dmesg or '', re.I))
    return {'refused': by_stage or by_ring, 'journal_failed_stage': by_stage,
            'ring_prepare': by_ring, 'dmesg': by_log}


def stage_marks(ring_text):
    """Timed stage marks (phase 254) in ring order: [(stage, ms, result)]."""
    return [(m.group(3), int(m.group(2)), int(m.group(1))) for m in
            re.finditer(r'^\d+ phase=254 mark result=(-?\d+) ms=(\d+) stage=(\S+)$', ring_text or '', re.M)]


def resume_boundary(ring_text):
    """Name the last completed resume stage and the stall after it."""
    order = ['DEVICES_RESUMING', 'DEVICES_RESUMED', 'CONSOLE_RESUMED', 'PLATFORM_ENDED',
             'TASKS_THAWED', 'FILESYSTEMS_THAWED', 'POST_SUSPEND_NOTIFIED', 'CONSOLE_RESTORED', 'EXIT']
    calls = {'DEVICES_RESUMING': 'dpm_resume_end', 'DEVICES_RESUMED': 'console_resume_all',
             'CONSOLE_RESUMED': 'platform_resume_end', 'PLATFORM_ENDED': 'suspend_thaw_processes',
             'TASKS_THAWED': 'filesystems_thaw', 'FILESYSTEMS_THAWED': 'pm_notifier_call_chain(PM_POST_SUSPEND)',
             'POST_SUSPEND_NOTIFIED': 'pm_restore_console', 'CONSOLE_RESTORED': 'mutex_unlock/pm_suspend return'}
    marks = stage_marks(ring_text)
    last = next((m for m in reversed(marks) if m[0] in order), None)
    if not last:
        return {'last_stage': None, 'stalled_call': None, 'marks': marks}
    return {'last_stage': last[0], 'last_ms': last[1], 'stalled_call': calls.get(last[0]),
            'open_device_callback': open_callback(ring_text), 'marks': marks}


def receipt_verdict(result_line, expected_boot):
    """rc boot_before boot_after taint_before taint_after from the device."""
    rc, before, after, taint_before, taint_after = result_line.split()
    return {'rc': int(rc), 'same_boot': before == after == expected_boot,
            'taint_unchanged': taint_before == taint_after}


def usb_loss_verdict(failure, observer):
    """Classify a lost USB transfer from the independent Wi-Fi capture."""
    if failure is None:
        return {'lost': False}
    status = ' '.join((observer or {}).get('status', {}).values()) if observer else ''
    reason = field(status, 'fault_reason')
    same_boot = observer is not None and observer.get('boot') == failure.get('boot')
    return {'lost': True, 'observer_alive': observer is not None, 'same_boot': same_boot,
            'fault_reason': reason, 'controller_error': number(status, 'error'),
            'classification': ('usb_transport_' + reason) if reason else
            ('usb_transport_unattributed' if same_boot else 'device_unreachable')}


class Run:
    def __init__(self, args, manifest):
        self.args, self.manifest, self.index = args, manifest, 0
        self.output = args.output
        self.output.mkdir(parents=True, mode=0o700, exist_ok=False)
        self.boot = None
        self.results = {}

    def ssh(self, host, code, timeout):
        return subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                               '-o', 'ConnectTimeout=4', host, 'python3 -'],
                              input=code, text=True, capture_output=True, timeout=timeout)

    def remote(self, body, timeout=40, hosts=None, required=True):
        self.index += 1
        stem = self.output / f'{self.index:03d}'
        code = REMOTE + '\n' + body + '\n'
        stem.with_suffix('.remote.py').write_text(code)
        failures = []
        for host in hosts or [self.args.host, self.args.wifi_host]:
            try:
                result = self.ssh(host, code, timeout)
                stem.with_suffix('.stdout').write_text(result.stdout)
                stem.with_suffix('.stderr').write_text(result.stderr)
                if result.returncode == 0:
                    stem.with_suffix('.host').write_text(host + '\n')
                    return json.loads(result.stdout)
                failures.append({'host': host, 'code': result.returncode, 'stderr': result.stderr[-2000:]})
            except (subprocess.TimeoutExpired, ValueError) as error:
                failures.append({'host': host, 'error': str(error)})
        stem.with_suffix('.failure.json').write_text(json.dumps(failures, indent=2))
        if required:
            raise RuntimeError('SSH step failed; inspect ' + str(stem))
        return None

    def live_host(self):
        """The first host that answers a no-op now; launch steps run there once."""
        for host in [self.args.host, self.args.wifi_host]:
            try:
                if subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                                   '-o', 'ConnectTimeout=4', host, 'true'], capture_output=True, timeout=15).returncode == 0:
                    return host
            except subprocess.TimeoutExpired:
                pass
        raise RuntimeError('no SSH host answers')

    def record(self, label, value, verdict=None):
        if verdict is not None:
            self.results[label] = verdict
        (self.output / (label + '.json')).write_text(json.dumps(value, indent=2) + '\n')
        print(f'{label}: {verdict if verdict is not None else "recorded"}', flush=True)
        return value

    def summary(self, stopped=None):
        (self.output / 'summary.json').write_text(json.dumps(
            {'results': self.results, 'stopped': stopped, 'boot': self.boot}, indent=2) + '\n')

    # --- awake checks ---------------------------------------------------
    def identity(self):
        value = self.remote("print(json.dumps({'versions':json.loads(read('/etc/y2linux/versions.json')),'boot':boot(),'taint':read('/proc/sys/kernel/tainted')}))")
        for name in ('build_git_commit', 'reborn_source_commit', 'kernel_version', 'rootfs_version'):
            if value['versions'][name] != self.manifest[name]:
                raise RuntimeError('Wrong candidate ' + name)
        self.boot = value['boot']
        self.record('identity', value, 'PASS')

    def timers(self):
        value = self.remote("print(json.dumps({'cpu':snapshot(),'sleep':sleep_latency()}))", timeout=60)
        timer = value['cpu']['timer']
        medians = [row['median_ms'] for row in value['sleep']]
        ok = (timer['admission'].get('events_ready') in ('Y', '1') and timer['highres_active'] and
              timer['no_hz_active'] and timer['clocksource'] == 'arch_sys_counter' and max(medians) < 2.0)
        self.record('timers', value, 'PASS' if ok else 'FAIL')
        return ok

    def qos(self):
        value = self.remote(r'''
initial=ctl('status');screen(False);samples=[];stop=threading.Event()
def poll():
 while not stop.wait(.02):samples.append(read('/sys/module/workload/parameters/leases'))
t=threading.Thread(target=poll);t.start()
try:
 key('navigation',158);time.sleep(.1);interactive=read('/sys/module/workload/parameters/leases')
 ctl('scan');time.sleep(3);scan=read('/sys/module/workload/parameters/leases')
 screen(True);time.sleep(.3);off=read('/sys/module/workload/parameters/leases')
finally:
 stop.set();t.join()
print(json.dumps({'interactive':interactive,'scan':scan,'screen_off':off,'samples':samples[-50:]}))''', timeout=60)
        ok = 'Interactive' in (value['interactive'] or '') and 'Interactive' not in (value['screen_off'] or '')
        self.record('qos', value, 'PASS' if ok else 'FAIL')
        return ok

    def mmc(self):
        value = self.remote(r'''
screen(True);before=mmc_state();errors=ext4_errors();time.sleep(8);rest=mmc_state()
data=os.urandom(1<<20);path=P('/data/system/platform/fix03-mmc-'+os.urandom(6).hex())
try:
 with path.open('xb') as f:f.write(data);f.flush();os.fsync(f.fileno())
 fd=os.open(path,os.O_RDONLY)
 try:os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED)
 finally:os.close(fd)
 ok=hashlib.sha256(path.read_bytes()).hexdigest()==hashlib.sha256(data).hexdigest()
finally:path.unlink(missing_ok=True)
sd=None
if os.path.ismount('/media/sd'):
 q=P('/media/sd/.fix03-'+os.urandom(6).hex())
 try:
  q.write_bytes(data[:65536]);os.sync();sd=q.read_bytes()==data[:65536]
 finally:q.unlink(missing_ok=True)
io=mmc_state();time.sleep(2)
print(json.dumps({'before':before,'rest':rest,'io':io,'after_io_rest':mmc_state(),'data_ok':ok,'sd_ok':sd,
 'ext4_before':errors,'ext4_after':ext4_errors(),'clk':read('/sys/kernel/debug/clk/clk_summary',65536),
 'dmesg_mmc':[l for l in subprocess.run(['dmesg'],capture_output=True,text=True).stdout.splitlines() if 'mmc' in l or 'msdc' in l.lower()][-40:]}))''', timeout=90)
        verdict = mmc_verdict(value['before'], value['rest'], value['io'])
        ok = verdict['pass'] and value['data_ok'] and value['sd_ok'] is not False and value['ext4_before'] == value['ext4_after']
        self.record('mmc-runtime-gating', {**value, 'verdict': verdict}, 'PASS' if ok else 'FAIL')
        return ok

    def slidle(self):
        # One SSH at each end of the window; the host stays silent in between
        # so the observer does not become the measured background.
        window = 240
        start = self.remote("screen(True);print(json.dumps({'cpu':snapshot(),'procs':processes(),'t':time.monotonic()}))")
        time.sleep(window)
        end = self.remote("print(json.dumps({'cpu':snapshot(),'procs':processes(),'t':time.monotonic()}))")
        cpu = end['cpu']
        verdict = coordinator_verdict(cpu['system_idle'].get('state'), cpu['idle_diagnostics'], cpu['online'])
        wake = self.remote("t=time.monotonic();screen(False);print(json.dumps({'online':read('/sys/devices/system/cpu/online'),'elapsed_s':time.monotonic()-t,'state':read('/sys/module/system_idle/parameters/state')}))")
        verdict['display_restore'] = wake['online'] == '0-3'
        verdict['wake_hold'] = wake_hold_verdict(wake['state'])
        attribution = background_attribution(start['procs'], end['procs'], end['t'] - start['t'])
        ok = verdict['pass'] and verdict['display_restore'] and verdict['wake_hold']['pass']
        self.record('coordinator-parking', {'start': start['cpu'], 'end': cpu, 'wake': wake,
                                            'attribution': attribution, 'verdict': verdict},
                    'PASS' if ok else 'FAIL')
        return ok

    def slidle_radios_off(self, settle=120, samples=8, interval=25):
        """Detached on the device: Wi-Fi/BT runtime-off removes the legitimate
        APDMA/BTIF blockers, so the observer cannot watch live. Collected later."""
        job = r'''
B=P('/sys/devices/system/cpu');out={'samples':[],'commands':[]}
def cmd(a):
 r=subprocess.run(a,text=True,capture_output=True,timeout=40);out['commands'].append({'a':a,'rc':r.returncode,'o':r.stdout[-400:],'e':r.stderr[-400:]})
def s(tag):
 out['samples'].append({'tag':tag,'t':time.monotonic(),'online':read(B/'online'),'coord':read('/sys/module/system_idle/parameters/state'),
  'idle':{p.name:read(p) for p in P('/sys/module/idle/parameters').glob('*')},'clk':{p.name:read(p) for p in P('/sys/module/clocks/parameters').glob('slow_*')}})
try:
 cmd(['y2-radio','wifi','off-runtime']);cmd(['y2-radio','bluetooth','off-runtime']);screen(True)
 time.sleep(SETTLE)  # restore hold, quiet window and 3->2->1 parking
 s('start')
 for i in range(SAMPLES):time.sleep(INTERVAL);s('t%d'%(i+1))
finally:
 cmd(['y2-radio','wifi','on-runtime']);cmd(['y2-radio','bluetooth','on-runtime']);time.sleep(3);s('radios_restored')
 out['dormant_preflight']=read('/sys/devices/platform/10006000.power-controller/dormant_preflight')
 P('/run/fix03-slidle.json').write_text(json.dumps(out))
'''.replace('SETTLE', str(settle)).replace('SAMPLES', str(samples)).replace('INTERVAL', str(interval))
        self.remote("P('/run/fix03-job.py').write_text(" + repr(REMOTE + job) + ");P('/run/fix03-slidle.json').unlink(missing_ok=True);"
                    "c=subprocess.Popen(['python3','/run/fix03-job.py'],stdout=open('/run/fix03-job.log','w'),stderr=subprocess.STDOUT,start_new_session=True,stdin=subprocess.DEVNULL);print(json.dumps({'pid':c.pid}))",
                    hosts=[self.args.host])
        time.sleep(settle + samples * interval + 60)
        value = None
        for _ in range(20):
            value = self.remote("print(read('/run/fix03-slidle.json',4<<20) or 'null')", required=False)
            if value:
                break
            time.sleep(10)
        verdict = slidle_verdict(((value or {}).get('samples') or [])[:-1])
        self.record('slidle-radios-off', {'job': value, 'verdict': verdict}, 'PASS' if verdict['pass'] else 'FAIL')
        return verdict['pass']

    def dvfs(self):
        value = self.remote(r'''
cpu=snapshot();rows=[];policy=P('/sys/devices/system/cpu/cpufreq/policy0')
before={n:read(policy/n) for n in ('scaling_governor','scaling_min_freq','scaling_max_freq')}
admitted=int(read('/sys/module/cpu_dvfs/parameters/qualification_max_khz') or 0)
try:
 write(policy/'scaling_governor','performance')
 for f in map(int,(read(policy/'scaling_available_frequencies') or '').split()):
  if f>admitted:continue
  write(policy/'scaling_min_freq','598000');write(policy/'scaling_max_freq',str(f));time.sleep(.5)
  rows.append({'khz':f,'cur':read(policy/'cpuinfo_cur_freq'),'uv':read('/sys/module/pwrap/parameters/cpu_voltage_uv'),'state':read('/sys/module/pwrap/parameters/cpu_voltage_state')})
finally:
 write(policy/'scaling_min_freq','598000');write(policy/'scaling_max_freq',before['scaling_max_freq'])
 write(policy/'scaling_min_freq',before['scaling_min_freq']);write(policy/'scaling_governor',before['scaling_governor'])
print(json.dumps({'readiness':read('/sys/module/pwrap/parameters/pwrap_readiness'),'dvfs':cpu['dvfs_diagnostics'],'rows':rows,'after':read('/sys/module/pwrap/parameters/cpu_voltage_uv')}))''', timeout=60)
        expected = {598000: '1150000', 747500: '1150000', 1040000: '1150000', 1196000: '1200000', 1300000: '1250000'}
        ok = all(row['uv'] == expected.get(row['khz']) and row['cur'] == str(row['khz']) for row in value['rows'])
        admitted = any(row['khz'] > 1040000 for row in value['rows'])
        verdict = 'PASS' if ok and admitted else 'PASS_CONSERVATIVE_ONLY' if ok else 'FAIL'
        self.record('pwrap-dvfs', value, verdict)
        return ok

    def usb_stress(self, rounds=10):
        payload = os.urandom(1 << 20)
        digest = hashlib.sha256(payload).hexdigest()
        observer = self.remote("print(json.dumps(usb_state()))", hosts=[self.args.wifi_host])
        rows, failure = [], None
        for index in range(rounds):
            path = '/data/system/platform/fix03-usb-' + uuid.uuid4().hex
            started = time.monotonic()
            try:
                up = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', self.args.host,
                                     'umask 077; cat > ' + shlex.quote(path) + ' && sync && sha256sum ' + shlex.quote(path)],
                                    input=payload, capture_output=True, timeout=60)
                down = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', self.args.host,
                                       'cat ' + shlex.quote(path) + '; rm -f ' + shlex.quote(path)],
                                      capture_output=True, timeout=60)
                ok = (up.returncode == 0 and up.stdout.decode().split()[0] == digest and
                      down.returncode == 0 and hashlib.sha256(down.stdout).hexdigest() == digest)
            except (subprocess.TimeoutExpired, IndexError) as error:
                ok, up = False, error
            rows.append({'round': index, 'ok': ok, 'seconds': time.monotonic() - started})
            if not ok:
                failure = {'round': index, 'boot': self.boot, 'error': str(up)[-500:]}
                break
        # Independent capture immediately after the stress (or the loss).
        captured = self.remote("print(json.dumps(usb_state()))", hosts=[self.args.wifi_host], required=False)
        verdict = usb_loss_verdict(failure, captured)
        status = ' '.join(((captured or {}).get('status') or {}).values())
        verdict.update({k: number(status, k) for k in ('irq_max_burst', 'irq_max_jiffy', 'irq_progress', 'dma_irqs', 'irqs')})
        self.record('usb-loaded-stress', {'before': observer, 'rows': rows, 'failure': failure,
                                          'observer_after': captured, 'verdict': verdict},
                    'PASS' if not failure else 'FAIL:' + verdict['classification'])
        return not failure

    def journal(self):
        value = self.remote(r'''
before={n:read('/sys/firmware/y2_pm/'+n) for n in ('state','previous','retention')}
try:write('/sys/firmware/y2_pm/selftest','run')
except OSError as error:print(json.dumps({'write_error':str(error)}),file=sys.stderr)
print(json.dumps({'before':before,'selftest':read('/sys/firmware/y2_pm/selftest'),'state':read('/sys/firmware/y2_pm/state'),'devices':read('/sys/firmware/y2_pm/devices',16384)}))''')
        ok = field(value['selftest'], 'result') == 'pass'
        self.record('journal-awake-selftest', value, 'PASS' if ok else 'FAIL')
        retained = None
        if ok and self.args.allow_warm_reboot:
            old = self.boot
            subprocess.run(['ssh', '-o', 'BatchMode=yes', self.live_host(), 'sync; reboot'], timeout=20)
            deadline = time.monotonic() + 240
            while time.monotonic() < deadline:
                time.sleep(10)
                after = self.remote("print(json.dumps({'boot':boot(),'retention':read('/sys/firmware/y2_pm/retention'),'previous':read('/sys/firmware/y2_pm/previous')}))", required=False)
                if after and after['boot'] != old:
                    self.boot = after['boot']
                    retained = field(after['retention'], 'selftest_scratch') == 'retained' and \
                        field(after['retention'], 'previous_stage') == 'SELFTEST_B'
                    self.record('journal-warm-reboot-retention', after, 'PASS' if retained else 'FAIL')
                    break
            else:
                self.record('journal-warm-reboot-retention', None, 'FAIL:no_return')
                retained = False
        return ok, retained

    # --- suspend ---------------------------------------------------------
    def suspend(self, mode, alarm=None, power=False, expect_refusal=False):
        token = uuid.uuid4().hex
        directory = '/data/system/platform/fix03-' + token
        script = '''#!/bin/sh
umask 077
before=$(cat /proc/sys/kernel/random/boot_id); taint_before=$(cat /proc/sys/kernel/tainted)
cat /proc/interrupts > DIR/before-interrupts
sleep 2
grep -q '\\[none\\]' /sys/power/pm_test || exit 2
restore_test() { printf 'none\\n' > /sys/power/pm_test; }
trap restore_test EXIT
printf '%s\\n' MODE > /sys/power/pm_test
printf '30\\n' > /sys/firmware/y2_pm/backstop_s
y2-suspend --owner-qualify
rc=$?
restore_test
dmesg | tail -n 200 > DIR/dmesg
cat /proc/interrupts > DIR/interrupts
cat /sys/firmware/y2_pm/state > DIR/journal; cat /sys/firmware/y2_pm/devices > DIR/devices
printf '%s %s %s %s %s\\n' "$rc" "$before" "$(cat /proc/sys/kernel/random/boot_id)" "$taint_before" "$(cat /proc/sys/kernel/tainted)" > DIR/result
'''.replace('MODE', shlex.quote(mode)).replace('DIR', shlex.quote(directory))
        body = f"directory=P({directory!r});directory.mkdir(mode=0o700);(directory/'run.sh').write_text({script!r})\n"
        if alarm:
            # Its JSON goes to a receipt, never into this step's stdout.
            body += f"alarm=subprocess.run(['y2-platform','rtc-alarm',{str(alarm)!r}],check=True,timeout=15,capture_output=True,text=True);(directory/'rtc-alarm').write_text(alarm.stdout+alarm.stderr)\n"
        body += "log=open(directory/'output','w');subprocess.Popen(['sh',str(directory/'run.sh')],stdout=log,stderr=log,start_new_session=True,stdin=subprocess.DEVNULL);print(json.dumps({'started':str(directory)}))"
        # A launch runs once on one live host: a retry after the suspend has
        # started would issue a second request.
        self.remote(body, hosts=[self.live_host()])
        started, pressed, answer = time.monotonic(), False, None
        deadline = started + (240 if mode == 'none' else 150)
        collect = f"d={directory!r};print(json.dumps({{'boot':boot(),'result':read(d+'/result'),'journal':read(d+'/journal'),'devices':read(d+'/devices',16384),'interrupts':read(d+'/interrupts',262144),'before_interrupts':read(d+'/before-interrupts',262144),'dmesg':read(d+'/dmesg',65536)}}))"
        while time.monotonic() < deadline:
            time.sleep(4)
            if power and not pressed and time.monotonic() - started >= 35:
                input('Power wake test: press Power ONCE now, then press Enter. ')
                pressed = True
            answer = self.remote(collect, required=False, timeout=20)
            if answer and answer['boot'] != self.boot:
                # A new boot is never resume: collect the retained boundary.
                lost = self.remote("print(json.dumps({'boot':boot(),'previous':read('/sys/firmware/y2_pm/previous'),'devices_previous':read('/sys/firmware/y2_pm/devices_previous',16384),'retention':read('/sys/firmware/y2_pm/retention'),'reset_status':read('/sys/firmware/y2_pm/reset_status'),'boot_journal':read('/data/system/platform/boot.json',65536)}))", required=False)
                self.boot = answer['boot']
                ring = (lost or {}).get('devices_previous')
                boundary = {'open_callback': open_callback(ring), 'resume': resume_boundary(ring),
                            'reset_cause': field((lost or {}).get('reset_status'), 'cause')}
                self.record('suspend-' + mode + '-reset', {'retained': lost, 'boundary': boundary}, 'FAIL:new_boot')
                return False
            if answer and answer['result']:
                break
        if not answer or not answer.get('result'):
            if mode == 'none' and not power:
                input('No automatic return. Press Power ONCE, then press Enter here. ')
                for _ in range(30):
                    time.sleep(4)
                    answer = self.remote(collect, required=False, timeout=20)
                    if answer and answer.get('result'):
                        break
            if not answer or not answer.get('result'):
                self.record('suspend-' + mode, {'answer': answer}, 'FAIL:no_return_retained_journal_needed_after_recovery')
                return False
        verdict = receipt_verdict(answer['result'], self.boot)
        ok = verdict['same_boot'] and verdict['taint_unchanged']
        if expect_refusal:
            refused = charger_refused(answer['journal'], answer['devices'], answer['dmesg'])
            ok = ok and verdict['rc'] != 0 and refused['refused']
            verdict['refusal'] = refused
        else:
            ok = ok and verdict['rc'] == 0
        if mode == 'none':
            label = 'mtk-pmic-keys' if power else 'mt6397-rtc'
            verdict['wake_irq_serviced'] = irq_count(answer['interrupts'], label) > irq_count(answer['before_interrupts'], label)
            ok = ok and verdict['wake_irq_serviced'] and (pressed if power else time.monotonic() - started < 200)
        verdict['elapsed_s'] = time.monotonic() - started
        verdict['resume'] = resume_boundary(answer['devices'])
        self.record('suspend-' + ('power' if power else 'rtc' if mode == 'none' else mode), {'answer': answer, 'verdict': verdict},
                    'PASS' if ok else 'FAIL')
        return ok


def irq_count(text, label):
    total = 0
    for line in (text or '').splitlines():
        if label not in line:
            continue
        for part in line.split()[1:]:
            if not part.isdigit():
                break
            total += int(part)
    return total


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--host', default='y2', help='USB SSH alias')
    p.add_argument('--wifi-host', help='independent Wi-Fi SSH alias (required with --run)')
    p.add_argument('--package', type=Path, default=Path('out/y2linux-cpu-final-fix03-candidate'))
    p.add_argument('--output', type=Path, default=Path('out/cpu-fix03-physical-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')))
    p.add_argument('--allow-warm-reboot', action='store_true', help='one ordinary reboot to prove SRAM retention')
    p.add_argument('--run', action='store_true')
    args = p.parse_args()
    if not args.run:
        print('\n'.join(f'{n}. {stage}' for n, stage in enumerate(PLAN, 1)))
        return
    if not args.wifi_host:
        raise SystemExit('--wifi-host is required: it is the independent USB/suspend observer')
    os.umask(0o077)
    run = Run(args, json.loads((args.package / 'manifest.json').read_text()))
    run.identity()
    awake = [run.timers(), run.qos(), run.mmc(), run.slidle(), run.slidle_radios_off(), run.dvfs(), run.usb_stress()]
    selftest, retained = run.journal()
    run.remote("screen(False);print(json.dumps({'ok':True}))", required=False)
    if not (all(awake) and selftest):
        run.summary('awake_checks_failed: suspend not attempted')
        print('Awake checks failed; suspend deliberately not attempted. Receipts: ' + str(run.output))
        return
    if retained is None:
        print('SRAM retention not proven (no --allow-warm-reboot); staged tests rely on the awake self-test and backstop.')
    charger = run.remote("print(json.dumps(charger()))")
    run.record('charger-state', charger, charger['category'])
    if charger['expect_refusal']:
        run.suspend('devices', expect_refusal=True)
        input('Charging is active: unplug USB (Wi-Fi remains the transport), then press Enter. ')
        charger = run.remote("print(json.dumps(charger()))", hosts=[args.wifi_host])
        if charger['expect_refusal']:
            run.summary('charger_still_active')
            return
    else:
        run.record('charger-refusal', charger, 'NOT_APPLICABLE:' + charger['category'])
    for mode in ('freezer', 'devices', 'platform', 'processors', 'core'):
        if not run.suspend(mode):
            run.summary('staged_' + mode + '_failed')
            return
    if not run.suspend('none', alarm=30):
        run.summary('rtc_suspend_failed')
        return
    run.record('post-rtc', run.remote("print(json.dumps({'cpu':snapshot(),'usb':usb_state(),'mmc':mmc_state()}))"))
    if not run.suspend('none', alarm=120, power=True):
        run.summary('power_wake_failed')
        return
    run.record('post-power', run.remote("print(json.dumps({'cpu':snapshot(),'usb':usb_state(),'mmc':mmc_state(),'reborn':ctl('status'),'audio':ctl('audio')}))"))
    for cycle in range(5):
        if not run.suspend('none', alarm=30):
            run.summary('rtc_cycle_%d_failed' % (cycle + 1))
            return
    run.record('post-cycles', run.remote("print(json.dumps({'cpu':snapshot(),'usb':usb_state(),'mmc':mmc_state(),'reborn':ctl('status'),'audio':ctl('audio')}))"))
    run.usb_stress(rounds=3)
    run.summary(None)
    print('Receipts: ' + str(run.output) + '. Inspect counters and journals; no inferred hardware passes.')


if __name__ == '__main__':
    main()
