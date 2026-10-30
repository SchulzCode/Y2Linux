//! The `memory` section: meminfo, vmstat, pressure and the processes asked for.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{counters, int, key_values, split_ws};
use crate::reply::Failure;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use y2p_core::Root;

/// The `memory` section (in this key order; null for a table that could not be read).
#[derive(Serialize)]
struct Memory {
    unit: &'static str,
    /// Each meminfo line's first value (kB), by name.
    meminfo: Option<BTreeMap<String, Option<i64>>>,
    vmstat: Option<Map<String, Value>>,
    pressure: Option<String>,
    processes: Vec<Process>,
}

/// One of the processes asked for (`--pid`).
#[derive(Serialize)]
struct Process {
    pid: i64,
    name: Option<String>,
    start_ticks: Option<i64>,
    rss_kib: Option<i64>,
    pss_kib: Option<i64>,
    threads: Option<i64>,
    voluntary_context_switches: Option<i64>,
    involuntary_context_switches: Option<i64>,
}

/// The first word of a table's value, as an integer (`VmRSS: 1234 kB`).
fn first_word(table: &BTreeMap<String, String>, key: &str) -> Option<i64> {
    table
        .get(key)
        .and_then(|v| int(v.split(' ').next().unwrap_or_default()))
}

pub fn memory(root: &Root, pids: &[i64], pss: bool) -> Result<Map<String, Value>, Failure> {
    let mut values = BTreeMap::new();
    for (k, v) in key_values(root.read("/proc/meminfo").as_deref(), ":") {
        if !v.is_empty() {
            values.insert(k, split_ws(&v).first().copied().and_then(int));
        }
    }
    let mut processes = Vec::new();
    for &pid in pids {
        if !(1..=4_194_304).contains(&pid) {
            return Err(Failure::value("invalid pid"));
        }
        let status = key_values(root.read(&format!("/proc/{pid}/status")).as_deref(), ":");
        let stat = root.read(&format!("/proc/{pid}/stat")).unwrap_or_default();
        let after = stat.rsplit_once(") ").map_or(stat.as_str(), |(_, r)| r);
        let fields = split_ws(after);
        let smaps = if pss {
            key_values(
                root.read(&format!("/proc/{pid}/smaps_rollup")).as_deref(),
                ":",
            )
        } else {
            Default::default()
        };
        let lookup = |key: &str| status.get(key).and_then(|v| int(v));
        processes.push(Process {
            pid,
            name: status.get("Name").cloned(),
            start_ticks: fields.get(19).and_then(|f| int(f)),
            rss_kib: first_word(&status, "VmRSS"),
            pss_kib: first_word(&smaps, "Pss"),
            threads: lookup("Threads"),
            voluntary_context_switches: lookup("voluntary_ctxt_switches"),
            involuntary_context_switches: lookup("nonvoluntary_ctxt_switches"),
        });
    }
    let vmstat = counters(root.read("/proc/vmstat").as_deref());
    let memory = Memory {
        unit: "KiB",
        meminfo: (!values.is_empty()).then_some(values),
        vmstat: (!vmstat.is_empty()).then_some(vmstat),
        pressure: root.read("/proc/pressure/memory"),
        processes,
    };
    match serde_json::to_value(memory) {
        Ok(Value::Object(section)) => Ok(section),
        _ => Err(Failure::value("memory_section_unserializable")),
    }
}
