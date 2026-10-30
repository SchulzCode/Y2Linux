//! The `y2-power-audit` tests of tests/test_power_audit.py, on the port: counter discontinuities must
//! not become fictitious power measurements.
// SPDX-License-Identifier: GPL-2.0-only
use super::delta::delta;
use super::sample::{Sample, Thread};
use serde_json::{json, Value};
use y2p_core::json::dumps;

const IDLE: &str = "/sys/devices/system/cpu/cpu0/cpuidle/state2";
const OPP: &str = "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state";

fn thread(name: &str, utime: i64) -> Thread {
    Thread {
        name: name.into(),
        state: String::new(),
        utime,
        stime: 0,
        switches: Vec::new(),
        wchan: None,
    }
}

fn sample(boot_id: &str, thread_key: &str, t: Thread, files: &[(&str, &str)]) -> Sample {
    Sample {
        boot_id: Some(boot_id.into()),
        threads: vec![(thread_key.into(), t)],
        files: files
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Some((*v).to_owned())))
            .collect(),
        monotonic: 0.,
        collection: 0.,
        cpu: 0.,
    }
}

/// The two endpoint samples of the Python test.
fn samples() -> (Sample, Sample) {
    let a = sample(
        "one",
        "4:4:2",
        thread("old", 0),
        &[
            ("/proc/interrupts", "CPU0 CPU1\n12: 100 50 timer\n"),
            ("/proc/stat", "ctxt 100\n"),
            (&format!("{IDLE}/time"), "1000000"),
            (&format!("{IDLE}/usage"), "4"),
            (&format!("{IDLE}/name"), "DORMANT"),
            (OPP, "598000 50\n1300000 50\n"),
        ],
    );
    let b = sample(
        "one",
        "4:4:8",
        thread("new", 500),
        &[
            ("/proc/interrupts", "CPU0\n12: 110 timer\n"),
            ("/proc/stat", "ctxt 120\n"),
            (&format!("{IDLE}/time"), "6000000"),
            (&format!("{IDLE}/usage"), "9"),
            (&format!("{IDLE}/name"), "DORMANT"),
            (OPP, "598000 140\n1300000 60\n"),
        ],
    );
    (a, b)
}

/// `b['files'][key] = value` (a dict assignment: replace or append).
fn set(sample: &mut Sample, key: &str, value: &str) {
    match sample.files.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = Some(value.into()),
        None => sample.files.push((key.into(), Some(value.into()))),
    }
}

fn delta_json(a: &Sample, b: &Sample, duration: f64) -> Value {
    let node = delta(a, b, duration).expect("delta");
    serde_json::from_str(&dumps(&node).unwrap()).unwrap()
}

fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

#[test]
fn real_deltas_restart_and_offline_irq_counters() {
    let (a, b) = samples();
    let result = delta_json(&a, &b, 10.);
    assert_eq!(number(&result["cpuidle"][0]["percent_wall_time"]), 50.);
    assert_eq!(result["cpuidle"][0]["entries"], 5);
    assert_eq!(number(&result["cpu0_idle_exits_per_second"]), 0.5);
    assert_eq!(result["top_cpu_threads"], json!([]));
    let irq = &result["irqs"][0];
    assert!(irq["per_second"].is_null());
    assert_eq!(irq["counter_discontinuity"], true);
    assert_eq!(irq["cpu_columns_changed"], true);
    assert_eq!(irq["common_cpu_deltas"], json!({"CPU0": 10}));
    assert_eq!(number(&irq["common_cpu_lower_bound_per_second"]), 1.);
    assert_eq!(
        number(&result["frequency_residency"][0]["percent_recorded_residency"]),
        90.
    );
    assert_eq!(number(&result["scheduler"]["ctxt"]["per_second"]), 2.);
    assert!(result["wakeups_per_second"].is_null());
}

#[test]
fn positive_sum_difference_still_invalid_across_cpu_offline() {
    let (a, mut b) = samples();
    set(&mut b, "/proc/interrupts", "CPU0\n12: 200 timer\n");
    let row = &delta_json(&a, &b, 10.)["irqs"][0];
    assert!(row["delta"].is_null());
    assert_eq!(number(&row["common_cpu_lower_bound_per_second"]), 10.);
}

#[test]
fn matching_cpu_columns_and_individual_counter_reset() {
    let (a, mut b) = samples();
    set(&mut b, "/proc/interrupts", "CPU0 CPU1\n12: 120 60 timer\n");
    let row = &delta_json(&a, &b, 10.)["irqs"][0];
    assert_eq!(row["delta"], 30);
    assert_eq!(number(&row["per_second"]), 3.);
    set(&mut b, "/proc/interrupts", "CPU0 CPU1\n12: 200 40 timer\n");
    let row = &delta_json(&a, &b, 10.)["irqs"][0];
    assert!(row["delta"].is_null());
    assert!(row["common_cpu_lower_bound_per_second"].is_null());
}

#[test]
fn cross_boot_refused() {
    let (a, mut b) = samples();
    b.boot_id = Some("two".into());
    let Err(error) = delta(&a, &b, 10.) else {
        panic!("refused")
    };
    assert!(error.is_value(), "ValueError");
}

#[test]
fn idle_interval_charged_after_window_does_not_claim_over_100_percent() {
    let (a, mut b) = samples();
    set(&mut b, &format!("{IDLE}/time"), "14700000");
    let result = delta_json(&a, &b, 10.);
    let row = &result["cpuidle"][0];
    assert!(row["percent_wall_time"].is_null());
    assert!(row["time_us"].is_null());
    assert_eq!(row["raw_time_delta_us"], 13700000);
    assert_eq!(row["entries"], 5);
    assert_eq!(number(&result["cpu0_idle_exits_per_second"]), 0.5);
    assert_eq!(
        row["residency_invalid_reason"],
        "idle_accounting_crosses_counter_window"
    );
}

#[test]
fn sum_of_states_cannot_exceed_one_cpu_window() {
    let (mut a, mut b) = samples();
    let base = "/sys/devices/system/cpu/cpu0/cpuidle/state1";
    for (key, old, new) in [
        ("time", "0", "6000000"),
        ("usage", "0", "2"),
        ("name", "WFI", "WFI"),
    ] {
        set(&mut a, &format!("{base}/{key}"), old);
        set(&mut b, &format!("{base}/{key}"), new);
    }
    let result = delta_json(&a, &b, 10.);
    let rows = result["cpuidle"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["percent_wall_time"].is_null()));
}

#[test]
fn hotplug_idle_reset_invalidates_entries_and_residency_together() {
    for reset in ["time", "usage"] {
        let (a, mut b) = samples();
        set(&mut b, &format!("{IDLE}/{reset}"), "0");
        let result = delta_json(&a, &b, 10.);
        let row = &result["cpuidle"][0];
        assert_eq!(row["counter_discontinuity"], true, "{reset}");
        assert!(row["time_us"].is_null());
        assert!(row["entries"].is_null());
        assert!(row["percent_wall_time"].is_null());
        assert!(result["cpu0_idle_exits_per_second"].is_null());
    }
}
