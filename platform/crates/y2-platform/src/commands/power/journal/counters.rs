//! The idle counters a journal row carries (cpuidle, scheduler, runtime PM, frequency, idle and clock
//! blockers).
// SPDX-License-Identifier: GPL-2.0-only
use super::glob_name;
use y2p_core::{
    json::{Object, ObjectExt},
    power_text::python_split,
    root::{python_int, read},
    Root,
};

/// `idle_counters(ctx)`: cheap cumulative counters only when a journal row is already due.
///
/// No AUXADC, subprocess, RPC, tracing, or additional periodic wakeup. Missing interfaces stay null.
/// Hotplug can reset per-core counters; readers must not turn decreasing values into negative rates.
pub fn idle_counters(root: &Root) -> Object {
    let started = process_time();
    let mut idle = Object::new();
    for cpu in 0..4 {
        let mut states = Object::new();
        for index in 0..3 {
            let base = format!("/sys/devices/system/cpu/cpu{cpu}/cpuidle/state{index}");
            if let Some(name) = root.read_limited(&format!("{base}/name"), 64) {
                let mut fields = Object::new();
                for field in ["usage", "time", "rejected", "disable"] {
                    fields.set(field, root.integer(&format!("{base}/{field}")));
                }
                states.set(&name, fields);
            }
        }
        if !states.is_empty() {
            idle.set(&cpu.to_string(), states);
        }
    }
    let mut scheduler = Object::new();
    for line in root.read("/proc/stat").unwrap_or_default().lines() {
        let fields = python_split(line);
        if let Some(name) = fields
            .first()
            .filter(|f| ["ctxt", "intr", "processes"].contains(f))
        {
            scheduler.set(name, fields.get(1).and_then(|v| python_int(v)));
        }
    }
    let mut runtime = Object::new();
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
            runtime.set(&name, read(&path, 64));
        }
    }
    let named = |pattern: &str, limit: usize| {
        let mut out = Object::new();
        for path in root.glob(pattern) {
            out.set(&glob_name(&path), read(&path, limit));
        }
        out
    };
    let mut result = Object::new()
        .with(
            "online",
            root.read_limited("/sys/devices/system/cpu/online", 64),
        )
        .with("cpuidle", idle)
        .with("scheduler", scheduler)
        .with("runtime_pm", runtime)
        .with(
            "frequency_khz",
            root.integer("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq"),
        )
        .with(
            "frequency_ticks",
            root.read_limited(
                "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
                512,
            ),
        )
        .with(
            "idle_rejections",
            named("/sys/module/idle/parameters/*", 128),
        )
        .with(
            "clock_blockers",
            named("/sys/module/clocks/parameters/*blockers", 128),
        )
        .with(
            "counter_reset_rule",
            "decreasing per-core counters are unknown, not negative rates",
        );
    result.set("collector_cpu_seconds", process_time() - started);
    result
}

/// `time.process_time()`.
fn process_time() -> f64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid timespec and a clock every Linux has.
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut now) };
    now.tv_sec as f64 + now.tv_nsec as f64 * 1e-9
}
