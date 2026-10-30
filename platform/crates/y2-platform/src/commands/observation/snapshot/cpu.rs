//! The `cpu` section: load, frequency policies, idle states and their completion counters, the
//! timer runtime, and what the power-management drivers report.
// SPDX-License-Identifier: GPL-2.0-only
use super::idle::idle_completion;
use crate::commands::observation::util::{
    self, counters, findall_map, int, name, parent_name, read, read_limited, reads_by_name,
    split_ws, splitlines, Readings,
};
use crate::{regex, reply::Failure};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};
use y2p_core::{command, Root};

/// Whether every CPU runs high-resolution timers and tickless idle, from /proc/timer_list.
pub fn timer_runtime(root: &Root) -> Result<Timer, Failure> {
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
    let mut cpus: Vec<TimerCpu> = Vec::new();
    for block in regex!(r"(?m)^cpu:\s*").split(&text).skip(1) {
        let lines = splitlines(block);
        // A `cpu:` with nothing after it (the text cut short): no line to number the CPU by.
        let Some(first) = lines.first() else {
            return Err(Failure::value("invalid_timer_list"));
        };
        if !util::isdigit(first) {
            continue;
        }
        let mut core = TimerCpu {
            cpu: util::int_or_raise(first)?,
            fields: BTreeMap::new(),
        };
        let fields = findall_map(
            regex!(r"(?m)^\s*\.(hres_active|nohz|tick_stopped|nr_hangs)\s*:\s*([0-9]+)\s*$"),
            block,
        );
        for (key, value) in fields {
            core.fields.insert(key, util::int_or_raise(&value)?);
        }
        cpus.push(core);
    }
    // True or false only when every CPU reports the field.
    let observed = |key: &str| -> Option<bool> {
        let values: Option<Vec<i64>> = cpus.iter().map(|c| c.fields.get(key).copied()).collect();
        values.filter(|values| !values.is_empty()).map(|values| {
            values
                .iter()
                .all(|v| if key == "nohz" { *v > 0 } else { *v == 1 })
        })
    };
    Ok(Timer {
        highres_active: observed("hres_active"),
        no_hz_active: observed("nohz"),
        source: "/proc/timer_list",
        reason: cpus.is_empty().then_some("runtime_timer_state_unavailable"),
        per_cpu: cpus,
        admission: None,
        clocksource: None,
        broadcast_clockevent: None,
        clockevents: None,
    })
}

/// The timer runtime: high-resolution timers and tickless idle on every CPU, then the clock sources
/// (filled in by the cpu section).
#[derive(Serialize)]
pub struct Timer {
    pub highres_active: Option<bool>,
    pub no_hz_active: Option<bool>,
    source: &'static str,
    reason: Option<&'static str>,
    per_cpu: Vec<TimerCpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    admission: Option<Readings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clocksource: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    broadcast_clockevent: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clockevents: Option<Readings>,
}

/// One CPU's timer fields from /proc/timer_list.
#[derive(Serialize)]
struct TimerCpu {
    cpu: i64,
    #[serde(flatten)]
    fields: BTreeMap<String, i64>,
}

/// CPU tick counters by line (`cpu`, `cpu0`, …): the first eight fields of /proc/stat.
pub type Ticks = BTreeMap<String, Vec<Option<i64>>>;

/// The `cpu` section (in this key order; `utilization_percent` is filled in by the caller from
/// two readings).
#[derive(Serialize)]
pub struct Cpu {
    online: Option<String>,
    load_average: Option<String>,
    pub ticks: Option<Ticks>,
    ticks_unit: &'static str,
    pub utilization_percent: Option<BTreeMap<String, f64>>,
    policies: Vec<Policy>,
    idle: Vec<IdleState>,
    idle_completion: super::idle::IdleCompletion,
    voltage_uv: Option<i64>,
    stock_bin: Option<String>,
    dvfs_ceiling_khz: Option<i64>,
    dvfs_fault: Option<String>,
    spm: Option<String>,
    dormant_preflight: Option<String>,
    cirq: Option<String>,
    suspend_stage: Option<String>,
    suspend_persistent: SuspendPersistent,
    dvfs_diagnostics: Readings,
    voltage_ownership: Option<String>,
    slidle_clock_owners: Option<String>,
    pwrap_readiness: Option<String>,
    slidle_attribution: Readings,
    mmc_runtime_pm: Readings,
    system_idle: Readings,
    radio_boot_retries: Option<i64>,
    clock_blockers: Readings,
    idle_diagnostics: Readings,
    recovery_options: Readings,
    counters: Counters,
    wakeup_count: Option<i64>,
    wakeup_count_reason: Option<String>,
    throttling_reason: Option<String>,
    workload_qos: WorkloadQos,
    pub timer: Timer,
}

/// One cpufreq policy: its attributes (null where a file is missing), then what the snapshot derives.
#[derive(Serialize)]
struct Policy {
    affected_cpus: Option<String>,
    scaling_governor: Option<String>,
    scaling_cur_freq: Option<String>,
    scaling_min_freq: Option<String>,
    scaling_max_freq: Option<String>,
    scaling_available_frequencies: Option<String>,
    cpuinfo_min_freq: Option<String>,
    cpuinfo_max_freq: Option<String>,
    cpuinfo_cur_freq: Option<String>,
    scaling_driver: Option<String>,
    /// The available frequencies at or below the DVFS ceiling and the policy's maximum.
    admitted_opp_khz: Vec<i64>,
    name: String,
    time_in_state: Option<Map<String, Value>>,
    time_in_state_unit: &'static str,
    frequency_unit: &'static str,
}

impl Policy {
    fn read(path: &Path, ceiling: Option<i64>) -> Self {
        let attribute = |field: &str| read(&path.join(field));
        let scaling_max_freq = attribute("scaling_max_freq");
        let scaling_available_frequencies = attribute("scaling_available_frequencies");
        let admitted = ceiling
            .zip(scaling_max_freq.as_deref().and_then(int))
            .map(|(c, m)| c.min(m));
        let admitted_opp_khz =
            split_ws(scaling_available_frequencies.as_deref().unwrap_or_default())
                .into_iter()
                .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
                .filter_map(int)
                .filter(|v| admitted.is_some_and(|a| *v <= a))
                .collect();
        let states = counters(read(&path.join("stats/time_in_state")).as_deref());
        Policy {
            affected_cpus: attribute("affected_cpus"),
            scaling_governor: attribute("scaling_governor"),
            scaling_cur_freq: attribute("scaling_cur_freq"),
            scaling_min_freq: attribute("scaling_min_freq"),
            scaling_max_freq,
            scaling_available_frequencies,
            cpuinfo_min_freq: attribute("cpuinfo_min_freq"),
            cpuinfo_max_freq: attribute("cpuinfo_max_freq"),
            cpuinfo_cur_freq: attribute("cpuinfo_cur_freq"),
            scaling_driver: attribute("scaling_driver"),
            admitted_opp_khz,
            name: name(path),
            time_in_state: (!states.is_empty()).then_some(states),
            time_in_state_unit: "USER_HZ_ticks",
            frequency_unit: "kHz",
        }
    }
}

/// One cpuidle state of one CPU (`cpuN/cpuidle/stateM`); its counters null where unreadable.
#[derive(Serialize, Clone)]
pub(super) struct IdleState {
    pub cpu: String,
    pub state: String,
    pub name: Option<String>,
    pub time_us: Option<i64>,
    pub usage: Option<i64>,
    pub exit_latency_us: Option<i64>,
    pub target_residency_us: Option<i64>,
    pub rejected: Option<i64>,
    pub disabled: Option<i64>,
}

impl IdleState {
    fn read(path: &Path) -> Self {
        let value = |f: &str| read(&path.join(f)).as_deref().and_then(int);
        IdleState {
            cpu: path.parent().map(parent_name).unwrap_or_default(),
            state: name(path),
            name: read(&path.join("name")),
            time_us: value("time"),
            usage: value("usage"),
            exit_latency_us: value("latency"),
            target_residency_us: value("residency"),
            rejected: value("rejected"),
            disabled: value("disable"),
        }
    }
}

/// The suspend trail that survives a reset (Fix02/Fix03) and the drivers' restore state.
#[derive(Serialize)]
struct SuspendPersistent {
    current: Option<String>,
    previous_boot: Option<String>,
    durable: Value,
    usb_restore: Readings,
    radio_restore: Option<String>,
    /// Fix02: awake SRAM proof, retention and callback trail.
    retention: Option<String>,
    selftest: Option<String>,
    backstop: Option<String>,
    device_callbacks: Option<String>,
    device_callbacks_previous_boot: Option<String>,
    /// Fix03: decoded RGU cause of the reset that started this boot.
    reset_status: Option<String>,
}

/// /proc/stat's totals the snapshot shows (null where a line is missing).
#[derive(Serialize)]
struct Counters {
    ctxt: Value,
    intr: Value,
    softirq: Value,
    processes: Value,
    procs_running: Value,
    procs_blocked: Value,
}

#[derive(Serialize)]
struct WorkloadQos {
    implemented: bool,
    lease_max_ms: u32,
    interactive_max_ms: u32,
    leases: Option<String>,
}

/// The `cpu` section.
pub fn cpu(root: &Root) -> Result<Cpu, Failure> {
    let stat = root.read("/proc/stat");
    let mut ticks = Ticks::new();
    for line in splitlines(stat.as_deref().unwrap_or_default()) {
        let fields = split_ws(line);
        if fields
            .first()
            .is_some_and(|f| regex!(r"\Acpu[0-9]*\z").is_match(f))
        {
            let values = fields.iter().skip(1).take(8).map(|v| int(v)).collect();
            ticks.insert(fields[0].to_owned(), values);
        }
    }
    let ceiling = root.integer("/sys/module/cpu_dvfs/parameters/qualification_max_khz");
    let policies = root
        .glob("/sys/devices/system/cpu/cpufreq/policy*")
        .iter()
        .map(|path| Policy::read(path, ceiling))
        .collect();
    let idle: Vec<IdleState> = root
        .glob("/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*")
        .iter()
        .map(|path| IdleState::read(path))
        .collect();
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
    let read_text = |p: &str| root.read(p);
    let mut usb_restore = Readings::new();
    for path in root.glob("/sys/bus/platform/drivers/y2-usb/*/status") {
        usb_restore.insert(parent_name(&path), read_limited(&path, 8192));
    }
    let suspend_persistent = SuspendPersistent {
        current: read_text("/sys/firmware/y2_pm/state"),
        previous_boot: read_text("/sys/firmware/y2_pm/previous"),
        durable: root
            .json("/data/system/platform/suspend-last.json")
            .unwrap_or(Value::Null),
        usb_restore,
        radio_restore: read_text("/sys/devices/platform/18070000.connectivity/status"),
        retention: read_text("/sys/firmware/y2_pm/retention"),
        selftest: read_text("/sys/firmware/y2_pm/selftest"),
        backstop: read_text("/sys/firmware/y2_pm/backstop_s"),
        device_callbacks: root.read_limited("/sys/firmware/y2_pm/devices", 16384),
        device_callbacks_previous_boot: root
            .read_limited("/sys/firmware/y2_pm/devices_previous", 16384),
        reset_status: read_text("/sys/firmware/y2_pm/reset_status"),
    };
    let total = |key: &str| totals.get(key).cloned().unwrap_or(Value::Null);
    let counters = Counters {
        ctxt: total("ctxt"),
        intr: total("intr"),
        softirq: total("softirq"),
        processes: total("processes"),
        procs_running: total("procs_running"),
        procs_blocked: total("procs_blocked"),
    };
    let mut timer = timer_runtime(root)?;
    timer.admission = Some(reads_by_name(
        root,
        "/sys/module/local_timer/parameters/*",
        false,
    ));
    timer.clocksource = Some(read_text(
        "/sys/devices/system/clocksource/clocksource0/current_clocksource",
    ));
    timer.broadcast_clockevent = Some(read_text(
        "/sys/devices/system/clockevents/broadcast/current_device",
    ));
    timer.clockevents = Some(reads_by_name(
        root,
        "/sys/devices/system/clockevents/clockevent*/current_device",
        true,
    ));
    let section = Cpu {
        online: read_text("/sys/devices/system/cpu/online"),
        load_average: read_text("/proc/loadavg"),
        ticks: (!ticks.is_empty()).then_some(ticks),
        ticks_unit: "USER_HZ_ticks",
        utilization_percent: None,
        policies,
        idle,
        idle_completion: completion,
        voltage_uv: root.integer("/sys/module/pwrap/parameters/cpu_voltage_uv"),
        stock_bin: read_text("/sys/module/cpu_dvfs/parameters/bin_supported"),
        dvfs_ceiling_khz: ceiling,
        dvfs_fault: read_text("/sys/module/cpu_dvfs/parameters/voltage_fault"),
        spm: read_text("/sys/devices/platform/10006000.power-controller/state"),
        dormant_preflight: read_text(
            "/sys/devices/platform/10006000.power-controller/dormant_preflight",
        ),
        cirq: read_text("/sys/devices/platform/10204000.interrupt-latch/state"),
        suspend_stage: read_text("/run/y2/suspend-stage"),
        suspend_persistent,
        dvfs_diagnostics: reads_by_name(root, "/sys/module/cpu_dvfs/parameters/*", false),
        voltage_ownership: read_text("/sys/module/pwrap/parameters/cpu_voltage_state"),
        slidle_clock_owners: read_text("/sys/module/clocks/parameters/slow_blocker_names"),
        pwrap_readiness: read_text("/sys/module/pwrap/parameters/pwrap_readiness"),
        slidle_attribution: reads_by_name(root, "/sys/module/clocks/parameters/slow_*", false),
        mmc_runtime_pm: reads_by_name(root, "/sys/bus/platform/devices/*.mmc/y2_runtime_pm", true),
        system_idle: reads_by_name(root, "/sys/module/system_idle/parameters/*", false),
        radio_boot_retries: root
            .integer("/sys/devices/platform/18070000.connectivity/boot_retries"),
        clock_blockers: reads_by_name(root, "/sys/module/clocks/parameters/*blockers", false),
        idle_diagnostics: reads_by_name(root, "/sys/module/idle/parameters/*", false),
        recovery_options: reads_by_name(root, "/sys/module/cpu_options/parameters/*", false),
        counters,
        wakeup_count,
        wakeup_count_reason: match wakeup_count {
            Some(_) => None,
            None => Some(
                wakeup
                    .reason
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "counter_unavailable".into()),
            ),
        },
        throttling_reason: None,
        workload_qos: WorkloadQos {
            implemented: root.path("/dev/y2-workload").exists(),
            lease_max_ms: 30000,
            interactive_max_ms: 500,
            leases: read_text("/sys/module/workload/parameters/leases"),
        },
        timer,
    };
    Ok(section)
}

/// Each core's busy percentage between two `ticks` readings (3 decimals); null without a valid pair.
/// Busy percent per CPU line between two readings of the same boot (none where a line is missing,
/// unread or went backwards).
pub fn utilization(before: Option<&Ticks>, after: Option<&Ticks>) -> Option<BTreeMap<String, f64>> {
    let (before, after) = (before?, after?);
    let mut result = BTreeMap::new();
    for (core, new) in after {
        let Some(old) = before.get(core) else {
            continue;
        };
        if old.len() != new.len() || new.len() < 5 {
            continue;
        }
        let (Some(old), Some(new)): (Option<Vec<i64>>, Option<Vec<i64>>) =
            (old.iter().copied().collect(), new.iter().copied().collect())
        else {
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
            result.insert(core.clone(), round3(busy));
        }
    }
    (!result.is_empty()).then_some(result)
}

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
