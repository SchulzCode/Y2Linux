#!/usr/bin/env python3
"""Plan or passively collect exact-candidate qualification; never start workloads.

Default is a local plan. --run is explicit owner execution over USB SSH only.
Polling survives transport gaps without hiding them. It never reboots, flashes,
cycles a radio, changes a clock or initiates suspend. Captures stay private.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from y2_platform.common import command
from y2_platform.collect import Growth
from y2_platform.observe import utilization

PROFILES = {
    'wired-8h': (28800, 'Owner starts S16/44.1 wired playback, normal safe volume; observes sound and controls.'),
    'bluetooth-8h': (28800, 'Owner pairs/trusts a peer, selects manual SBC, proves actual PCM/audio, then starts playback.'),
    'scan-playback': (1800, 'Owner starts playback and a library scan of a declared 1k/10k/20k dataset.'),
    'wifi-playback': (1800, 'Owner establishes IP/route/DNS, starts playback and a bounded declared Wi-Fi transfer.'),
    'wifi-bluetooth': (1800, 'Owner establishes Wi-Fi plus actual SBC, then performs declared scan/transfer intervals.'),
    'sd-cycles': (1800, 'Owner logs insertion/eject/removal/reinsertion and card UUID; deliberate removal uses disposable media.'),
    'usb-cycles': (1800, 'Owner logs cable and PC sleep/wake cycles; missing SSH intervals remain explicit gaps.'),
    'file-transfer': (1800, 'Owner runs authenticated verified uploads, large and small files, then compares hashes.'),
    'renderer-cycles': (900, 'Owner toggles screen/renderer through supported controls; checks return and audio continuity.'),
    'service-restart': (900, 'Owner deliberately restarts one named service and logs action/expected readiness transitions.'),
    'repeated-boot': (1800, 'Owner invokes the platform shutdown/reboot contract between samples; compare boot IDs.'),
    'suspend-resume': (900, 'PHYSICAL_GATE: separately approved owner resume experiment only; no automatic suspend.'),
    'ota-recovery': (1800, 'PHYSICAL_GATE: owner follows signed-update/recovery session; capture each exact source pair separately.'),
    'near-full-data': (1800, 'Owner uses disposable scratch only, preserves database/music and reserve; records admission/refusal.'),
    'cpu-memory-thermal': (1800, 'Owner labels idle/screen-off/FLAC/24-96/EQ/crossfade/scan/artwork/radio workload and parameters.'),
}
COUNTERS = ('audio_xruns','audio_recoveries','decoder_stalls','ffmpeg_decode_errors',
            'bluetooth_connects','bluetooth_disconnects','bluetooth_errors','wifi_errors',
            'library_scan_errors','playback_errors','log_write_errors','logs_dropped')
MAX_BYTES = 64*1024**2


def identity(versions):
    result = {k: versions.get(k) for k in ('build_git_commit','reborn_source_commit','kernel_version','rootfs_version')}
    if any(not isinstance(v,str) or not v for v in result.values()) or any(
            not re.fullmatch('[0-9a-f]{40}',result[k]) for k in ('build_git_commit','reborn_source_commit')):
        raise ValueError('exact_full_source_pair_and_versions_required')
    return result


def check_identity(sample, expected):
    if sample.get('schema') != 'org.y2linux.status/v1':
        raise ValueError('unexpected_status_schema')
    record=sample.get('record',{})
    actual=dict(zip(('build_git_commit','reborn_source_commit','kernel_version','rootfs_version'),
                    (record.get(k) for k in ('y2linux_commit','reborn_commit','kernel','rootfs_release'))))
    if actual != expected or not record.get('boot_id') or type(record.get('monotonic_ns')) is not int:
        raise ValueError('candidate_identity_mismatch')


def ssh_options(private_key, known_hosts):
    for path in (private_key,known_hosts):
        if not path.is_file(): raise ValueError('owner_key_and_verified_known_hosts_required')
    return ['ssh','-F','/dev/null','-oBatchMode=yes','-oIdentitiesOnly=yes',
            '-oStrictHostKeyChecking=yes','-oConnectTimeout=5','-oServerAliveInterval=3',
            '-oServerAliveCountMax=1','-oUserKnownHostsFile='+str(known_hosts.resolve()),
            '-i',str(private_key.resolve()),'root@10.42.0.1']


def run(profile, expected, output, seconds, interval, options, runner=command, sleep=time.sleep, clock=time.monotonic):
    if profile not in PROFILES or not 1<=seconds<=86400 or not 10<=interval<=300:
        raise ValueError('qualification_bounds_exceeded')
    output=Path(output);output.mkdir(mode=0o700,parents=True,exist_ok=False)
    start=clock(); deadline=start+seconds; count=gaps=total=0; failure=None
    def emit(stream,value):
        nonlocal total
        raw=(json.dumps(value,sort_keys=True,allow_nan=False)+'\n').encode()
        if len(raw)>1024**2 or total+len(raw)>MAX_BYTES: raise ValueError('capture_64MiB_limit')
        stream.write(raw);stream.flush();total+=len(raw)
    def call(args):
        return runner([*options,shlex.join(args)],timeout=12,limit=1024**2)
    with (output/'samples.jsonl').open('xb') as stream:
        os.chmod(stream.name,0o600)
        try:
            while clock()<deadline:
                tick=clock()
                answer=call(['y2-platform','status','--json','--reborn','--pss'])
                envelope={'schema':'org.y2linux.qualification-event/v1',
                          'host_wall_timestamp':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                          'host_monotonic_s':clock(),'ssh_observation_seconds':clock()-tick}
                if not answer['ok']:
                    gaps+=1;envelope.update(kind='transport_gap',reason=answer['reason'])
                    emit(stream,envelope)
                else:
                    sample=json.loads(answer['output']);check_identity(sample,expected)
                    sample['record']['workload']=profile
                    sample['record']['parameters'].update(host_poll_interval_seconds=interval,
                        workload_started_by='owner',observer='USB SSH polling; includes its overhead')
                    emit(stream,envelope|{'kind':'sample','sample':sample});count+=1
                remaining=min(deadline,tick+interval)-clock()
                if remaining>0: sleep(remaining)
            if count==0: failure='no_valid_candidate_samples'
        except (OSError,ValueError,KeyboardInterrupt) as error:
            failure=str(error) or type(error).__name__
    receipt={'schema':'org.y2linux.qualification-capture/v1','profile':profile,'expected':expected,
             'state':'CaptureIncomplete' if failure else 'CaptureComplete','failure':failure,
             'samples':count,'transport_gaps':gaps,'elapsed_seconds':clock()-start,'requested_seconds':seconds,
             'workload':PROFILES[profile][1],'automatic_physical_actions':False,
             'physical_acceptance':'PENDING_OWNER_REVIEW','endurance_qualified':False,
             'evidence_level':'IMPLEMENTED','bytes':total,
             'samples_sha256':hashlib.sha256((output/'samples.jsonl').read_bytes()).hexdigest()}
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    # Only bounded readonly kernel output, never NVRAM/calibration/credentials.
    answer=call(['sh','-c','dmesg -r | tail -c 65536'])
    if answer['ok']:
        (output/'kernel-tail.txt').write_text(answer['output']+'\n')
        os.chmod(output/'kernel-tail.txt',0o600)
    return receipt


def analyze(path):
    samples=gaps=0; previous=None; boots=set(); changes=collections.Counter(); maxima={}
    deltas=collections.Counter(); resets=collections.Counter(); growth={}; observed=set(); first_time={}
    def maximum(key,value):
        if isinstance(value,(int,float)) and not isinstance(value,bool): maxima[key]=max(maxima.get(key,value),value)
    def difference(name,before,after):
        if type(before) not in (int,float) or type(after) not in (int,float): return
        observed.add(name)
        if after<before: resets[name]+=1
        else: deltas[name]+=after-before
    total=0
    with Path(path).open('rb') as stream:
        for raw in stream:
            total+=len(raw)
            if len(raw)>1024**2 or total>MAX_BYTES: raise ValueError('capture_size_bound')
            value=json.loads(raw)
            if value.get('kind')=='transport_gap': gaps+=1;continue
            value=value.get('sample',value)
            if value.get('schema')=='org.y2linux.collection-summary/v1': continue
            if value.get('schema')!='org.y2linux.status/v1': raise ValueError('unexpected_record')
            record=value['record'];boot=record.get('boot_id');now=record.get('monotonic_ns')
            if not boot or type(now) is not int: raise ValueError('missing_boot_or_monotonic_identity')
            boots.add(boot);samples+=1;first_time.setdefault(boot,now)
            if len(boots)>128: raise ValueError('boot_count_bound')
            for zone in value.get('thermal',{}).get('zones',[]): maximum('die_millicelsius:'+str(zone.get('type')),zone.get('temperature_millicelsius'))
            for key in ('Cached','Slab','MemAvailable','LowFree','HighFree'):
                maximum('memory_kib:'+key,(value.get('memory',{}).get('meminfo') or {}).get(key))
            processes=value.get('memory',{}).get('processes',[])
            for process in processes:
                key=f"{boot}:{process.get('pid')}:{process.get('start_ticks')}"
                maximum('rss_kib:'+key,process.get('rss_kib'));maximum('pss_kib:'+key,process.get('pss_kib'))
                maximum('threads:'+key,process.get('threads'))
                if process.get('start_ticks') is not None and now-first_time[boot]>=60*10**9:
                    if key not in growth and len(growth)>=256: raise ValueError('process_series_bound')
                    growth.setdefault(key,Growth()).add((now-first_time[boot])/1e9,process.get('rss_kib'))
            if previous and previous['record']['boot_id']==boot:
                if now<=previous['record']['monotonic_ns']: raise ValueError('non_increasing_monotonic_sample')
                for core,percent in (utilization(previous['cpu'].get('ticks'),value['cpu'].get('ticks')) or {}).items(): maximum('cpu_percent:'+core,percent)
                for name in ('ctxt','intr','softirq'):
                    difference('cpu:'+name,previous['cpu'].get('counters',{}).get(name),value['cpu'].get('counters',{}).get(name))
                difference('power:wakeup_count',previous['cpu'].get('wakeup_count'),value['cpu'].get('wakeup_count'))
                for name in ('rx_bytes','tx_bytes','rx_errors','tx_errors','rx_dropped','tx_dropped'):
                    difference('wifi:'+name,previous.get('wifi',{}).get('traffic_counters',{}).get(name),value.get('wifi',{}).get('traffic_counters',{}).get(name))
                oldids={(p.get('pid'),p.get('start_ticks')) for p in previous.get('memory',{}).get('processes',[]) if p.get('start_ticks') is not None}
                newids={(p.get('pid'),p.get('start_ticks')) for p in processes if p.get('start_ticks') is not None}
                if oldids and oldids==newids:
                    for name in COUNTERS: difference('reborn:'+name,(previous.get('reborn_metrics') or {}).get(name),(value.get('reborn_metrics') or {}).get(name))
                for section in ('wifi','bluetooth'):
                    if previous.get(section,{}).get('state')!=value.get(section,{}).get('state'): changes[section+':observed_state_changes']+=1
                oldvol={v['path']:v.get('generation') for v in previous.get('storage',{}).get('volumes',[])}
                for volume in value.get('storage',{}).get('volumes',[]):
                    if volume['path'] in oldvol and oldvol[volume['path']]!=volume.get('generation'): changes['mount:'+volume['path']]+=1
            previous=value
    return {'schema':'org.y2linux.qualification-analysis/v1','samples':samples,'transport_gaps':gaps,
            'boot_ids':sorted(boots),'maxima':maxima,'counter_deltas':{k:deltas[k] for k in sorted(observed)},
            'counter_resets':dict(resets),'observed_transitions':dict(changes),
            'memory_growth_after_60s':{k:g.result() for k,g in growth.items()},
            'qualification':'PENDING_OWNER_REVIEW','missing_counters_are_not_zero':True,
            'limits':'Sampled observations can miss transient faults. No packet-loss, energy, electrical durability or leak diagnosis inferred.'}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    capture=sub.add_parser('capture');capture.add_argument('--profile',choices=PROFILES,required=True)
    capture.add_argument('--versions',type=Path,required=True);capture.add_argument('--output',type=Path)
    capture.add_argument('--seconds',type=int);capture.add_argument('--interval',type=int,default=30)
    capture.add_argument('--run',action='store_true');capture.add_argument('--private-key',type=Path);capture.add_argument('--known-hosts',type=Path)
    analyze_parser=sub.add_parser('analyze');analyze_parser.add_argument('samples',type=Path)
    args=parser.parse_args()
    if args.command=='analyze': result=analyze(args.samples)
    else:
        expected=identity(json.loads(args.versions.read_text()));seconds=args.seconds or PROFILES[args.profile][0]
        if not args.run:
            result={'profile':args.profile,'expected':expected,'seconds':seconds,'interval':args.interval,
                    'owner_workload':PROFILES[args.profile][1],'network_contacted':False,
                    'next':'Owner controls workload; --run with fresh --output, --private-key and verified --known-hosts only collects.'}
        else:
            if not args.output or not args.private_key or not args.known_hosts: parser.error('--run requires output and pinned owner SSH credentials')
            result=run(args.profile,expected,args.output,seconds,args.interval,ssh_options(args.private_key,args.known_hosts))
    print(json.dumps(result,indent=2))
    return 1 if result.get('state')=='CaptureIncomplete' else 0


if __name__=='__main__': raise SystemExit(main())
