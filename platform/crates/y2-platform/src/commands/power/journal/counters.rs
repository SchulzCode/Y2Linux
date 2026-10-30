//! The idle counters a journal row carries (cpuidle, scheduler, runtime PM, frequency, idle and clock
//! blockers): the power journal's rows and the suspend group's stage rows in the same journal.
// SPDX-License-Identifier: GPL-2.0-only
use super::glob_name;
use serde::Serialize;
use std::collections::BTreeMap;
use y2p_core::clock::process_time;
use y2p_core::{
    power_text::python_split,
    root::{parse_int, read},
    text::splitlines,
    Root,
};

/// `idle_counters(ctx)`: cheap cumulative counters only when a journal row is already due.
///
/// No AUXADC, subprocess, RPC, tracing, or additional periodic wakeup. Missing interfaces stay null.
/// Hotplug can reset per-core counters; readers must not turn decreasing values into negative rates.
pub fn idle_counters(root: &Root) -> IdleCounters {
    let started = process_time();
    let mut cpuidle = BTreeMap::new();
    for cpu in 0..4 {
        let mut states = BTreeMap::new();
        for index in 0..3 {
            let base = format!("/sys/devices/system/cpu/cpu{cpu}/cpuidle/state{index}");
            if let Some(name) = root.read_limited(&format!("{base}/name"), 64) {
                let field = |field: &str| root.integer(&format!("{base}/{field}"));
                let counts = IdleState {
                    usage: field("usage"),
                    time: field("time"),
                    rejected: field("rejected"),
                    disable: field("disable"),
                };
                states.insert(name, counts);
            }
        }
        if !states.is_empty() {
            cpuidle.insert(cpu.to_string(), states);
        }
    }
    let mut scheduler = BTreeMap::new();
    for line in splitlines(&root.read("/proc/stat").unwrap_or_default()) {
        let fields = python_split(line);
        if let Some(name) = fields
            .first()
            .filter(|f| ["ctxt", "intr", "processes"].contains(f))
        {
            scheduler.insert(name.to_string(), fields.get(1).and_then(|v| parse_int(v)));
        }
    }
    let mut runtime_pm = BTreeMap::new();
    for path in root.glob("/sys/bus/platform/devices/*/power/runtime_status") {
        let name = path
            .parent()
            .and_then(|p| p.parent())
            .map(glob_name)
            .unwrap_or_default();
        if [
            "11200000.",
            "11220000.",
            "11230000.",
            "11240000.",
            "13010000.",
            "14007000.",
            "14008000.",
            "18070000.",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        {
            runtime_pm.insert(name, read(&path, 64));
        }
    }
    let named = |pattern: &str, limit: usize| {
        root.glob(pattern)
            .iter()
            .map(|path| (glob_name(path), read(path, limit)))
            .collect()
    };
    IdleCounters {
        online: root.read_limited("/sys/devices/system/cpu/online", 64),
        cpuidle,
        scheduler,
        runtime_pm,
        frequency_khz: root.integer("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq"),
        frequency_ticks: root.read_limited(
            "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
            512,
        ),
        idle_rejections: named("/sys/module/idle/parameters/*", 128),
        clock_blockers: named("/sys/module/clocks/parameters/*blockers", 128),
        counter_reset_rule: "decreasing per-core counters are unknown, not negative rates",
        collector_cpu_seconds: process_time() - started,
    }
}

/// The cheap cumulative counters of a journal row: per-core idle states, scheduler counts, runtime
/// PM of the watched devices, the CPU frequency, idle rejections and clock blockers.
#[derive(Serialize, Debug)]
pub struct IdleCounters {
    online: Option<String>,
    /// CPU number → idle state name → its counters.
    cpuidle: BTreeMap<String, BTreeMap<String, IdleState>>,
    scheduler: BTreeMap<String, Option<i64>>,
    runtime_pm: BTreeMap<String, Option<String>>,
    frequency_khz: Option<i64>,
    frequency_ticks: Option<String>,
    idle_rejections: BTreeMap<String, Option<String>>,
    clock_blockers: BTreeMap<String, Option<String>>,
    counter_reset_rule: &'static str,
    collector_cpu_seconds: f64,
}

/// One cpuidle state's counters.
#[derive(Serialize, Debug)]
pub struct IdleState {
    usage: Option<i64>,
    time: Option<i64>,
    rejected: Option<i64>,
    disable: Option<i64>,
}
