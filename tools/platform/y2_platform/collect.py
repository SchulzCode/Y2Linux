"""Bounded streaming observation for workload, thermal and endurance comparisons."""
# SPDX-License-Identifier: GPL-2.0-only
import json
import time
from .observe import snapshot, utilization


class Growth:
    """Constant-memory least squares; positive growth is not a leak diagnosis."""
    def __init__(self):
        self.n = 0
        self.mean_t = self.mean_y = self.xx = self.xy = 0.0
        self.first = self.last = self.maximum = None

    def add(self, seconds, kib):
        if kib is None:
            return
        self.n += 1
        dt, dy = seconds - self.mean_t, kib - self.mean_y
        self.mean_t += dt / self.n
        self.mean_y += dy / self.n
        self.xx += dt * (seconds - self.mean_t)
        self.xy += dt * (kib - self.mean_y)
        self.first = kib if self.first is None else self.first
        self.last = kib
        self.maximum = kib if self.maximum is None else max(self.maximum, kib)

    def result(self):
        return {'samples': self.n, 'first_kib': self.first, 'last_kib': self.last,
                'maximum_kib': self.maximum,
                'slope_kib_per_hour': self.xy / self.xx * 3600 if self.xx > 0 else None,
                'interpretation': 'growth_observation_not_leak_diagnosis'}


def collect(ctx, stream, seconds=60, interval=5, pids=(), pss=False,
            workload='idle', warmup=60, reborn=False, sampler=snapshot, sleep=time.sleep):
    if not 1 <= seconds <= 86400 or not 1 <= interval <= 300 or not 0 <= warmup <= seconds or len(pids) > 32:
        raise ValueError('collection_bounds_exceeded')
    if not 1 <= len(workload) <= 96 or any(ord(c) < 32 for c in workload):
        raise ValueError('invalid_workload_label')
    start = time.monotonic()
    deadline = start + seconds
    previous = None
    growth = {}
    sample_count = failures = 0
    output_bytes = 0
    while time.monotonic() < deadline:
        sample_start = time.monotonic()
        value = sampler(ctx, pids, pss)
        value['record']['workload'] = workload
        value['record']['parameters'].update(interval_seconds=interval, duration_seconds=seconds,
                                             pss=pss, warmup_seconds=warmup)
        if previous and previous['record']['boot_id'] == value['record']['boot_id']:
            value['cpu']['utilization_percent'] = utilization(previous['cpu']['ticks'], value['cpu']['ticks'])
        if reborn:
            answer = ctx.command(['/usr/bin/rebornctl', 'metrics', '--json'], timeout=1)
            try:
                value['reborn_metrics'] = json.loads(answer['output']) if answer['ok'] else None
            except (TypeError, ValueError):
                value['reborn_metrics'] = None
        age = time.monotonic() - start
        if age >= warmup:
            for process in value['memory']['processes']:
                key = f"{value['record']['boot_id']}:{process['pid']}:{process.get('start_ticks')}"
                if process.get('start_ticks') is not None:
                    # PID reuse starts a new series; no process identity guessing.
                    if key not in growth and len(growth) >= 256:
                        continue
                    growth.setdefault(key, Growth()).add(age, process['rss_kib'])
        sample_count += 1
        failures += any(r['state'] == 'Failed' for r in value['readiness'].values())
        line = json.dumps(value, sort_keys=True, allow_nan=False) + '\n'
        output_bytes += len(line.encode())
        if output_bytes > 64 * 1024**2:
            raise ValueError('collection_output_limit_64MiB')
        stream.write(line)
        stream.flush()
        previous = value
        remaining = min(deadline, sample_start + interval) - time.monotonic()
        if remaining > 0:
            sleep(remaining)
    record = ctx.record(workload, {'duration_seconds': seconds, 'interval_seconds': interval})
    record.update(result='completed', failure=None)
    result = {'schema': 'org.y2linux.collection-summary/v1', 'record': record,
              'samples': sample_count, 'samples_with_failed_readiness': failures,
              'growth': {key: data.result() for key, data in growth.items()},
              'bytes': output_bytes, 'elapsed_seconds': time.monotonic() - start}
    stream.write(json.dumps(result, sort_keys=True) + '\n')
    stream.flush()
    return result
