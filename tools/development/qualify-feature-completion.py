#!/usr/bin/env python3
"""One integrated owner-run qualification entrypoint. Default is an offline plan.

The assistant prepares this tool but does not run its exercise or sleep modes.
Read-only observation can use either SSH path. Mutating launches run ONCE on one
probed host; transport failure never replays a launch on the alternate transport.
No flashing, raw storage writes, repartitioning or voltage programming exists.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import time

CASES = {
    'identity': 'Exact package/source pair, boot ID and taint before and after each lane.',
    'cpu': 'GPT6/GPT4/PPI29, highres/NO_HZ, schedutil/QoS, guarded OPPs, automatic parking.',
    'c2': 'Real sustained idle, actual C2 entries/residency and exact restore; legitimate clock blockers remain.',
    'c3': 'Read preflight. Entry requires CPU0-only/secondaries off, admitted OPP, domains/clocks, CIRQ/deadline; never bypass a failed predicate.',
    'sleep': 'Backstopped staged PM, then RTC, Power and five RTC cycles; SAME boot ID and userspace/product restoration.',
    'storage': 'Negotiated eMMC/SD mode, command/data tuning eyes, runtime PM; filesystem scratch hash and no new faults. Long integrity remains a separate duration within this run.',
    'usb': 'Repeated bidirectional hash transfers with independent Wi-Fi observer; DMA progress/storm/abort counters; cable and PC reconnect require owner action.',
    'battery': 'Voltage/estimated SOC, source/current ceilings, shutdown checkpoint. Low voltage uses host policy fixtures; never deliberately discharge below hard floor.',
    'wired': 'ALSA contract plus low-bit fixture, I2S payload/slot/rate capture and listening; 44.1/48/88.2/96 family transitions. Capture equipment required for native precision proof.',
    'codecs': 'For each enabled endpoint: peer/mutual/preference/negotiated/active PCM; load/thermal/reconnect/fallback; SBC XQ bitpool; LDAC 303/606/909 and 330/660/990, actual ABR transitions.',
    'coexistence': 'Capture Wi-Fi idle/scan/transfer intervals for each enabled active codec; packet/drop/load counters.',
    'product': 'Current Product v2 wheel/EQ/sleep/refusal/boot-ready-frame/shutdown-dark-frame/listening observations.',
    'operations': 'Redacted export/retrieval; preserving beta update and recovery rehearsal remain explicit owner operations, never automatic flashing.',
}


def load_legacy():
    path = Path(__file__).with_name('qualify-cpu-fix03.py')
    spec = importlib.util.spec_from_file_location('feature_cpu', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Campaign:
    def __init__(self, args):
        self.args = args
        self.manifest = json.loads((args.package/'manifest.json').read_text())
        self.output = args.output
        self.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.results = []
        self.boot = None
        self.legacy = load_legacy()
        legacy_args = argparse.Namespace(host=args.host, wifi_host=args.wifi_host,
            output=self.output/'platform', package=args.package, allow_warm_reboot=False)
        class SingleLaunchRun(self.legacy.Run):
            def remote(self, body, timeout=40, hosts=None, required=True):
                # Some retained helpers combine reads and writes. Default every
                # such call to one host; a lost response never repeats a write.
                if hosts is None:
                    try:
                        hosts = [self.live_host()]
                    except RuntimeError:
                        if not required:
                            return None
                        raise
                return super().remote(body, timeout, hosts=hosts, required=required)
        self.platform = SingleLaunchRun(legacy_args, self.manifest)

    def record(self, name, status, evidence):
        row = {'case': name, 'state': status, 'evidence': evidence}
        self.results.append(row)
        (self.output/'results.json').write_text(json.dumps(self.results, indent=2)+'\n')
        print(name+': '+status, flush=True)

    def command(self, argv, mutate=False, timeout=45):
        # Read retries may use Wi-Fi. Writes use only a previously probed host.
        hosts = [self.platform.live_host()] if mutate else [h for h in (self.args.host,self.args.wifi_host) if h]
        errors = []
        for host in hosts:
            try:
                result = subprocess.run(['ssh', '-T', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes',
                    '-oConnectTimeout=5', '-oServerAliveInterval=3', '-oServerAliveCountMax=2', host,
                    shlex.join(argv)], capture_output=True, timeout=timeout)
                if len(result.stdout) > 2*1024**2:
                    raise ValueError('remote_output_limit')
                if result.returncode == 0:
                    return json.loads(result.stdout)
                errors.append({'transport': host, 'exit': result.returncode})
            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                errors.append({'transport': host, 'failure': type(error).__name__})
        raise RuntimeError(json.dumps(errors))

    def identity(self):
        value = self.command(['y2-platform','status'])
        record = value['record']
        for field, key in (('build_git_commit','y2linux_commit'),('reborn_source_commit','reborn_commit'),
                           ('kernel_version','kernel'),('rootfs_version','rootfs_release')):
            if self.manifest[field] != record[key]:
                raise ValueError('candidate_identity_mismatch:'+field)
        if not record.get('boot_id') or self.boot and self.boot != record['boot_id']:
            raise ValueError('unexpected_boot_change_stop_exercises')
        if value.get('system', {}).get('kernel_taint') != 0:
            raise ValueError('kernel_taint_not_zero_stop_exercises')
        self.boot = record['boot_id']
        return value

    def observe(self, label):
        value = self.identity()
        self.record(label, 'OBSERVED', value)
        for case, argv in (('health',['y2-platform','health','--full']),
                           ('codecs',['y2-platform','codec-settings']),
                           ('sleep-contract',['y2-platform','sleep','status'])):
            try: self.record(label+'-'+case, 'OBSERVED', self.command(argv))
            except RuntimeError as error: self.record(label+'-'+case, 'FAILED', str(error))

    def exercises(self):
        self.platform.identity()
        awake = []
        for name, action in (('timers',self.platform.timers),('qos',self.platform.qos),
            ('mmc',self.platform.mmc),('parking',self.platform.slidle),
            ('c2',self.platform.slidle_radios_off),('dvfs',self.platform.dvfs),('usb',self.platform.usb_stress)):
            # Independent source lanes continue after an ordinary test failure;
            # lost recovery, changed boot or failed identity stops mutation.
            self.identity()
            try:
                ok = action()
                awake.append(bool(ok)); self.record(name, 'PASS' if ok else 'FAIL', 'platform receipts')
            except Exception as error:
                awake.append(False); self.record(name, 'FAILED', type(error).__name__)
            self.identity()
        if self.args.sleep:
            proof, _ = self.platform.journal()
            if not all(awake) or not proof:
                self.record('sleep', 'BLOCKED', 'awake regression or SRAM proof failed')
                return
            charger = self.platform.remote("print(json.dumps(charger()))")
            if charger['expect_refusal']:
                self.platform.suspend('devices', expect_refusal=True)
                input('Unplug charging USB; keep the Wi-Fi observer available. Press Enter when ready. ')
                charger = self.platform.remote("print(json.dumps(charger()))", hosts=[self.args.wifi_host])
                if charger['expect_refusal']:
                    self.record('sleep', 'BLOCKED', 'charger remains active'); return
            for mode in ('freezer','devices','platform','processors','core'):
                if not self.platform.suspend(mode):
                    self.record('sleep', 'FAIL', 'staged '+mode); return
            for name, seconds, power in [('RTC',30,False),('Power',120,True)]+[('cycle-'+str(i),30,False) for i in range(5)]:
                if not self.platform.suspend('none',alarm=seconds,power=power):
                    self.record('sleep', 'FAIL', name); return
                self.observe('after-'+name)
            self.record('sleep', 'PASS', 'same-boot RTC/Power and five cycles; inspect product restoration receipts')
        self.record('diagnostic-export', 'OBSERVED', self.command(['y2-platform','export-diagnostics'],mutate=True,timeout=90))

    def finish(self):
        covered = {r['case'] for r in self.results}
        for name, procedure in CASES.items():
            if name not in covered:
                self.record(name, 'PHYSICAL_PROOF_PENDING', procedure)
        self.platform.summary(None)
        sums = ''.join(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+str(p.relative_to(self.output))+'\n'
                       for p in sorted(self.output.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS')
        (self.output/'SHA256SUMS').write_text(sums)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, default=Path('out/y2linux-community-beta-feature-completion-candidate'))
    parser.add_argument('--output', type=Path, default=Path('out/feature-qualification-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')))
    parser.add_argument('--host', default='y2')
    parser.add_argument('--wifi-host')
    parser.add_argument('--run', action='store_true', help='read-only baseline observation')
    parser.add_argument('--exercise', action='store_true', help='owner-run bounded platform exercises')
    parser.add_argument('--sleep', action='store_true', help='owner-run guarded RTC/Power sleep after all awake gates pass')
    parser.add_argument('--observe-seconds', type=int, default=0, help='capture declared codec/UX/coexistence workload without changing it')
    args = parser.parse_args()
    if not args.run:
        print(json.dumps({'schema':'org.y2linux.feature-qualification-plan/v1','cases':CASES,
                          'default':'offline plan only','exercise':'requires --run --exercise and independent --wifi-host'},indent=2)); return
    if not 0 <= args.observe_seconds <= 28800:
        parser.error('observation duration must be 0..28800 seconds')
    if args.sleep and not args.exercise or args.exercise and not args.wifi_host:
        parser.error('sleep requires exercise; exercise requires an independent Wi-Fi host')
    os.umask(0o077)
    campaign = Campaign(args)
    try:
        campaign.observe('baseline')
        if args.exercise: campaign.exercises()
        deadline = time.monotonic()+args.observe_seconds
        while time.monotonic() < deadline:
            time.sleep(min(30,max(0,deadline-time.monotonic())))
            campaign.observe('workload-'+str(len(campaign.results)))
        campaign.observe('final')
    finally:
        campaign.finish()


if __name__ == '__main__': main()
