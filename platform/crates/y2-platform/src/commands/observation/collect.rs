//! `y2-platform collect` (`collect.collect`): bounded streaming observation for workload, thermal and
//! endurance comparisons; one status line per sample, then a summary.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    process_identity, reborn_metrics,
    snapshot::{snapshot, utilization},
    util,
};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::io::Write;
use y2p_core::{
    clock,
    json::{dumps_sorted, Separators},
    Root,
};

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
    let mut previous: Option<Map<String, Value>> = None;
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
        // An input Python crashes on ends the stream as its traceback does (super::crash).
        let mut value = super::crashing(snapshot(root, &pids, options.pss, 0.0))?;
        let record = value["record"].as_object_mut().expect("record");
        record.insert("workload".into(), json!(label));
        if let Some(Value::Object(parameters)) = record.get_mut("parameters") {
            parameters.insert("interval_seconds".into(), json!(interval));
            parameters.insert("duration_seconds".into(), json!(seconds));
            parameters.insert("pss".into(), json!(options.pss));
            parameters.insert("warmup_seconds".into(), json!(warmup));
        }
        if let Some(previous) = previous.as_ref().filter(|p| !p.is_empty()) {
            if previous["record"]["boot_id"] == value["record"]["boot_id"] {
                let busy = utilization(previous["cpu"].get("ticks"), value["cpu"].get("ticks"));
                value["cpu"]["utilization_percent"] = busy;
            }
        }
        if options.reborn {
            value.insert("reborn_metrics".into(), reborn_metrics(root));
        }
        let age = clock::monotonic() - start;
        if age >= warmup as f64 {
            let boot = match &value["record"]["boot_id"] {
                Value::String(s) => s.clone(),
                _ => "None".into(),
            };
            for process in value["memory"]["processes"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let start_ticks = &process["start_ticks"];
                if start_ticks.is_null() {
                    continue;
                }
                let key = format!("{boot}:{}:{start_ticks}", process["pid"]);
                // PID reuse starts a new series; no process identity guessing.
                let index = match growth.iter().position(|(k, _)| *k == key) {
                    Some(index) => index,
                    None if growth.len() >= 256 => continue,
                    None => {
                        growth.push((key, Growth::default()));
                        growth.len() - 1
                    }
                };
                growth[index].1.add(age, process["rss_kib"].as_i64());
            }
        }
        samples += 1;
        failures += u64::from(
            value["readiness"]
                .as_object()
                .is_some_and(|r| r.values().any(|item| item["state"] == "Failed")),
        );
        util::sortable(&Value::Object(value.clone()))?;
        let line = dumps_sorted(&value, Separators::Spaced).map_err(Failure::value)? + "\n";
        bytes += line.len();
        if bytes > 64 * 1024 * 1024 {
            return Err(Failure::value("collection_output_limit_64MiB"));
        }
        if let Err(error) = out.write_all(line.as_bytes()).and_then(|()| out.flush()) {
            let (text, class) = y2p_core::py::os_error(&error, None);
            return Err(Failure::Os { text, class });
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
    record.insert("result".into(), json!("completed"));
    record.insert("failure".into(), Value::Null);
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
