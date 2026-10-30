//! Real boot-policy writes against isolated sysfs fixtures and faults (were
//! tests/test_cpu_idle_boot_policy.py). Python replaced `_write` to inject one-off faults; here a fault
//! is what the files themselves do (a read-only control, one that reads back empty).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};

fn qualified() -> Fixture {
    let fixture = Fixture::new();
    let mut preflight: Vec<String> = FOUNDATIONS
        .iter()
        .map(|n| format!("prerequisite_{n}=1"))
        .collect();
    preflight.push(
        "prerequisite_topology=0\nprerequisite_frequency=0\nprerequisite_clocks=0\nunmet=topology,frequency,clocks,"
            .into(),
    );
    for (name, value) in [
        (
            "/proc/sys/kernel/random/boot_id",
            "qualified-boot".to_owned(),
        ),
        ("/proc/sys/kernel/osrelease", "qualified-kernel".into()),
        ("/proc/sys/kernel/tainted", "0".into()),
        (
            "/etc/y2linux/versions.json",
            json!({"kernel_version": "qualified-kernel"}).to_string(),
        ),
        (
            "/sys/devices/system/cpu/cpuidle/current_driver",
            "mt6582-idle".into(),
        ),
        (&format!("{STATE}/name"), "DORMANT".into()),
        (&disable_state(), "1".into()),
        (BUDGET, "0".into()),
        (PREFLIGHT, preflight.join("\n")),
        (
            "/sys/devices/system/cpu/cpu0/cpuidle/state0/disable",
            "0".into(),
        ),
        (
            "/sys/devices/system/cpu/cpu0/cpuidle/state1/disable",
            "0".into(),
        ),
        (
            "/sys/devices/system/cpu/cpu1/cpuidle/state2/disable",
            "1".into(),
        ),
    ] {
        put(&fixture, name, &value);
    }
    fixture
}

fn put(fixture: &Fixture, name: &str, value: &str) {
    fixture.put(name, &format!("{value}\n"));
}

fn read(fixture: &Fixture, name: &str) -> String {
    fixture.root.read(name).unwrap()
}

fn apply_json(fixture: &Fixture, action: &str) -> Value {
    serde_json::from_str(&apply(&fixture.root, action).ok().unwrap().stdout).unwrap()
}

fn state(fixture: &Fixture) -> Value {
    apply_json(fixture, "start")["state"].clone()
}

fn reasons(result: &Value) -> Vec<String> {
    serde_json::from_value(result["reasons"].clone()).unwrap()
}

fn unlock(fixture: &Fixture) {
    fixture.remove(LOCK);
}

fn assert_quarantined(fixture: &Fixture) {
    assert_eq!(read(fixture, BUDGET), "0");
    assert_eq!(read(fixture, &disable_state()), "1");
}

/// Every file below `directory` and its content.
fn files(directory: &Path, found: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, found);
        } else {
            let text = fs::read_to_string(&path).unwrap_or_default();
            found.insert(path.to_string_lossy().into_owned(), text);
        }
    }
}

#[test]
fn enables_only_cpu0_and_leaves_transient_entry_guards_to_kernel() {
    let fixture = qualified();
    let mut before = BTreeMap::new();
    files(&fixture.path("/sys"), &mut before);
    let result = apply_json(&fixture, "start");
    assert_eq!(result["state"], "Enabled");
    // The only files written: the budget and CPU0's dormant state (in path order here).
    let mut after = BTreeMap::new();
    files(&fixture.path("/sys"), &mut after);
    let changed: Vec<&String> = after
        .iter()
        .filter(|(path, text)| before.get(*path) != Some(text))
        .map(|(path, _)| path)
        .collect();
    let host = |name: &str| fixture.path(name).to_string_lossy().into_owned();
    assert_eq!(changed, [&host(&disable_state()), &host(BUDGET)]);
    assert_eq!(read(&fixture, BUDGET), "-1");
    assert_eq!(read(&fixture, &disable_state()), "0");
    for state in ["state0", "state1"] {
        assert_eq!(
            read(
                &fixture,
                &format!("/sys/devices/system/cpu/cpu0/cpuidle/{state}/disable")
            ),
            "0"
        );
    }
    assert_eq!(
        read(
            &fixture,
            "/sys/devices/system/cpu/cpu1/cpuidle/state2/disable"
        ),
        "1"
    );
    assert_eq!(fixture.json(RECORD)["boot_id"], "qualified-boot");
}

#[test]
fn kernel_warn_alone_does_not_skip_deep_idle() {
    let fixture = qualified();
    put(&fixture, "/proc/sys/kernel/tainted", "512");
    assert_eq!(state(&fixture), "Enabled");
}

#[test]
fn each_missing_or_failed_foundation_keeps_safe_fallback() {
    let fixture = qualified();
    for name in FOUNDATIONS {
        unlock(&fixture);
        let lines: Vec<String> = FOUNDATIONS
            .iter()
            .map(|n| format!("prerequisite_{n}={}", if *n == name { 0 } else { 1 }))
            .collect();
        put(&fixture, PREFLIGHT, &lines.join("\n"));
        let result = apply_json(&fixture, "start");
        assert!(
            reasons(&result).contains(&format!("foundation:{name}")),
            "{name}"
        );
        assert_quarantined(&fixture);
    }
}

#[test]
fn explicit_deep_idle_off_missing_state_and_unknown_identity_stay_off() {
    let fixture = qualified();
    for (name, value) in [
        (format!("{STATE}/name"), "unavailable"),
        ("/proc/sys/kernel/tainted".into(), "1"),
        ("/proc/sys/kernel/osrelease".into(), "wrong-kernel"),
        (
            "/sys/devices/system/cpu/cpuidle/current_driver".into(),
            "other",
        ),
    ] {
        let old = read(&fixture, &name);
        put(&fixture, &name, value);
        unlock(&fixture);
        assert_eq!(state(&fixture), "Skipped", "{name}");
        assert_quarantined(&fixture);
        put(&fixture, &name, &old);
    }
}

#[test]
fn later_manual_disable_is_not_reenabled_by_repeated_start() {
    let fixture = qualified();
    assert_eq!(state(&fixture), "Enabled");
    put(&fixture, BUDGET, "0");
    put(&fixture, &disable_state(), "1");
    let result = apply_json(&fixture, "start");
    assert!(reasons(&result).contains(&"already_applied_this_boot".to_owned()));
    assert_quarantined(&fixture);
}

#[test]
fn existing_bounded_owner_trial_is_not_replaced() {
    let fixture = qualified();
    put(&fixture, BUDGET, "1");
    let result = apply_json(&fixture, "start");
    assert!(reasons(&result).contains(&"existing_owner_runtime_policy".to_owned()));
    assert_eq!(read(&fixture, BUDGET), "1");
    assert_eq!(read(&fixture, &disable_state()), "1");
}

#[test]
fn readback_mismatch_or_enable_write_failure_rolls_back() {
    // Python's `budget_readback` fault (the -1 write alone ignored) has no file that does it: a
    // budget that never reads back is refused before any write (existing_owner_runtime_policy).
    // `state_write`: the dormant state refuses every write, the rollback's too. Root writes anyway.
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let fixture = qualified();
    let control = fixture.path(&disable_state());
    fs::set_permissions(&control, fs::Permissions::from_mode(0o444)).unwrap();
    let result = apply_json(&fixture, "start");
    assert_eq!(result["state"], "Failed");
    let errors: Vec<String> = serde_json::from_value(result["rollback_errors"].clone()).unwrap();
    assert_eq!(errors, [format!("{}:write", disable_state())]);
    assert_quarantined(&fixture);
}

#[test]
fn stop_quarantines_and_cannot_be_undone_by_start() {
    let fixture = qualified();
    put(&fixture, BUDGET, "-1");
    put(&fixture, &disable_state(), "0");
    assert_eq!(apply_json(&fixture, "stop")["state"], "Disabled");
    assert_quarantined(&fixture);
    let result = apply_json(&fixture, "start");
    assert!(reasons(&result).contains(&"already_applied_this_boot".to_owned()));
}

#[test]
fn unavailable_preflight_is_not_interpreted_as_ready() {
    let fixture = qualified();
    fixture.remove(PREFLIGHT);
    let result = apply_json(&fixture, "start");
    assert_eq!(reasons(&result).len(), FOUNDATIONS.len());
    assert_quarantined(&fixture);
}

#[test]
fn lost_boot_receipt_fails_closed_after_controls_were_enabled() {
    let fixture = qualified();
    // The receipt cannot replace a directory: atomic_json's rename fails after the enable writes.
    fixture.put(&format!("{RECORD}/occupied"), "");
    let error = apply(&fixture.root, "start").err().unwrap();
    assert!(matches!(error, Failure::Os { .. }), "{}", error.message());
    assert_quarantined(&fixture);
}

#[test]
fn failed_stop_readback_does_not_report_disabled() {
    let fixture = qualified();
    // A budget that takes writes but never reads them back.
    fixture.remove(BUDGET);
    std::os::unix::fs::symlink("/dev/null", fixture.path(BUDGET)).unwrap();
    let result = apply_json(&fixture, "stop");
    assert_eq!(result["state"], "Failed");
    assert!(reasons(&result).contains(&format!("{BUDGET}:readback")));
    assert_eq!(read(&fixture, &disable_state()), "1");
}
