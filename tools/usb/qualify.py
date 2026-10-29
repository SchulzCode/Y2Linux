#!/usr/bin/env python3
"""Bounded real-host USB transfer matrix. No flash, raw sectors or formatting.
RETIRED for the player: its device side runs Python, and the player has none since test build 169
(docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.

Every device/USB access is inside the shared nonblocking flock. Workloads use
unique scratch directories; cleanup refuses a changed boot/mount identity.
MTP runs through GIO, exercising the normal GNOME/GVfs desktop path.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import shlex
import shutil
import subprocess
import tempfile
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'development'))
from device_lease import open_lease

SNAPSHOT = '''import json,pathlib,os
p=pathlib.Path
def read(name):
 try:return p(name).read_text()
 except OSError:return None
processes={}
for child in p("/proc").iterdir():
 if not child.name.isdigit():continue
 name=(read(str(child/"comm")) or "").strip()
 if name not in ("umtprd","dropbear","reborn"):continue
 stat=read(str(child/"stat"))
 if not stat:continue
 fields=stat.rsplit(")",1)[1].split()
 memory={}
 for line in (read(str(child/"status")) or "").splitlines():
  if line.startswith(("VmRSS:","VmHWM:","VmSize:")):memory[line.split(":",1)[0]]=int(line.split()[1])
 processes[child.name]={"name":name,"ticks":int(fields[11])+int(fields[12]),"start":fields[19],"memory_kib":memory}
print(json.dumps({"hz":os.sysconf("SC_CLK_TCK"),"processes":processes,"frequency_khz":{v.name:read(str(v/"scaling_cur_freq")) for v in p("/sys/devices/system/cpu/cpufreq").glob("policy*")},"file_transfer":read("/run/y2/usb-transfer.json"),"boot":read("/proc/sys/kernel/random/boot_id"),"mounts":read("/proc/self/mountinfo"),"stat":read("/proc/stat"),"memory":read("/proc/meminfo"),"interrupts":read("/proc/interrupts"),"usb":read("/sys/bus/platform/drivers/y2-usb/11200000.usb/status"),"speed":[read(str(v/"current_speed")) for v in p("/sys/class/udc").glob("*")],"versions":read("/etc/y2linux/versions.json"),"storage_stat":{str(v):read(str(v)) for v in p("/sys/class/block").glob("mmcblk*/stat")}}))'''


def run(argv, timeout=90):
    answer = subprocess.run(argv, check=True, capture_output=True, text=True, timeout=timeout)
    return answer.stdout.strip()


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def delta(before, after, seconds):
    def stat(value):
        return {line.split()[0]: [int(n) for n in line.split()[1:]]
                for line in (value or '').splitlines() if line.startswith(('cpu ', 'intr ', 'ctxt '))}
    a, b = stat(before['stat']), stat(after['stat'])
    ticks = [y-x for x,y in zip(a.get('cpu',[])[:8], b.get('cpu',[])[:8])]
    total = sum(ticks)
    processes=[]
    for pid, current in after.get('processes',{}).items():
        old=before.get('processes',{}).get(pid)
        if old and old['start']==current['start'] and current['ticks']>=old['ticks']:
            processes.append({'pid':pid,'name':current['name'],
                'cpu_percent_one_core':100*(current['ticks']-old['ticks'])/after.get('hz',100)/seconds,
                'memory_kib':current['memory_kib'], 'memory_scope':'RSS now; HWM is process lifetime, not transfer-only peak'})
    return {'processes':processes,'cpu_busy_percent': 100*(total-sum(ticks[3:5]))/total if total and all(t>=0 for t in ticks) else None,
            'irq_per_s': (b['intr'][0]-a['intr'][0])/seconds if 'intr' in a and 'intr' in b else None,
            'context_switches_per_s': (b['ctxt'][0]-a['ctxt'][0])/seconds if 'ctxt' in a and 'ctxt' in b else None,
            'cpu_scope': 'all CPUs aggregate average; no peak-memory claim'}


def validate_volume(volumes, volume):
    selected=volumes[3 if volume=='internal' else 2]
    if selected.get('state')!='Ready':
        raise RuntimeError('media_not_ready_or_writable')
    if volume=='internal' and (selected.get('uuid')!='79324c69-6e75-4801-8000-000000000103' or
            selected.get('start')!='3742720' or selected.get('sectors')!='11460608' or selected.get('filesystem')!='ext4'):
        raise RuntimeError('internal_user_partition_identity_invalid')
    if volume=='sd' and selected.get('filesystem') not in ('vfat','exfat','ext4'):
        raise RuntimeError('unsupported_sd_filesystem')
    return selected


class Harness:
    def __init__(self, args):
        self.args = args
        self.ssh = ['ssh', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5']
        if args.ssh_config: self.ssh += ['-F', str(args.ssh_config)]
        else: self.ssh += ['-i', str(Path.home()/'.ssh/y2linux_ed25519')]
        self.sftp = ['sftp', '-q', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5']
        if args.ssh_config: self.sftp += ['-F', str(args.ssh_config)]
        else: self.sftp += ['-i', str(Path.home()/'.ssh/y2linux_ed25519')]
        self.identity = None

    def remote(self, argv, timeout=90):
        return run(self.ssh + [self.args.host, shlex.join(argv)], timeout)

    def snapshot(self):
        return json.loads(self.remote(['python3', '-c', SNAPSHOT], timeout=15))

    def guard(self):
        current = self.snapshot()
        # Relevant mount IDs/dev_t/fs options and boot must remain identical.
        def media_mounts(value):
            return [s for s in (value or '').splitlines() if ' /data ' in s or ' /media/sd ' in s]
        if self.identity and (current['boot'] != self.identity['boot'] or media_mounts(current['mounts']) != media_mounts(self.identity['mounts'])):
            raise RuntimeError('boot_or_media_identity_changed; scratch cleanup requires owner inspection')
        return current

    def sftp_batch(self, pairs, timeout):
        # One authenticated SFTP session per workload, including small files.
        def quoted(path): return '"'+str(path).replace('\\','\\\\').replace('"','\\"')+'"'
        with tempfile.NamedTemporaryFile(mode='w', prefix='y2-usb-sftp-') as batch:
            for action,source,destination in pairs:
                batch.write(action+' '+quoted(source)+' '+quoted(destination)+'\n')
            batch.flush()
            run(self.sftp+['-b',batch.name,self.args.host], timeout=timeout)

    def copies(self, files, local, upload):
        remaining=self.deadline-time.monotonic()
        if remaining<=0: raise RuntimeError('bounded_batch_deadline; incomplete cases retained')
        pairs=[('put -f',file,self.device_url(file.name)) if upload else
               ('get',self.device_url(file.name),local/('returned-'+file.name)) for file in files]
        if self.args.protocol=='sftp':
            self.sftp_batch(pairs,min(90,remaining))
        else:
            for _,source,destination in pairs:
                remaining=self.deadline-time.monotonic()
                if remaining<=0: raise RuntimeError('bounded_batch_deadline; incomplete cases retained')
                run(['gio','copy',str(source),str(destination)],timeout=min(90,remaining))

    def device_url(self, name):
        if self.args.protocol == 'sftp': return self.device_dir+'/'+name
        from urllib.parse import quote
        return self.args.mtp_uri.rstrip('/')+'/'+quote(self.folder)+'/'+quote(name)

    def case(self, files, local):
        total = sum(p.stat().st_size for p in files)
        before = self.guard(); start=time.monotonic()
        self.copies(files,local,True)
        duration=time.monotonic()-start; after=self.guard()
        write={'seconds':duration,'MB_per_s':total/1e6/duration,**delta(before,after,duration)}
        # Independently hash the actual Y2 files, then verify the downloaded bytes.
        script='import hashlib,json,pathlib; p=pathlib.Path('+repr(self.device_dir)+'); print(json.dumps({v.name:hashlib.file_digest(v.open("rb"),"sha256").hexdigest() for v in p.iterdir() if v.is_file()}))'
        hashes=json.loads(self.remote(['python3','-c',script]))
        for file in files:
            if hashes.get(file.name)!=digest(file): raise RuntimeError('upload_sha256_mismatch:'+file.name)
        before=self.guard(); start=time.monotonic()
        self.copies(files,local,False)
        duration=time.monotonic()-start; after=self.guard()
        read={'seconds':duration,'MB_per_s':total/1e6/duration,**delta(before,after,duration)}
        for file in files:
            if digest(local/('returned-'+file.name))!=digest(file): raise RuntimeError('download_sha256_mismatch:'+file.name)
        if self.args.protocol=='mtp': run(['gio','remove',*[self.device_url(file.name) for file in files]])
        else:
            self.remote(['python3','-c','import pathlib; p=pathlib.Path('+repr(self.device_dir)+'); [(p/n).unlink() for n in '+repr([f.name for f in files])+']'])
        return {'files':len(files),'bytes':total,'write':write,'read':read,'sha256':'MATCH',
                'hashes':{f.name:{'host_before':digest(f),'device_after_write':hashes[f.name],
                    'host_after_read':digest(local/('returned-'+f.name))} for f in files},
                'before':before,'after':after}

    def matrix(self):
        self.identity=self.snapshot()
        if self.args.package:
            expected=json.loads((self.args.package/'manifest.json').read_text())
            installed=json.loads(self.identity['versions'])
            for field in ('build_git_commit','reborn_source_commit','kernel_version','rootfs_version'):
                if installed.get(field)!=expected.get(field): raise RuntimeError('wrong_installed_candidate:'+field)
        script='import sys,json,pathlib;sys.path.insert(0,"/usr/lib/y2-platform");from y2_platform.common import Context,read;from y2_platform.observe import storage;v=storage(Context())["volumes"]\nfor i in (1,3):\n p=pathlib.Path("/sys/dev/block")/v[i].get("device_id","absent");v[i].update(start=read(p/"start"),sectors=read(p/"size"))\nprint(json.dumps(v))'
        self.volume_identity=validate_volume(json.loads(self.remote(['python3','-c',script])),self.args.volume)
        self.folder='Y2-USB-check-'+uuid.uuid4().hex
        base='/media/sd' if self.args.volume=='sd' else '/media/internal/music'
        self.device_dir=base+'/'+self.folder
        # mkdir refuses a collision. No owner-selected destination path.
        if self.args.protocol=='mtp': run(['gio','mkdir',self.device_url('')])
        else: self.remote(['mkdir','-m','700',self.device_dir])
        report={'schema':'org.y2linux.usb-physical/v1','protocol':self.args.protocol,'identity':self.identity,
                'scratch':self.device_dir,'volume':self.args.volume,'volume_identity':self.volume_identity,'cases':[],'result':'RUNNING','host':run(['uname','-a']),
                'host_desktop_discovery': 'GIO MTP URI usable' if self.args.protocol=='mtp' else 'ECM/SFTP only'}
        deadline=time.monotonic()+240
        self.deadline=deadline
        try:
            with tempfile.TemporaryDirectory(prefix='y2-usb-host-') as directory:
                local=Path(directory)
                for mib in self.args.sizes:
                    if time.monotonic()>deadline: break
                    free=int(self.remote(['python3','-c','import os;v=os.statvfs('+repr(base)+');print(v.f_bavail*v.f_frsize)']))
                    if free-mib*1024**2 < 96*1024**2:
                        report['cases'].append({'mib':mib,'result':'SKIP_RESERVE'}); continue
                    file=local/('sequential-%s.bin'%mib)
                    with file.open('xb') as stream:
                        block=os.urandom(1024**2)
                        for _ in range(mib): stream.write(block)
                    row=self.case([file],local); row['workload']='sequential-%s-MiB'%mib
                    report['cases'].append(row); file.unlink(); (local/('returned-'+file.name)).unlink()
                for count in self.args.small_files:
                    if time.monotonic()>deadline: break
                    files=[]
                    for i in range(count):
                        file=local/('small-%04d.bin'%i); file.write_bytes(os.urandom(4096)); files.append(file)
                    row=self.case(files,local); row['workload']='small-%s'%count; report['cases'].append(row)
                    for file in files: file.unlink(); (local/('returned-'+file.name)).unlink()
                if self.args.music_corpus and time.monotonic()<deadline:
                    files=[]
                    for i,file in enumerate(sorted(self.args.music_corpus.rglob('*'))):
                        if file.is_file() and not file.is_symlink() and file.suffix.lower() in ('.flac','.mp3'):
                            copy=local/('music-%04d'%i+file.suffix.lower()); shutil.copyfile(file,copy); files.append(copy)
                        if len(files)>=100: break
                    if files:
                        free=int(self.remote(['python3','-c','import os;v=os.statvfs('+repr(base)+');print(v.f_bavail*v.f_frsize)']))
                        if sum(f.stat().st_size for f in files)+96*1024**2<=free:
                            row=self.case(files,local);row['workload']='mixed-FLAC-MP3';report['cases'].append(row)
                report['complete'] = len(report['cases']) == len(self.args.sizes) + len(self.args.small_files) + bool(self.args.music_corpus)
                report['result']='PASS_BOUNDED_TRANSFER_MATRIX' if report['complete'] else 'PASS_PARTIAL_BOUNDED_TRANSFER_MATRIX'
        except Exception as error:
            report.update(result='FAIL',error=str(error))
        finally:
            try:
                self.guard()
                # On failure keep uncertain copies for inspection. Never run a
                # recursive delete over a mount or owner-provided directory.
                if report['result'].startswith('PASS'):
                    if self.args.protocol=='mtp': run(['gio','remove',self.device_url('')])
                    else: self.remote(['rmdir','--',self.device_dir])
                else: report['scratch_retained']=True
            except Exception as error: report['cleanup_error']=str(error)
        return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run',action='store_true',help='perform bounded physical matrix')
    parser.add_argument('--protocol',choices=['sftp','mtp'],default='sftp')
    parser.add_argument('--host',default='root@10.42.0.1')
    parser.add_argument('--volume',choices=['internal','sd'],default='internal')
    parser.add_argument('--ssh-config',type=Path)
    parser.add_argument('--mtp-uri',help='GIO URI of Y2 Internal Storage, e.g. mtp://[usb:003,012]/Internal Storage')
    parser.add_argument('--package',type=Path)
    parser.add_argument('--device-lock-fd',type=int,help='inherited integration campaign lease')
    parser.add_argument('--sizes',type=int,nargs='+',default=[1,16,128,512])
    parser.add_argument('--small-files',type=int,nargs='+',default=[100,1000])
    parser.add_argument('--music-corpus',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if not args.run: parser.error('--run is required; never flashes or formats')
    if args.protocol=='mtp' and not args.mtp_uri: parser.error('--mtp-uri required from the standard file manager/GIO')
    if args.mtp_uri and (not args.mtp_uri.startswith('mtp://') or '\n' in args.mtp_uri): parser.error('only standard MTP URIs allowed')
    if any(n<1 or n>512 for n in args.sizes) or any(n<1 or n>1000 for n in args.small_files): parser.error('bounded workload sizes required')
    args.output.mkdir(parents=True,exist_ok=True)
    if (args.output/'result.json').exists(): parser.error('fresh evidence output required')
    try:
        lease = open_lease(args.device_lock_fd)
    except BlockingIOError:
        result={'result':'NOT_RUN_DEVICE_LOCK_BUSY','physical_access':False}
    else:
        with lease:
            try: result=Harness(args).matrix()
            except Exception as error: result={'result':'FAIL_PREFLIGHT','error':str(error)}
    (args.output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k in ('result','error','cleanup_error')}))
    return 0 if result['result'].startswith('PASS') else 2


if __name__=='__main__': raise SystemExit(main())
