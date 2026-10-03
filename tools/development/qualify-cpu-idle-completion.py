#!/usr/bin/env python3
"""One owner command after flashing. Default shows the plan without SSH.

Launch mutations once; recover observation through the independently pinned
Wi-Fi alias. The device job can turn both transports off during legitimate
deep idle and restores them in finally. Never flashes, pushes or sends reboot.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import time
import uuid

PLAN = [
    'Exact package/kernel/Linux/Reborn/rootfs/release identity before mutation',
    'GPT6/GPT4/PPI29/CNTFRQ/highres/NO_HZ snapshots and four-core C1 latency/residency',
    'Three normal Linux hotplug cycles and all five guarded OPP readbacks',
    'Screen-on/off CPU, IRQ, context-switch, temperature and idle-residency windows',
    'Natural coordinator parking (up to 20 minutes of genuine quiet/hold expiry)',
    'C2 entries, useful residency, clock restore and fsynced eMMC/installed-SD checksums',
    'C3 individual prerequisites, normal USB role detach and radio runtime-off',
    'One RGU-backstopped timer-wake C3 entry, then 19 further bounded trials only after success',
    'Any failed C3 trial disables C3 and stops repetition; independent regressions continue',
    'Timer/interrupt/context continuity, screen/workload wake, real silent playback',
    'USB bidirectional integrity and automatic Wi-Fi observation fallback',
    'Restore owner screen, radio, DVFS and parking settings; retain qualified C3 only if requested',
]


def identity_errors(manifest, actual):
    keys = ('build_git_commit', 'reborn_source_commit', 'kernel_version', 'rootfs_version',
            'release_version', 'build_id', 'platform_api_version', 'data_schema_version')
    errors = [k for k in keys if manifest.get(k) is None or actual['versions'].get(k) != manifest[k]]
    if actual.get('uname_release') != manifest.get('kernel_version'):
        errors.append('uname_release')
    if actual.get('reborn_build') != manifest.get('reborn_source_commit'):
        errors.append('running_reborn_build')
    if not actual.get('boot'):
        errors.append('boot_id')
    return errors


IDENTITY = """
import json,pathlib,subprocess
P=pathlib.Path
s=json.loads(subprocess.check_output(['rebornctl','status','--json']))
print(json.dumps({'boot':P('/proc/sys/kernel/random/boot_id').read_text().strip(),
 'versions':json.loads(P('/etc/y2linux/versions.json').read_text()),
 'uname_release':subprocess.check_output(['uname','-r'],text=True).strip(),
 'reborn_build':s.get('build_id'),'playback_state':s.get('playback',{}).get('state'),
 'radio':{'wifi':P('/sys/class/net/wlan0').exists(),'bluetooth':s.get('bluetooth',{}).get('powered') is True},
 'cmdline':P('/proc/cmdline').read_text().strip()}))
"""


class Run:
    def __init__(self, args):
        self.args = args
        self.directory = args.output.resolve()
        self.directory.mkdir(parents=True, mode=0o700)
        self.counter = 0
        self.source = None
        self.start_boot = None
        self.wifi_address = None

    def ssh(self, host, command, body=None, timeout=30):
        argv = ['ssh', '-F', str(self.args.ssh_config), '-T', '-o', 'BatchMode=yes',
                '-o', 'StrictHostKeyChecking=yes', '-o', 'ConnectTimeout=4',
                '-o', 'ServerAliveInterval=3', '-o', 'ServerAliveCountMax=2']
        if host == self.args.wifi_host and self.wifi_address:
            argv += ['-o', 'HostName='+self.wifi_address]
        argv += [host, command]
        utc = datetime.datetime.now(datetime.timezone.utc).isoformat()
        t = time.monotonic()
        try:
            r = subprocess.run(argv, input=body, capture_output=True, timeout=timeout)
            code, output, error = r.returncode, r.stdout, r.stderr
        except subprocess.TimeoutExpired as e:
            code, output, error = 124, e.stdout or b'', e.stderr or b''
        self.counter += 1
        base = self.directory/('%04d' % self.counter)
        base.with_suffix('.stdout').write_bytes(output)
        base.with_suffix('.stderr').write_bytes(error)
        if body:
            base.with_suffix('.stdin').write_bytes(body)
        base.with_suffix('.json').write_text(json.dumps({
            'host_utc': utc, 'command': argv, 'exit': code, 'duration_s': time.monotonic()-t,
            'source': self.source, 'device_boot_id': self.start_boot}, indent=2)+'\n')
        return code, output, error

    def observe(self, command, timeout=30):
        for host in (self.args.host, self.args.wifi_host):
            result = self.ssh(host, command, timeout=timeout)
            if result[0] == 0:
                return host, result
        return None, result

    def launch_once(self, command, body, timeout=30):
        host, result = self.observe('true')
        if host is None:
            raise RuntimeError('No authenticated observer available')
        # A transport error may mean the mutation already happened. Never retry.
        return self.ssh(host, command, body=body, timeout=timeout)

    def usb_integrity(self):
        payload = hashlib.sha256(b'Y2 CPU idle USB regression').digest()*32768
        expected = hashlib.sha256(payload).hexdigest()
        rows = []
        # Wi-Fi observer remains independent of the loaded USB channel.
        wifi = self.ssh(self.args.wifi_host, 'cat /proc/sys/kernel/random/boot_id')
        if wifi[0] or wifi[1].decode().strip() != self.start_boot:
            return {'pass': False, 'error': 'independent Wi-Fi observer unavailable'}
        for _ in range(3):
            code, output, error = self.ssh(self.args.host,
                "python3 -c 'import sys,hashlib; x=sys.stdin.buffer.read(); "
                "sys.stdout.buffer.write(x); print(hashlib.sha256(x).hexdigest(),file=sys.stderr)'",
                body=payload, timeout=45)
            observed = hashlib.sha256(output).hexdigest()
            observer_host, observer = self.observe('cat /proc/sys/kernel/random/boot_id')
            rows.append({'exit': code, 'send_sha256': expected, 'receive_sha256': observed,
                         'device_sha256': error.decode().strip(), 'observer': observer_host})
            if code or observed != expected or error.decode().strip() != expected or observer[0] or observer[1].decode().strip() != self.start_boot:
                return {'pass': False, 'rounds': rows}
        return {'pass': True, 'rounds': rows}

    def cleanup(self):
        listener_file = self.directory/'wifi-listener.json'
        if listener_file.exists():
            owned = json.loads(listener_file.read_text())
            script = '''
import json,pathlib,os,signal
x=OWNED
p=pathlib.Path(x['pid_file'])
if p.exists() and p.read_text().strip()==str(x['pid']):
 cmd=pathlib.Path('/proc/'+str(x['pid'])+'/cmdline')
 if cmd.exists() and x['pid_file'].encode() in cmd.read_bytes():
  os.kill(x['pid'],signal.SIGTERM);p.unlink(missing_ok=True)
print('owned temporary observer cleanup complete')
'''.replace('OWNED',repr(owned))
            self.launch_once('python3 -', script.encode())
        identity_file = self.directory/'installed-identity.json'
        if identity_file.exists() and not json.loads(identity_file.read_text())['radio']['wifi']:
            self.launch_once('y2-radio wifi off-runtime', None)

    def run(self):
        manifest = json.loads((self.args.package/'manifest.json').read_text())
        code, output, _ = self.launch_once('python3 -', IDENTITY.encode())
        if code:
            raise RuntimeError('Installed identity could not be read')
        actual = json.loads(output)
        errors = identity_errors(manifest, actual)
        if errors:
            raise RuntimeError('Installed identity mismatch: '+', '.join(errors))
        if actual['playback_state'] != 'stopped':
            raise RuntimeError('Stop current playback before qualification; owner media state must be preserved')
        self.source, self.start_boot = actual['versions'], actual['boot']
        (self.directory/'installed-identity.json').write_text(json.dumps(actual, indent=2)+'\n')
        # Existing authorized Wi-Fi alias pins the same physical host. Establish
        # a temporary key-only listener only if its normal observer is absent.
        wifi = self.ssh(self.args.wifi_host, 'true')
        if wifi[0]:
            setup = r'''
import json,pathlib,subprocess,os
subprocess.run(['y2-radio','wifi','on-runtime'],check=True,stdout=subprocess.DEVNULL)
import time
for _ in range(60):
 addresses=json.loads(subprocess.check_output(['ip','-j','-4','addr','show','dev','wlan0']))
 candidates=[a['local'] for d in addresses for a in d.get('addr_info',[]) if a.get('scope')=='global']
 if candidates:break
 time.sleep(.5)
addresses=json.loads(subprocess.check_output(['ip','-j','-4','addr','show','dev','wlan0']))
address=next(a['local'] for d in addresses for a in d.get('addr_info',[]) if a.get('scope')=='global')
pid='/run/cpu-idle-wifi-ssh.pid'
if pathlib.Path(pid).exists():raise RuntimeError('existing qualification listener is not reachable; inspect its owner')
log=open('/run/cpu-idle-wifi-ssh.log','ab',buffering=0)
p=subprocess.Popen(['/usr/sbin/dropbear','-F','-E','-s','-g','-j','-k','-p',address+':22',
 '-r','/etc/dropbear/dropbear_ed25519_host_key','-P',pid,'-K','10','-I','3600'],
 stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
print(json.dumps({'address':address,'pid':p.pid,'pid_file':pid}))
'''
            code, output, _ = self.launch_once('python3 -', setup.encode())
            if code:
                raise RuntimeError('Cannot establish independent Wi-Fi observer')
            listener = json.loads(output)
            self.wifi_address = listener['address']
            (self.directory/'wifi-listener.json').write_text(json.dumps(listener, indent=2)+'\n')
            time.sleep(1)
            if self.ssh(self.args.wifi_host, 'true')[0]:
                raise RuntimeError('Wi-Fi alias must point to the current device address '+listener['address'])
        config = {'device_directory': '/data/system/platform/cpu-idle-qualification/'+uuid.uuid4().hex,
                  'enable_qualified_runtime': self.args.enable_qualified_runtime,
                  'expected_boot': self.start_boot, 'expected_versions': self.source, 'original_radio':{**actual['radio'], 'wifi':True}}
        body = Path(__file__).with_name('cpu_idle_completion_device.py').read_text()
        launch = """
import json,pathlib,subprocess,os
config=CONFIG
d=pathlib.Path(config['device_directory']);d.mkdir(parents=True,mode=0o700)
os.chmod(d,0o700)
(d/'config.json').write_text(json.dumps(config));(d/'job.py').write_text(SOURCE)
log=(d/'job.log').open('ab',buffering=0)
p=subprocess.Popen(['python3',str(d/'job.py'),str(d/'config.json')],stdin=subprocess.DEVNULL,
 stdout=log,stderr=log,start_new_session=True)
(d/'pid').write_text(str(p.pid));print(json.dumps({'pid':p.pid,'directory':str(d)}))
""".replace('CONFIG', repr(config)).replace('SOURCE', repr(body))
        (self.directory/'job-config.json').write_text(json.dumps(config, indent=2)+'\n')
        launched = self.launch_once('python3 -', launch.encode())
        if launched[0]:
            print('Launch transport failed; observing the recorded job path without retry.', flush=True)
        path = config['device_directory']
        deadline = time.monotonic()+2400
        last_stage = None
        progress = None
        while time.monotonic() < deadline:
            # Only read-only polling is retried across USB/Wi-Fi. During C3
            # both transports may deliberately be down; the durable job runs on.
            host, result = self.observe('cat '+shlex.quote(path+'/progress.json'))
            if result[0] == 0:
                progress = json.loads(result[1])
                (self.directory/'device-result.json').write_text(json.dumps(progress, indent=2)+'\n')
                if progress.get('stage') != last_stage:
                    last_stage = progress.get('stage')
                    print('Device phase:', last_stage, 'observer:', host, flush=True)
                if progress.get('done'):
                    break
            host, current = self.observe('cat /proc/sys/kernel/random/boot_id')
            if current[0] == 0 and current[1].decode().strip() != self.start_boot:
                self.observe('cat /sys/firmware/y2_pm/previous /sys/firmware/y2_pm/devices_previous /sys/firmware/y2_pm/reset_status; dmesg')
                raise RuntimeError('Device reset; retained stage captured. No C3 retry or automatic reflash.')
            time.sleep(60) # sparse observers preserve the coordinator quiet window
        if not progress or not progress.get('done'):
            self.observe('cat '+shlex.quote(path+'/job.log'))
            raise RuntimeError('Qualification did not finish; job path '+path+'; no mutation retry')
        progress['USB_WiFi_integrity'] = self.usb_integrity()
        progress['pass'] = progress.get('pass') and progress['USB_WiFi_integrity']['pass']
        if not progress['pass']:
            self.launch_once("sh -c 'echo 1 > /sys/devices/system/cpu/cpu0/cpuidle/state2/disable; echo 0 > /sys/module/spm/parameters/dormant_budget'", None)
        (self.directory/'result.json').write_text(json.dumps(progress, indent=2)+'\n')
        print('Qualification:', 'PASS' if progress['pass'] else 'FAIL', str(self.directory/'result.json'))
        return 0 if progress['pass'] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--package', type=Path)
    parser.add_argument('--ssh-config', type=Path)
    parser.add_argument('--host', default='y2')
    parser.add_argument('--wifi-host', default='y2-owner-wifi')
    parser.add_argument('--output', type=Path, default=Path('out/cpu-idle-completion-physical')/
                        datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ-candidate'))
    parser.add_argument('--enable-qualified-runtime', action='store_true',
                        help='leave normal C3 policy enabled only after all 20 checked timer wakes')
    args = parser.parse_args()
    if not args.run:
        print('\n'.join(PLAN))
        return 0
    if not args.package or not args.ssh_config or not args.ssh_config.is_file():
        parser.error('--run requires the built --package and existing authorized --ssh-config')
    os.umask(0o077)
    runner = Run(args)
    try:
        return runner.run()
    except Exception as e:
        print('Qualification failed:', e)
        return 1
    finally:
        try:
            runner.cleanup()
        except Exception as e:
            print('Observer cleanup recorded:', e)


if __name__ == '__main__':
    raise SystemExit(main())
