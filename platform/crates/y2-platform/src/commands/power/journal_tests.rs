//! The power journal tests of tests/test_power_journal.py, on the port: incident retention has a low
//! write rate and cannot defeat battery safety.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use serde_json::{json, Value};

/// The Python `setUp`: a verified data mount and a boot id.
fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/data/.y2data-schema", "1")
        .put("/run/y2-data-device", "/dev/data")
        .put(
            "/proc/self/mountinfo",
            "5 1 179:7 / /data rw - ext4 /dev/data rw",
        )
        .put("/proc/sys/kernel/random/boot_id", "one");
    fixture
}

fn rows_of(fixture: &Fixture, name: &str) -> Vec<Value> {
    fixture
        .read(name)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn rows(fixture: &Fixture) -> Vec<Value> {
    rows_of(fixture, PATH)
}

fn battery(value: Value) -> Object {
    match value {
        Value::Object(o) => o,
        other => panic!("{other:?}"),
    }
}

const PREVIOUS: &str = "/data/system/platform/power-journal.previous.jsonl";
const RECEIPT: &str = "/data/system/platform/suspend-last.json";

#[test]
fn steady_state_only_writes_every_five_minutes() {
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    for now in 0..901 {
        let sample = battery(json!({"voltage_uv": 3900000 - now, "status": "Discharging"}));
        journal.observe_at(&fixture.root, "Normal", &sample, None, None, now as f64);
    }
    let rows = rows(&fixture);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3]["monotonic_s"], 900.0);
}

#[test]
fn state_storm_is_bounded_but_shutdown_is_immediate() {
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    for now in 0..60 {
        let status = if now % 2 == 1 {
            "Charging"
        } else {
            "Discharging"
        };
        let sample = battery(json!({ "status": status }));
        journal.observe_at(&fixture.root, "Normal", &sample, None, None, now as f64);
    }
    assert_eq!(rows(&fixture).len(), 1);
    let shutdown: crate::commands::power::coordinator::Intent = serde_json::from_value(json!({
        "id": "x", "state": "ShutdownPending", "action": "poweroff", "reason": "low_battery",
        "deadline_monotonic_ns": 1, "client": null, "acknowledgement": null}))
    .unwrap();
    journal.observe_at(
        &fixture.root,
        "ShutdownPending",
        &battery(json!({"voltage_uv": 3400000})),
        Some(&shutdown),
        None,
        60.,
    );
    assert_eq!(
        rows(&fixture).last().unwrap()["shutdown"]["reason"],
        "low_battery"
    );
    journal.observe_at(
        &fixture.root,
        "Suspend",
        &Object::new(),
        None,
        Some("suspend_kernel_suspend"),
        61.,
    );
    assert_eq!(
        rows(&fixture).last().unwrap()["event"],
        "suspend_kernel_suspend"
    );
}

/// Python patched `LIMIT` to 200; the journal's `append` is `append_bounded(PATH, row, LIMIT)`, so
/// this appends its rows with that limit directly.
#[test]
fn rotation_keeps_complete_rows_and_private_permissions() {
    let fixture = fixture();
    for i in 0..10 {
        let row = Object::new()
            .with("i", i as i64)
            .with("detail", "x".repeat(80));
        let encoded = dumps(&row).unwrap() + "\n";
        append_bounded(&fixture.root, PATH, encoded.as_bytes(), 200, false).unwrap();
    }
    assert_eq!(rows(&fixture).last().unwrap()["i"], 9);
    assert_eq!(rows_of(&fixture, PREVIOUS).last().unwrap()["i"], 7);
    assert_eq!(fixture.mode(PATH) & 0o777, 0o600);
}

#[test]
fn missing_mount_and_write_failure_do_not_raise() {
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    fixture.put("/run/y2-data-device", "/dev/wrong");
    assert!(!journal.observe_at(
        &fixture.root,
        "ShutdownPending",
        &Object::new(),
        None,
        None,
        0.
    ));
    assert!(!fixture.exists(PATH));
    fixture.put("/run/y2-data-device", "/dev/data");
    // Python made `append` raise OSError('full'); here the journal file cannot be opened.
    fixture.mkdir(PATH);
    assert!(!journal.observe_at(
        &fixture.root,
        "ShutdownPending",
        &Object::new(),
        None,
        Some("shutdown_execute"),
        1.
    ));
    assert_eq!(journal.failures, 1);
}

#[test]
fn competing_writer_never_blocks_safety() {
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    journal.observe_at(&fixture.root, "Normal", &Object::new(), None, None, 0.);
    let lock_path = fixture.path("/data/system/platform/.power-journal.lock");
    let lock = y2p_core::lock::try_exclusive(&lock_path)
        .unwrap()
        .expect("lock free");
    assert!(!journal.observe_at(
        &fixture.root,
        "ShutdownPending",
        &Object::new(),
        None,
        None,
        1.
    ));
    drop(lock);
    assert_eq!(journal.failures, 1);
    assert_eq!(rows(&fixture).len(), 1);
}

#[test]
fn completed_sleep_endpoints_survive_a_later_refusal() {
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    let empty = Object::new();
    journal.observe_at(&fixture.root, "Normal", &empty, None, None, 0.);
    let completed = json!({"mode": "product", "stage": "complete_result_0", "cycles": 4,
        "suspended_seconds": 1840.5, "maintenance_wakes": 3,
        "first_entry": {"boottime_s": 100.0, "voltage_uv": 4000000},
        "resume": {"boottime_s": 1950.0, "voltage_uv": 3980000}});
    fixture.put_json(RECEIPT, &completed);
    journal.observe_at(
        &fixture.root,
        "Suspend",
        &empty,
        None,
        Some("suspend_complete_result_0"),
        1.,
    );
    fixture.put_json(RECEIPT, &json!({"stage": "refused_charger_active"}));
    journal.observe_at(
        &fixture.root,
        "Suspend",
        &empty,
        None,
        Some("suspend_refused_charger_active"),
        2.,
    );
    let rows = rows(&fixture);
    let mut expected = completed.clone();
    expected["error"] = Value::Null;
    expected["reason"] = Value::Null;
    assert_eq!(rows[rows.len() - 2]["suspend"], expected);
    assert!(rows[rows.len() - 1]["suspend"]["first_entry"].is_null());
    // POWER-FINAL-18: a refusal after the radio quiesce keeps its reason.
    fixture.put_json(
        RECEIPT,
        &json!({"stage": "product_refused",
                "product": {"state": "refused", "reason": "wake_event_in_progress"}}),
    );
    journal.observe_at(
        &fixture.root,
        "Suspend",
        &empty,
        None,
        Some("suspend_product_refused"),
        3.,
    );
    assert_eq!(
        self::rows(&fixture).last().unwrap()["suspend"]["reason"],
        "wake_event_in_progress"
    );
}

#[test]
fn overnight_retention_at_maximum_row_size_and_real_sleep_receipt() {
    // An eight-hour night plus eight hours before/after must survive even
    // with the largest permitted row; rotation must not depend on SOC.
    // POWER-FINAL-16: 8 MiB files keep a full day of maximum-size rows.
    let fixture = fixture();
    let mut journal = PowerJournal::new();
    let detail = "x".repeat(16200);
    for index in 0..1153 {
        let row = Object::new()
            .with("sample", index as i64)
            .with("detail", detail.as_str());
        append(&fixture.root, &row, false).expect("append");
    }
    let retained: Vec<i64> = rows_of(&fixture, PREVIOUS)
        .iter()
        .chain(rows(&fixture).iter())
        .map(|row| row["sample"].as_i64().unwrap())
        .collect();
    assert!((865..1153).all(|i| retained.contains(&i)));
    let size = |name: &str| std::fs::metadata(fixture.path(name)).unwrap().len();
    assert!(size(PREVIOUS) + size(PATH) <= 16777216);
    fixture.put_json(
        RECEIPT,
        &json!({"product": {"state": "sleeping", "request_boot_id": "old"}}),
    );
    journal.observe_at(&fixture.root, "Normal", &Object::new(), None, None, 0.);
    let rows = rows(&fixture);
    let row = rows.last().unwrap();
    assert_eq!(row["sleep"]["state"], "restore_failed");
    assert_eq!(row["sleep"]["reason"], "boot_changed_before_restoration");
    assert!(!row["boottime_s"].is_null());
}

#[test]
fn idle_counters_are_cumulative_and_collected_only_for_due_rows() {
    let fixture = fixture();
    for (name, value) in [
        ("/sys/devices/system/cpu/online", "0"),
        (
            "/sys/devices/system/cpu/cpu0/cpuidle/state2/name",
            "DORMANT",
        ),
        ("/sys/devices/system/cpu/cpu0/cpuidle/state2/usage", "122"),
        (
            "/sys/devices/system/cpu/cpu0/cpuidle/state2/time",
            "4500000",
        ),
        ("/sys/devices/system/cpu/cpu0/cpuidle/state2/disable", "0"),
        (
            "/proc/stat",
            "cpu 1 2 3 4\nintr 1500 100 1400\nctxt 2600\nprocesses 12\n",
        ),
        (
            "/sys/bus/platform/devices/13010000.gpu/power/runtime_status",
            "suspended",
        ),
    ] {
        fixture.put(name, value);
    }
    let mut journal = PowerJournal::new();
    let sample = battery(json!({"status": "Discharging"}));
    let usage = "/sys/devices/system/cpu/cpu0/cpuidle/state2/usage";
    for now in 0..601 {
        if now == 450 {
            fixture.put(usage, "123");
        }
        journal.observe_at(&fixture.root, "Normal", &sample, None, None, now as f64);
    }
    // Three due rows in 601 samples, each with the counters as they were when it was written.
    let rows = rows(&fixture);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["idle_counters"].is_object()));
    let usages: Vec<&Value> = rows
        .iter()
        .map(|row| &row["idle_counters"]["cpuidle"]["0"]["DORMANT"]["usage"])
        .collect();
    assert_eq!(usages, [&json!(122), &json!(122), &json!(123)]);
    let row = &rows[2]["idle_counters"];
    assert_eq!(row["cpuidle"]["0"]["DORMANT"]["usage"], 123);
    assert!(row["cpuidle"]["0"]["DORMANT"]["rejected"].is_null());
    assert_eq!(
        row["scheduler"],
        json!({"intr": 1500, "ctxt": 2600, "processes": 12})
    );
    assert_eq!(row["runtime_pm"]["13010000.gpu"], "suspended");
    assert!(row["collector_cpu_seconds"].as_f64().unwrap() >= 0.0);
}
