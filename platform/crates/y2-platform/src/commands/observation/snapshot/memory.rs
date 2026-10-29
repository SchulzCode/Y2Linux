//! The `memory` section: meminfo, vmstat, pressure and the processes asked for.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{counters, key_values, number, split_ws, text};
use crate::reply::Failure;
use serde_json::{json, Map, Value};
use y2p_core::Root;

pub fn memory(root: &Root, pids: &[i64], pss: bool) -> Result<Map<String, Value>, Failure> {
    let mut values = Map::new();
    for (k, v) in key_values(root.read("/proc/meminfo").as_deref(), ":") {
        if !v.is_empty() {
            values.insert(k, number(split_ws(&v).first().copied()));
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
        let first_word = |map: &std::collections::BTreeMap<String, String>, key: &str| {
            number(
                map.get(key)
                    .map(|v| v.split(' ').next().unwrap_or_default()),
            )
            .clone()
        };
        let lookup = |key: &str| number(status.get(key).map(String::as_str));
        processes.push(json!({
            "pid": pid,
            "name": status.get("Name"),
            "start_ticks": if fields.len() > 19 { number(Some(fields[19])) } else { Value::Null },
            "rss_kib": first_word(&status, "VmRSS"),
            "pss_kib": first_word(&smaps, "Pss"),
            "threads": lookup("Threads"),
            "voluntary_context_switches": lookup("voluntary_ctxt_switches"),
            "involuntary_context_switches": lookup("nonvoluntary_ctxt_switches"),
        }));
    }
    let vmstat = counters(root.read("/proc/vmstat").as_deref());
    let mut result = Map::new();
    result.insert("unit".into(), json!("KiB"));
    result.insert(
        "meminfo".into(),
        if values.is_empty() {
            Value::Null
        } else {
            Value::Object(values)
        },
    );
    result.insert(
        "vmstat".into(),
        if vmstat.is_empty() {
            Value::Null
        } else {
            Value::Object(vmstat)
        },
    );
    result.insert("pressure".into(), text(root.read("/proc/pressure/memory")));
    result.insert("processes".into(), Value::Array(processes));
    Ok(result)
}
