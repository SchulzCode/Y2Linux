#!/usr/bin/env python3
"""One owner-run Fix01 qualification over authenticated SSH. Never flash/reboot.

Default prints the plan. --run explicitly starts tests on the installed candidate.
Use the owner's existing SSH config (including pinned host keys and key/account).
--wifi-host permits battery suspend while USB is unplugged. All receipts private.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import time
import uuid

PLAN = ['timer admission/PPI29/CNTFRQ', 'actual highres and NO_HZ',
        'real Reborn playback/input/artwork/scan leases', 'screen-off and SLIDLE entries',
        'stock-bin high OPP/voltage/thermal admission', 'DORMANT prerequisites (no activation)',
        'charger refusal without taint', 'freezer/devices/platform/processors/core regression',
        'full RTC same-boot wake', 'Power wake', 'USB/radio/Reborn restore',
        'bounded playback and platform measurements']

REMOTE = r'''
import json,os,pathlib,subprocess,time,struct,sqlite3,threading
P=pathlib.Path

def read(path):
 try:return P(path).read_text().strip()
 except OSError:return None

def ctl(*args):
 return json.loads(subprocess.check_output(['rebornctl',*args,'--json'],timeout=15))

def snapshot():
 return json.loads(subprocess.check_output(['y2-platform','status','cpu'],timeout=15))['cpu']

def resume_receipt(directory):
 result=read(directory+'/result')
 value={'result':result,'boot':read('/proc/sys/kernel/random/boot_id'),'journal':read('/sys/firmware/y2_pm/state')}
 if result:
  for name in ('before-dmesg','dmesg','before-interrupts','interrupts','output'):
   value[name]=read(directory+'/'+name)
 return value

def key(name,code):
 devices=list(P('/sys/class/input').glob('event*/device/name'))
 target=next(p for p in devices if name in p.read_text())
 event=target.parent.parent.name
 fd=os.open('/dev/input/'+event,os.O_WRONLY)
 try:
  for value in (1,0):
   os.write(fd,struct.pack('llHHi',0,0,1,code,value))
   os.write(fd,struct.pack('llHHi',0,0,0,0,0));time.sleep(.015)
 finally:os.close(fd)

def hint_test():
 samples=[];stop=threading.Event()
 def poll():
  while not stop.wait(.01): samples.append({'t':time.monotonic(),'leases':read('/sys/module/workload/parameters/leases')})
 worker=threading.Thread(target=poll);worker.start()
 initial=ctl('status')
 try:
  if initial.get('screen_off'):key('pmic',116)
  key('navigation',103)
  with sqlite3.connect('file:/data/reborn/library.db?mode=ro',uri=True) as db:
   track=db.execute('select tracks.id from tracks join sources on sources.id=tracks.source_id where deleted=0 and sources.online=1 limit 1').fetchone()
  if track:
   ctl('output','wired');ctl('play',str(track[0]));time.sleep(4)
  ctl('scan');time.sleep(4)
  key('navigation',108)
  if not ctl('status').get('screen_off'):key('pmic',116)
  time.sleep(.1)
  screen_off={'status':ctl('status'),'leases':read('/sys/module/workload/parameters/leases')}
  ctl('stop')
 finally:
  stop.set();worker.join()
 return {'samples':samples,'screen_off':screen_off,'initial':initial,'audio':ctl('audio')}

def playback_measurement():
 with sqlite3.connect('file:/data/reborn/library.db?mode=ro',uri=True) as db:
  track=db.execute('select tracks.id from tracks join sources on sources.id=tracks.source_id where deleted=0 and sources.online=1 limit 1').fetchone()
 if not track:return {'measured':False,'reason':'no_online_library_track'}
 samples=[]
 try:
  ctl('output','wired');ctl('play',str(track[0]))
  for _ in range(6):
   time.sleep(5);samples.append({'audio':ctl('audio'),'metrics':ctl('metrics'),'cpu':snapshot()})
 finally:ctl('stop')
 return {'measured':True,'track_id':track[0],'samples':samples,'health':ctl('health')}

def dvfs_test():
 policy=P('/sys/devices/system/cpu/cpufreq/policy0')
 before={n:read(policy/n) for n in ('scaling_governor','scaling_min_freq','scaling_max_freq')}
 rows=[]
 try:
  (policy/'scaling_governor').write_text('performance')
  for frequency in map(int,(read(policy/'scaling_available_frequencies') or '').split()):
   # Policy and thermal maximum remain authoritative; no qualification bypass.
   if frequency>int(read(policy/'cpuinfo_max_freq')):continue
   (policy/'scaling_min_freq').write_text('598000')
   (policy/'scaling_max_freq').write_text(str(frequency))
   time.sleep(.5);rows.append({'requested_khz':frequency,'status':snapshot()})
 finally:
  (policy/'scaling_min_freq').write_text('598000')
  (policy/'scaling_max_freq').write_text(before['scaling_max_freq'])
  (policy/'scaling_min_freq').write_text(before['scaling_min_freq'])
  (policy/'scaling_governor').write_text(before['scaling_governor'])
 return rows
'''

class Run:
    def __init__(self, args, manifest):
        self.args=args;self.manifest=manifest;self.index=0;self.hosts=[args.host]
        if args.wifi_host:self.hosts.append(args.wifi_host)
        self.output=args.output
        self.output.mkdir(parents=True,mode=0o700,exist_ok=False)
        self.boot=None

    def remote(self, code, timeout=25, required=True):
        self.index+=1
        stem=self.output/f'{self.index:03d}'
        stem.with_suffix('.remote.py').write_text(code)
        failures=[]
        for host in self.hosts:
            try:
                result=subprocess.run(['ssh','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes',
                                       '-o','ConnectTimeout=4',host,'python3 -'],
                                      input=code,text=True,capture_output=True,timeout=timeout)
                stem.with_suffix('.stdout').write_text(result.stdout)
                stem.with_suffix('.stderr').write_text(result.stderr)
                if result.returncode==0:
                    value=json.loads(result.stdout)
                    stem.with_suffix('.host').write_text(host+'\n')
                    return value
                failures.append({'host':host,'code':result.returncode,'stderr':result.stderr})
            except (subprocess.TimeoutExpired,ValueError) as error:
                failures.append({'host':host,'error':str(error)})
        stem.with_suffix('.failure.json').write_text(json.dumps(failures,indent=2))
        if required:raise RuntimeError('SSH test failed; inspect '+str(stem))
        return None

    def evaluate(self,label,result):
        (self.output/(label+'.json')).write_text(json.dumps(result,indent=2)+'\n')
        print(label,flush=True)
        return result

    def capture(self,label):
        value=self.remote(REMOTE+'\nprint(json.dumps(snapshot()))\n')
        return self.evaluate(label,value)

    def integrity(self,label,host):
        payload=bytes(range(256))*1024
        encoded=base64.b64encode(payload).decode('ascii')
        path='/data/system/platform/fix01-transfer-'+uuid.uuid4().hex
        source=REMOTE+f'''
import base64,hashlib
data=base64.b64decode({encoded!r});path=P({path!r})
try:
 with path.open('xb') as stream:stream.write(data);stream.flush();os.fsync(stream.fileno())
 result=path.read_bytes()
 print(json.dumps({{'sha256':hashlib.sha256(result).hexdigest(),'returned':base64.b64encode(result).decode('ascii')}}))
finally:
 path.unlink(missing_ok=True)
'''
        previous=self.hosts
        try:
            self.hosts=[host]
            result=self.remote(source,timeout=60)
        finally:self.hosts=previous
        assert result['sha256']==hashlib.sha256(payload).hexdigest()
        assert base64.b64decode(result.pop('returned'))==payload
        return self.evaluate(label,{'host':host,'bytes_each_direction':len(payload),**result})

    def suspend(self,mode,alarm=True,expect_refusal=False,power=False):
        token=uuid.uuid4().hex
        directory='/data/system/platform/fix01-'+token
        script='''#!/bin/sh
umask 077
before=$(cat /proc/sys/kernel/random/boot_id)
taint_before=$(cat /proc/sys/kernel/tainted)
dmesg > DIR/before-dmesg
cat /proc/interrupts > DIR/before-interrupts
# Allow the launching SSH command to return before radios are quiesced.
sleep 2
grep -q '\\[none\\]' /sys/power/pm_test || exit 2
restore_test() { printf 'none\\n' > /sys/power/pm_test; }
trap restore_test EXIT
printf '%s\\n' MODE > /sys/power/pm_test
y2-suspend --owner-qualify
rc=$?
restore_test
dmesg > DIR/dmesg
cat /proc/interrupts > DIR/interrupts
printf '%s %s %s %s %s\\n' "$rc" "$before" "$(cat /proc/sys/kernel/random/boot_id)" "$taint_before" "$(cat /proc/sys/kernel/tainted)" > DIR/result
'''.replace('MODE',shlex.quote(mode)).replace('DIR',shlex.quote(directory))
        code=REMOTE+f'''
directory=P({directory!r});directory.mkdir(mode=0o700)
script=directory/'run.sh';script.write_text({script!r})
'''
        if alarm:
            code+="subprocess.run(['y2-platform','rtc-alarm','120'],check=True,timeout=15)\n"
        code+="log=open(directory/'output','w');subprocess.Popen(['sh',str(script)],stdout=log,stderr=log,start_new_session=True,stdin=subprocess.DEVNULL);print(json.dumps({'started':str(directory)}))\n"
        self.remote(code)
        started=time.monotonic()
        pressed=False
        automatic=True
        deadline=time.monotonic()+(90 if mode!='none' else 180)
        answer=None
        while time.monotonic()<deadline:
            time.sleep(3)
            if power and not pressed and time.monotonic()-started >= 35:
                input('Power wake test: press Power ONCE now, then press Enter. ')
                pressed=True
            answer=self.remote(REMOTE+f"\nprint(json.dumps(resume_receipt({directory!r})))\n",required=False)
            if answer and answer['boot']!=self.boot:raise RuntimeError('New boot is not resume: '+json.dumps(answer))
            if answer and answer['result']:break
        if (not answer or not answer.get('result')) and mode!='none':
            self.evaluate('staged-timeout-'+mode,answer)
            raise RuntimeError('Staged test/transport timeout; no RTC wake was requested')
        if not answer or not answer.get('result'):
            # First RTC recovery attempt only after automatic wake/reconnect expired.
            automatic=False
            input('RTC/transport did not return. Press Power ONCE, then press Enter here. ')
            deadline=time.monotonic()+90
            while time.monotonic()<deadline:
                time.sleep(3)
                answer=self.remote(REMOTE+f"\nprint(json.dumps(resume_receipt({directory!r})))\n",required=False)
                if answer and answer['result']:break
        self.evaluate('suspend-'+('power-' if power else 'rtc-' if alarm else '')+mode+'-'+token,{'receipt':answer,'automatic_return':automatic,'host_elapsed_s':time.monotonic()-started})
        if not answer or not answer.get('result'):raise RuntimeError('No wake; retained stage must be collected after owner recovery')
        rc,before,after,taint_before,taint_after=answer['result'].split()
        if before!=after or after!=self.boot or taint_before!=taint_after:
            raise RuntimeError('Suspend boot/taint regression')
        if (rc=='0')==expect_refusal:raise RuntimeError('Unexpected suspend result '+rc)
        if expect_refusal:
            previous=set((answer.get('before-dmesg') or '').splitlines())
            fresh='\n'.join(line for line in (answer.get('dmesg') or '').splitlines() if line not in previous)
            if 'y2_charger_prepare_pm returns -16' not in fresh:
                raise RuntimeError('Failure was not the expected fresh charger -EBUSY refusal')
        if mode=='none' and not power and (not automatic or time.monotonic()-started < 100):
            raise RuntimeError('RTC wake was not demonstrated; same-boot Power recovery is recorded separately')
        if power and (not pressed or time.monotonic()-started >= 115):
            raise RuntimeError('Power wake was not distinguished from RTC recovery alarm')
        if mode=='none':
            label='mtk-pmic-keys' if power else 'mt6397-rtc'
            if irq_count(answer.get('interrupts'),label)<=irq_count(answer.get('before-interrupts'),label):
                raise RuntimeError('Expected wake IRQ was not serviced: '+label)
        return answer


def irq_count(text,label):
    total=0
    for line in (text or '').splitlines():
        if label not in line:continue
        for field in line.split()[1:]:
            if not field.isdigit():break
            total+=int(field)
    return total

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--host',default='y2')
    p.add_argument('--wifi-host')
    p.add_argument('--package',type=Path,default=Path('out/y2linux-cpu-final-fix01-candidate'))
    p.add_argument('--output',type=Path,default=Path('out/cpu-fix01-physical-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')))
    p.add_argument('--run',action='store_true')
    args=p.parse_args()
    if not args.run:
        print('\n'.join(f'{n}. {stage}' for n,stage in enumerate(PLAN,1)));return
    os.umask(0o077)
    manifest=json.loads((args.package/'manifest.json').read_text())
    run=Run(args,manifest)
    identity=run.remote(REMOTE+"\nprint(json.dumps({'versions':json.loads(read('/etc/y2linux/versions.json')),'boot':read('/proc/sys/kernel/random/boot_id')}))\n")
    for field in ('build_git_commit','reborn_source_commit','kernel_version','rootfs_version'):
        if identity['versions'][field]!=manifest[field]:raise RuntimeError('Wrong candidate '+field)
    run.boot=identity['boot'];run.evaluate('identity',identity)
    cpu=run.capture('timer-first')
    timer=cpu['timer']
    admitted=timer['admission'].get('events_ready') in ('Y','1')
    run.evaluate('timer-verdict',{'admitted':admitted,'highres':timer['highres_active'],'nohz':timer['no_hz_active'],'local_events':timer.get('clockevents')})
    run.evaluate('real-reborn-hints',run.remote(REMOTE+'\nprint(json.dumps(hint_test()))\n',timeout=35))
    run.capture('slidle-before')
    # Initial hold60 + quiet30 + two additional 5-second park steps. Do not
    # issue an active lease or force CPUs offline during this eligibility window.
    for _ in range(12):time.sleep(10)
    run.capture('slidle-after-eligible-window')
    run.evaluate('dvfs-matrix',run.remote(REMOTE+'\nprint(json.dumps(dvfs_test()))\n',timeout=30))
    run.capture('dormant-prerequisites-only')
    attached=run.remote(REMOTE+"\nprint(json.dumps({'charging':any(p.read_text().strip()=='1' for p in P('/sys/class/power_supply').glob('*/online'))}))\n")
    if attached['charging']:run.suspend('devices',alarm=False,expect_refusal=True)
    if not args.wifi_host:
        raise RuntimeError('Battery full suspend needs --wifi-host; preserve captured independent results')
    input('Confirm Wi-Fi SSH works, unplug charging USB, then press Enter. ')
    for mode in ('freezer','devices','platform','processors','core'):run.suspend(mode,alarm=False)
    if not admitted or not timer['highres_active'] or not timer['no_hz_active']:
        raise RuntimeError('Timer prerequisite failed; full sleep deferred with exact admission receipts')
    run.suspend('none')
    run.capture('rtc-resume')
    # Power test uses one deliberate Power press; RTC remains a recovery backstop.
    run.suspend('none',power=True)
    run.capture('power-resume')
    run.integrity('wifi-post-resume-integrity',args.wifi_host)
    input('Reconnect USB for ECM/ACM restoration checks, then press Enter. ')
    run.integrity('usb-post-resume-integrity',args.host)
    run.capture('usb-radio-restored')
    run.evaluate('bounded-playback',run.remote(REMOTE+'\nprint(json.dumps(playback_measurement()))\n',timeout=60))
    run.evaluate('workload-measurements',run.remote(REMOTE+"\nprint(json.dumps({'audio':ctl('audio'),'metrics':ctl('metrics'),'health':ctl('health'),'cpu':snapshot()}))\n"))
    print('Receipts: '+str(run.output)+'. Inspect measured counters/wake reasons; no inferred hardware passes.')

if __name__=='__main__':main()
