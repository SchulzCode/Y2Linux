//! `y2-platform collect` (`collect.collect`): bounded streaming observation for workload, thermal and
//! endurance comparisons; one status line per sample, then a summary.
// SPDX-License-Identifier: GPL-2.0-only
use super::snapshot::readiness::Readiness;
use super::{
    process_identity, reborn_metrics,
    snapshot::{snapshot, utilization, Snapshot},
    util,
};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::io::Write;
use y2p_core::{clock, json::dumps_sorted, Root};

pub struct Options {
    pub seconds: i64,
    pub interval: i64,
    pub warmup: i64,
    pub workload: String,
    pub pids: Vec<i64>,
    pub pss: bool,
    pub reborn: bool,
}

/// Constant-memory least squares; positive growth is not a leak diagnosis.
#[derive(Default)]
struct Growth {
    n: u64,
    mean_t: f64,
    mean_y: f64,
    xx: f64,
    xy: f64,
    first: Option<i64>,
    last: Option<i64>,
    maximum: Option<i64>,
}

impl Growth {
    fn add(&mut self, seconds: f64, kib: Option<i64>) {
        let Some(kib) = kib else { return };
        let y = kib as f64;
        self.n += 1;
        let (dt, dy) = (seconds - self.mean_t, y - self.mean_y);
        self.mean_t += dt / self.n as f64;
        self.mean_y += dy / self.n as f64;
        self.xx += dt * (seconds - self.mean_t);
        self.xy += dt * (y - self.mean_y);
        self.first = self.first.or(Some(kib));
        self.last = Some(kib);
        self.maximum = Some(self.maximum.map_or(kib, |m| m.max(kib)));
    }

    fn result(&self) -> Value {
        json!({"samples": self.n, "first_kib": self.first, "last_kib": self.last,
               "maximum_kib": self.maximum,
               "slope_kib_per_hour": if self.xx > 0.0 { Some(self.xy / self.xx * 3600.0) } else { None },
               "interpretation": "growth_observation_not_leak_diagnosis"})
    }
}

pub fn run(root: &Root, options: &Options) -> Outcome {
    let seconds = options.seconds;
    let interval = options.interval;
    let warmup = options.warmup;
    if !(1..=86_400).contains(&seconds)
        || !(1..=300).contains(&interval)
        || !(0..=seconds).contains(&warmup)
        || options.pids.len() > 32
    {
        return Err(Failure::value("collection_bounds_exceeded"));
    }
    let label = &options.workload;
    let length = label.chars().count();
    if !(1..=96).contains(&length) || label.chars().any(|c| (c as u32) < 32) {
        return Err(Failure::value("invalid_workload_label"));
    }
    // The pacing sleeps in real time, so it reads the kernel's clock, not the root's (a test clock
    // standing still would never let the loop end); the snapshots read the root's.
    let start = clock::monotonic();
    let deadline = start + seconds as f64;
    let mut previous: Option<Snapshot> = None;
    let mut growth: Vec<(String, Growth)> = Vec::new();
    let (mut samples, mut failures, mut bytes) = (0u64, 0u64, 0usize);
    let mut out = std::io::stdout().lock();
    while clock::monotonic() < deadline {
        let sample_start = clock::monotonic();
        let mut pids = options.pids.clone();
        if options.reborn {
            if let Some((pid, _)) = process_identity(root, "process", "/usr/bin/reborn") {
                if !pids.contains(&pid) {
                    pids.push(pid);
                    pids.truncate(32);
                }
            }
        }
        // A refused snapshot ends the stream with its refusal record after the lines so far.
        let mut value = snapshot(root, &pids, options.pss, 0.0)?;
        value.record.workload = label.to_string();
        {
            let parameters = &mut value.record.parameters;
            parameters.insert("interval_seconds".into(), json!(interval));
            parameters.insert("duration_seconds".into(), json!(seconds));
            parameters.insert("pss".into(), json!(options.pss));
            parameters.insert("warmup_seconds".into(), json!(warmup));
        }
        // Utilisation since the previous sample of the same boot.
        if let Some(previous) = &previous {
            if previous.record.boot_id == value.record.boot_id {
                value.cpu.utilization_percent =
                    utilization(previous.cpu.ticks.as_ref(), value.cpu.ticks.as_ref());
            }
        }
        if options.reborn {
            value.reborn_metrics = Some(reborn_metrics(root));
        }
        let age = clock::monotonic() - start;
        if age >= warmup as f64 {
            let boot = value
                .record
                .boot_id
                .clone()
                .unwrap_or_else(|| "None".into());
            for process in &value.memory.processes {
                let Some(start_ticks) = process.start_ticks else {
                    continue;
                };
                let key = format!("{boot}:{}:{start_ticks}", process.pid);
                // PID reuse starts a new series; no process identity guessing.
                let index = match growth.iter().position(|(k, _)| *k == key) {
                    Some(index) => index,
                    None if growth.len() >= 256 => continue,
                    None => {
                        growth.push((key, Growth::default()));
                        growth.len() - 1
                    }
                };
                growth[index].1.add(age, process.rss_kib);
            }
        }
        samples += 1;
        failures += u64::from(
            value
                .readiness
                .items()
                .iter()
                .any(|item| item.state == Readiness::Failed),
        );
        value.printable()?;
        let line = dumps_sorted(&value).map_err(Failure::value)? + "\n";
        bytes += line.len();
        if bytes > 64 * 1024 * 1024 {
            return Err(Failure::value("collection_output_limit_64MiB"));
        }
        if let Err(error) = out.write_all(line.as_bytes()).and_then(|()| out.flush()) {
            return Err(Failure::from(error));
        }
        previous = Some(value);
        let remaining = deadline.min(sample_start + interval as f64) - clock::monotonic();
        if remaining > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(remaining));
        }
    }
    let mut parameters = Map::new();
    parameters.insert("duration_seconds".into(), json!(seconds));
    parameters.insert("interval_seconds".into(), json!(interval));
    let mut record = util::record(root, label, parameters);
    record.result = json!("completed");
    record.failure = Value::Null;
    let growth: Map<String, Value> = growth
        .iter()
        .map(|(key, series)| (key.clone(), series.result()))
        .collect();
    let summary = json!({"schema": "org.y2linux.collection-summary/v1", "record": record,
        "samples": samples, "samples_with_failed_readiness": failures,
        "growth": growth, "bytes": bytes, "elapsed_seconds": clock::monotonic() - start});
    Reply::sorted(&summary, 0)
}

#[cfg(test)]
#[path = "collect_tests.rs"]
mod collect_tests;
