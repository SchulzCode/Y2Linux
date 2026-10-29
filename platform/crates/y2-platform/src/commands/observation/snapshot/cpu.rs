//! The `cpu` section: load, frequency policies, idle states and their completion counters, the
//! timer runtime, and what the power-management drivers report.
// SPDX-License-Identifier: GPL-2.0-only
use super::idle::idle_completion;
use crate::commands::observation::util::{
    self, counters, findall_map, int, name, number, parent_name, read, read_limited, reads_by_name,
    split_ws, splitlines, text,
};
use crate::{regex, reply::Failure};
use serde_json::{json, Map, Value};
use y2p_core::{command, Root};

/// Whether every CPU runs high-resolution timers and tickless idle, from /proc/timer_list.
pub fn timer_runtime(root: &Root) -> Result<Map<String, Value>, Failure> {
    // On the player through a bounded child (0.2 s, 256 KiB), as the wakeup counter: a slow procfs
    // read cannot hold the snapshot.
    let text = if root.is_real() {
        let answer =
            command::run_with(root, &["/bin/cat", "/proc/timer_list"], 0.2, 262_144, false);
        if answer.ok {
            answer.output
        } else {
            None
        }
    } else {
        root.read_limited("/proc/timer_list", 262_144)
    };
    let text = text.unwrap_or_default();
    let mut cpus: Vec<Map<String, Value>> = Vec::new();
    for block in regex!(r"(?m)^cpu:\s*").split(&text).skip(1) {
        let lines = splitlines(block);
        // A `cpu:` with nothing after it (the text cut short): no line to number the CPU by.
        let Some(first) = lines.first() else {
            return Err(Failure::value("invalid_timer_list"));
        };
        if !util::isdigit(first) {
            continue;
        }
        // `int(first)`: a superscript or circled digit passes isdigit() but is no int.
        let mut core = Map::new();
        core.insert("cpu".into(), json!(util::int_or_raise(first)?));
        let fields = findall_map(
            regex!(r"(?m)^\s*\.(hres_active|nohz|tick_stopped|nr_hangs)\s*:\s*(\d+)\s*$"),
            block,
        );
        for (key, value) in fields {
            core.insert(key, json!(util::int_or_raise(&value)?));
        }
        cpus.push(core);
    }
    // True or false only when every CPU reports the field.
    let observed = |key: &str| -> Value {
        let values: Option<Vec<i64>> = cpus
            .iter()
            .map(|c| c.get(key).and_then(Value::as_i64))
            .collect();
        match values {
            Some(values) if !values.is_empty() => {
                json!(values
                    .iter()
                    .all(|v| if key == "nohz" { *v > 0 } else { *v == 1 }))
            }
            _ => Value::Null,
        }
    };
    let mut result = Map::new();
    result.insert("highres_active".into(), observed("hres_active"));
    result.insert("no_hz_active".into(), observed("nohz"));
    result.insert("source".into(), json!("/proc/timer_list"));
    result.insert(
        "reason".into(),
        if cpus.is_empty() {
            json!("runtime_timer_state_unavailable")
        } else {
            Value::Null
        },
    );
    result.insert(
        "per_cpu".into(),
        Value::Array(cpus.into_iter().map(Value::Object).collect()),
    );
    Ok(result)
}

/// The `cpu` section.
pub fn cpu(root: &Root) -> Result<Map<String, Value>, Failure> {
    let stat = root.read("/proc/stat");
    let mut ticks = Map::new();
    for line in splitlines(stat.as_deref().unwrap_or_default()) {
        let fields = split_ws(line);
        if fields
            .first()
            .is_some_and(|f| regex!(r"\Acpu[0-9]*\z").is_match(f))
        {
            let values: Vec<Value> = fields
                .iter()
                .skip(1)
                .take(8)
                .map(|v| number(Some(v)))
                .collect();
            ticks.insert(fields[0].to_owned(), Value::Array(values));
        }
    }
    let ceiling = root.integer("/sys/module/cpu_dvfs/parameters/qualification_max_khz");
    let mut policies = Vec::new();
    for path in root.glob("/sys/devices/system/cpu/cpufreq/policy*") {
        let mut fields = Map::new();
        for field in [
            "affected_cpus",
            "scaling_governor",
            "scaling_cur_freq",
            "scaling_min_freq",
            "scaling_max_freq",
            "scaling_available_frequencies",
            "cpuinfo_min_freq",
            "cpuinfo_max_freq",
            "cpuinfo_cur_freq",
            "scaling_driver",
        ] {
            fields.insert(field.into(), text(read(&path.join(field))));
        }
        let maximum = fields
            .get("scaling_max_freq")
            .and_then(Value::as_str)
            .and_then(int);
        let admitted = ceiling.zip(maximum).map(|(c, m)| c.min(m));
        let available = fields
            .get("scaling_available_frequencies")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let opp: Vec<Value> = split_ws(&available)
            .into_iter()
            .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
            .filter_map(int)
            .filter(|v| admitted.is_some_and(|a| *v <= a))
            .map(Value::from)
            .collect();
        fields.insert("admitted_opp_khz".into(), Value::Array(opp));
        fields.insert("name".into(), json!(name(&path)));
        let states = counters(read(&path.join("stats/time_in_state")).as_deref());
        fields.insert(
            "time_in_state".into(),
            if states.is_empty() {
                Value::Null
            } else {
                Value::Object(states)
            },
        );
        fields.insert("time_in_state_unit".into(), json!("USER_HZ_ticks"));
        fields.insert("frequency_unit".into(), json!("kHz"));
        policies.push(Value::Object(fields));
    }
    let mut idle = Vec::new();
    for path in root.glob("/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*") {
        let cpu_name = path.parent().map(parent_name).unwrap_or_default();
        let value = |f: &str| number(read(&path.join(f)).as_deref());
        let mut state = Map::new();
        state.insert("cpu".into(), json!(cpu_name));
        state.insert("state".into(), json!(name(&path)));
        state.insert("name".into(), text(read(&path.join("name"))));
        state.insert("time_us".into(), value("time"));
        state.insert("usage".into(), value("usage"));
        state.insert("exit_latency_us".into(), value("latency"));
        state.insert("target_residency_us".into(), value("residency"));
        state.insert("rejected".into(), value("rejected"));
        state.insert("disabled".into(), value("disable"));
        idle.push(state);
    }
    let totals = counters(stat.as_deref());
    // This sysfs read is a suspend handshake: it blocks while a wakeup source is active, even with
    // O_NONBLOCK. Keep it outside the observer process and use the existing bounded runner to
    // kill/reap an interruptible stalled read.
    let wakeup_path = root.path("/sys/power/wakeup_count");
    let wakeup = command::run_with(
        root,
        &["/bin/cat", &wakeup_path.to_string_lossy()],
        0.1,
        64,
        false,
    );
    let wakeup_count = if wakeup.ok {
        wakeup.output.as_deref().and_then(int).filter(|v| *v >= 0)
    } else {
        None
    };
    let completion = idle_completion(root, &idle);
    let mut result = Map::new();
    result.insert(
        "online".into(),
        text(root.read("/sys/devices/system/cpu/online")),
    );
    result.insert("load_average".into(), text(root.read("/proc/loadavg")));
    result.insert(
        "ticks".into(),
        if ticks.is_empty() {
            Value::Null
        } else {
            Value::Object(ticks)
        },
    );
    result.insert("ticks_unit".into(), json!("USER_HZ_ticks"));
    result.insert("utilization_percent".into(), Value::Null);
    result.insert("policies".into(), Value::Array(policies));
    result.insert(
        "idle".into(),
        Value::Array(idle.into_iter().map(Value::Object).collect()),
    );
    result.insert("idle_completion".into(), Value::Object(completion));
    let read_text = |p: &str| text(root.read(p));
    result.insert(
        "voltage_uv".into(),
        json!(root.integer("/sys/module/pwrap/parameters/cpu_voltage_uv")),
    );
    result.insert(
        "stock_bin".into(),
        read_text("/sys/module/cpu_dvfs/parameters/bin_supported"),
    );
    result.insert("dvfs_ceiling_khz".into(), json!(ceiling));
    result.insert(
        "dvfs_fault".into(),
        read_text("/sys/module/cpu_dvfs/parameters/voltage_fault"),
    );
    result.insert(
        "spm".into(),
        read_text("/sys/devices/platform/10006000.power-controller/state"),
    );
    result.insert(
        "dormant_preflight".into(),
        read_text("/sys/devices/platform/10006000.power-controller/dormant_preflight"),
    );
    result.insert(
        "cirq".into(),
        read_text("/sys/devices/platform/10204000.interrupt-latch/state"),
    );
    result.insert("suspend_stage".into(), read_text("/run/y2/suspend-stage"));
    let mut persistent = Map::new();
    persistent.insert("current".into(), read_text("/sys/firmware/y2_pm/state"));
    persistent.insert(
        "previous_boot".into(),
        read_text("/sys/firmware/y2_pm/previous"),
    );
    persistent.insert(
        "durable".into(),
        root.json("/data/system/platform/suspend-last.json")
            .unwrap_or(Value::Null),
    );
    let mut usb = Map::new();
    for path in root.glob("/sys/bus/platform/drivers/y2-usb/*/status") {
        usb.insert(parent_name(&path), text(read_limited(&path, 8192)));
    }
    persistent.insert("usb_restore".into(), Value::Object(usb));
    persistent.insert(
        "radio_restore".into(),
        read_text("/sys/devices/platform/18070000.connectivity/status"),
    );
    // Fix02: awake SRAM proof, retention and callback trail.
    persistent.insert(
        "retention".into(),
        read_text("/sys/firmware/y2_pm/retention"),
    );
    persistent.insert("selftest".into(), read_text("/sys/firmware/y2_pm/selftest"));
    persistent.insert(
        "backstop".into(),
        read_text("/sys/firmware/y2_pm/backstop_s"),
    );
    persistent.insert(
        "device_callbacks".into(),
        text(root.read_limited("/sys/firmware/y2_pm/devices", 16384)),
    );
    persistent.insert(
        "device_callbacks_previous_boot".into(),
        text(root.read_limited("/sys/firmware/y2_pm/devices_previous", 16384)),
    );
    // Fix03: decoded RGU cause of the reset that started this boot.
    persistent.insert(
        "reset_status".into(),
        read_text("/sys/firmware/y2_pm/reset_status"),
    );
    result.insert("suspend_persistent".into(), Value::Object(persistent));
    result.insert(
        "dvfs_diagnostics".into(),
        reads_by_name(root, "/sys/module/cpu_dvfs/parameters/*", false),
    );
    result.insert(
        "voltage_ownership".into(),
        read_text("/sys/module/pwrap/parameters/cpu_voltage_state"),
    );
    result.insert(
        "slidle_clock_owners".into(),
        read_text("/sys/module/clocks/parameters/slow_blocker_names"),
    );
    result.insert(
        "pwrap_readiness".into(),
        read_text("/sys/module/pwrap/parameters/pwrap_readiness"),
    );
    result.insert(
        "slidle_attribution".into(),
        reads_by_name(root, "/sys/module/clocks/parameters/slow_*", false),
    );
    result.insert(
        "mmc_runtime_pm".into(),
        reads_by_name(root, "/sys/bus/platform/devices/*.mmc/y2_runtime_pm", true),
    );
    result.insert(
        "system_idle".into(),
        reads_by_name(root, "/sys/module/system_idle/parameters/*", false),
    );
    result.insert(
        "radio_boot_retries".into(),
        json!(root.integer("/sys/devices/platform/18070000.connectivity/boot_retries")),
    );
    result.insert(
        "clock_blockers".into(),
        reads_by_name(root, "/sys/module/clocks/parameters/*blockers", false),
    );
    result.insert(
        "idle_diagnostics".into(),
        reads_by_name(root, "/sys/module/idle/parameters/*", false),
    );
    result.insert(
        "recovery_options".into(),
        reads_by_name(root, "/sys/module/cpu_options/parameters/*", false),
    );
    let mut picked = Map::new();
    for key in [
        "ctxt",
        "intr",
        "softirq",
        "processes",
        "procs_running",
        "procs_blocked",
    ] {
        picked.insert(key.into(), totals.get(key).cloned().unwrap_or(Value::Null));
    }
    result.insert("counters".into(), Value::Object(picked));
    result.insert("wakeup_count".into(), json!(wakeup_count));
    result.insert(
        "wakeup_count_reason".into(),
        match wakeup_count {
            Some(_) => Value::Null,
            None => json!(wakeup
                .reason
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "counter_unavailable".into())),
        },
    );
    result.insert("throttling_reason".into(), Value::Null);
    result.insert(
        "workload_qos".into(),
        json!({"implemented": root.path("/dev/y2-workload").exists(), "lease_max_ms": 30000,
               "interactive_max_ms": 500,
               "leases": text(root.read("/sys/module/workload/parameters/leases"))}),
    );
    let mut timer = timer_runtime(root)?;
    timer.insert(
        "admission".into(),
        reads_by_name(root, "/sys/module/local_timer/parameters/*", false),
    );
    timer.insert(
        "clocksource".into(),
        read_text("/sys/devices/system/clocksource/clocksource0/current_clocksource"),
    );
    timer.insert(
        "broadcast_clockevent".into(),
        read_text("/sys/devices/system/clockevents/broadcast/current_device"),
    );
    timer.insert(
        "clockevents".into(),
        reads_by_name(
            root,
            "/sys/devices/system/clockevents/clockevent*/current_device",
            true,
        ),
    );
    result.insert("timer".into(), Value::Object(timer));
    Ok(result)
}

/// Each core's busy percentage between two `ticks` readings (3 decimals); null without a valid pair.
pub fn utilization(before: Option<&Value>, after: Option<&Value>) -> Value {
    let mut result = Map::new();
    let empty = Map::new();
    let before = before.and_then(Value::as_object).unwrap_or(&empty);
    for (core, new) in after.and_then(Value::as_object).unwrap_or(&empty) {
        let (Some(old), Some(new)) = (before.get(core).and_then(Value::as_array), new.as_array())
        else {
            continue;
        };
        if old.len() != new.len() || new.len() < 5 {
            continue;
        }
        let (Some(old), Some(new)): (Option<Vec<i64>>, Option<Vec<i64>>) = (
            old.iter().map(Value::as_i64).collect(),
            new.iter().map(Value::as_i64).collect(),
        ) else {
            continue;
        };
        let diff: Vec<i128> = old
            .iter()
            .zip(&new)
            .map(|(a, b)| i128::from(*b) - i128::from(*a))
            .collect();
        let total: i128 = diff.iter().sum();
        if diff.iter().min().is_some_and(|m| *m >= 0) && total > 0 {
            let busy = 100.0 * (total - diff[3] - diff[4]) as f64 / total as f64;
            result.insert(core.clone(), json!(round3(busy)));
        }
    }
    if result.is_empty() {
        Value::Null
    } else {
        Value::Object(result)
    }
}

/// `x` rounded to 3 decimals, half to even on the exact binary value (Python's `round(x, 3)`, which
/// the percentages were defined by).
pub fn round3(x: f64) -> f64 {
    let text = format!("{x:.3}");
    let candidate: f64 = text.parse().unwrap_or(x);
    // format! rounds the exact binary value half-to-even, as CPython's round() does.
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_matches_python() {
        // round(x, 3) in CPython
        assert_eq!(round3(2.0005), 2.001);
        assert_eq!(round3(13.0434782608), 13.043);
        assert_eq!(round3(0.0015), 0.002);
        assert_eq!(round3(100.0), 100.0);
        // Utilisation percentages 100 * busy / total as the snapshot computes them.
        let values: Vec<f64> = (1..4000u32)
            .map(|total| 100.0 * f64::from(total * 7 % 997) / f64::from(total))
            .collect();
        // round(x, 3) as CPython printed it (recorded from host Python 3.15).
        let ours: String = values
            .iter()
            .map(|v| y2p_core::json::float_repr(round3(*v)) + "\n")
            .collect();
        y2p_core::recorded::check(env!("CARGO_MANIFEST_DIR"), "round3.txt", ours.as_bytes());
    }
}
